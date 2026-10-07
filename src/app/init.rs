use std::path::{Path, PathBuf};

use serde::Serialize;
use tempfile::Builder;

use crate::graph;
use crate::storage::StoreConfig;
use crate::storage::database::{STORE_FORMAT, SqliteDatabase};
use crate::{CommonplaceError, Result};

#[derive(Debug, Serialize)]
pub struct InitResult {
    pub path: PathBuf,
    pub format: &'static str,
    pub created: bool,
}

pub fn initialize(requested_root: &Path) -> Result<InitResult> {
    initialize_inner(requested_root)
        .map_err(|error| error.context(format!("initialize store {}", requested_root.display())))
}

fn initialize_inner(requested_root: &Path) -> Result<InitResult> {
    let root = absolute_path(requested_root)?;
    let database_path = root.join("commonplace.sqlite3");

    let metadata = match std::fs::metadata(&root) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(CommonplaceError::Io(error).context("cannot inspect store path")),
    };
    if let Some(metadata) = metadata {
        if !metadata.is_dir() {
            return Err(CommonplaceError::Conflict(format!(
                "knowledge-base path is not a directory: {}",
                root.display()
            )));
        }

        StoreConfig::validate(&root)?;
        SqliteDatabase::validate(&database_path)?;
        drop(graph::GraphRuntime::open(&root)?);

        return Ok(InitResult {
            path: root,
            format: STORE_FORMAT,
            created: false,
        });
    }

    let parent = root.parent().ok_or_else(|| {
        CommonplaceError::InvalidInput(format!(
            "knowledge-base path has no parent: {}",
            root.display()
        ))
    })?;
    std::fs::create_dir_all(parent)?;

    let staging = Builder::new()
        .prefix(".commonplace-init-")
        .tempdir_in(parent)?;
    let staging_root = staging.path();
    let staging_database = staging_root.join("commonplace.sqlite3");
    let staging_config = staging_root.join("config.json");

    SqliteDatabase::initialize(&staging_database)?;
    std::fs::write(
        &staging_config,
        serde_json::to_vec_pretty(&StoreConfig::expected())?,
    )?;
    graph::initialize(staging_root)?;

    let staging_path = staging.keep();
    if let Err(error) = std::fs::rename(&staging_path, &root) {
        let cleanup = std::fs::remove_dir_all(&staging_path);
        let mut error = CommonplaceError::Io(error).context(format!(
            "cannot publish staging store {} to {}",
            staging_path.display(),
            root.display()
        ));
        if let Err(cleanup) = cleanup {
            error = error.context(format!("staging cleanup also failed: {cleanup}"));
        }
        return Err(error);
    }

    Ok(InitResult {
        path: root,
        format: STORE_FORMAT,
        created: true,
    })
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(CommonplaceError::InvalidInput(
            "knowledge-base path cannot be empty".to_owned(),
        ));
    }

    if path.is_absolute() {
        Ok(path.to_owned())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

#[cfg(test)]
mod tests {
    use super::initialize;

    #[test]
    fn initialization_is_atomic_and_idempotent() {
        let parent = tempfile::tempdir().expect("temporary parent");
        let root = parent.path().join("knowledge");

        let created = initialize(&root).expect("initialize store");
        assert!(created.created);
        assert!(root.join("commonplace.sqlite3").is_file());
        assert!(root.join("graph/current").is_dir());
        assert!(root.join("config.json").is_file());

        let reopened = initialize(&root).expect("validate existing store");
        assert!(!reopened.created);
    }

    #[test]
    fn existing_uninitialized_directory_is_rejected() {
        let parent = tempfile::tempdir().expect("temporary parent");
        let root = parent.path().join("knowledge");
        std::fs::create_dir(&root).expect("create directory");

        let error = initialize(&root).expect_err("uninitialized directory must fail");
        assert_eq!(error.code(), "conflict");
    }
}
