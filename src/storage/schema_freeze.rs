use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use serde::Deserialize;

use crate::{CommonplaceError, Result};

const MARKER_FORMAT: &str = "commonplace-schema-freeze/1";
pub const MARKER_NAME: &str = "schema-freeze.json";
pub const PENDING_NAME: &str = ".schema-freeze.pending";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyState {
    Unfrozen,
    Frozen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreezeOutcome {
    pub created: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    format: String,
    schema_version: i64,
}

pub fn inspect_for_apply(root: &Path, schema_version: i64) -> Result<ApplyState> {
    if path_exists(&root.join(PENDING_NAME))? {
        return Err(CommonplaceError::Conflict(format!(
            "schema freeze is incomplete at {}; rerun schema freeze before applying vocabulary",
            root.join(PENDING_NAME).display()
        )));
    }
    if !path_exists(&root.join(MARKER_NAME))? {
        return Ok(ApplyState::Unfrozen);
    }
    validate_final(root, schema_version)?;
    Ok(ApplyState::Frozen)
}

pub fn freeze(root: &Path, schema_version: i64) -> Result<FreezeOutcome> {
    freeze_with(
        root,
        schema_version,
        |from, to| fs::rename(from, to),
        super::sync_directory,
    )
}

fn freeze_with(
    root: &Path,
    schema_version: i64,
    mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
    mut sync_directory: impl FnMut(&Path) -> std::io::Result<()>,
) -> Result<FreezeOutcome> {
    let final_path = root.join(MARKER_NAME);
    let pending_path = root.join(PENDING_NAME);
    if path_exists(&final_path)? {
        validate_final(root, schema_version)?;
        remove_pending(&pending_path)?;
        sync_after_install(root, schema_version, &mut sync_directory)?;
        return Ok(FreezeOutcome { created: false });
    }

    remove_pending(&pending_path)?;
    let bytes = format!("{{\"format\":\"{MARKER_FORMAT}\",\"schema_version\":{schema_version}}}\n");
    let write_result = (|| -> std::io::Result<()> {
        let mut pending = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pending_path)?;
        pending.write_all(bytes.as_bytes())?;
        pending.flush()?;
        pending.sync_all()?;
        rename(&pending_path, &final_path)?;
        Ok(())
    })();
    if let Err(error) = write_result {
        return Err(CommonplaceError::Storage(format!(
            "failed to install schema freeze marker at {} from {}: {error}. Rerun schema freeze; it is idempotent and replaces pending scratch",
            final_path.display(),
            pending_path.display()
        )));
    }
    sync_after_install(root, schema_version, &mut sync_directory)?;
    Ok(FreezeOutcome { created: true })
}

fn validate_final(root: &Path, schema_version: i64) -> Result<()> {
    let path = root.join(MARKER_NAME);
    let metadata = fs::symlink_metadata(&path).map_err(|error| marker_error(&path, error))?;
    if !metadata.file_type().is_file() {
        return Err(CommonplaceError::Conflict(format!(
            "schema freeze marker is not a regular file: {}",
            path.display()
        )));
    }
    let bytes = fs::read(&path).map_err(|error| marker_error(&path, error))?;
    let marker: Marker = serde_json::from_slice(&bytes).map_err(|error| {
        CommonplaceError::Conflict(format!(
            "schema freeze marker is invalid at {}: {error}",
            path.display()
        ))
    })?;
    if marker.format != MARKER_FORMAT {
        return Err(CommonplaceError::Conflict(format!(
            "unsupported schema freeze marker format {:?} at {}",
            marker.format,
            path.display()
        )));
    }
    if marker.schema_version < 0 || marker.schema_version != schema_version {
        return Err(CommonplaceError::Conflict(format!(
            "schema freeze marker version {} does not match SQLite schema version {schema_version} at {}",
            marker.schema_version,
            path.display()
        )));
    }
    let expected =
        format!("{{\"format\":\"{MARKER_FORMAT}\",\"schema_version\":{schema_version}}}\n");
    if bytes != expected.as_bytes() {
        return Err(CommonplaceError::Conflict(format!(
            "schema freeze marker does not use the canonical representation at {}",
            path.display()
        )));
    }
    Ok(())
}

fn remove_pending(path: &Path) -> Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(CommonplaceError::Io(error).context(format!(
                "cannot inspect schema freeze scratch {}",
                path.display()
            )));
        }
    };
    let removal = if metadata.file_type().is_dir() {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    };
    removal.map_err(|error| {
        CommonplaceError::Storage(format!(
            "failed to clear schema freeze pending scratch at {}: {error}. Rerun schema freeze after the reserved pending path can be removed",
            path.display()
        ))
    })?;
    Ok(())
}

fn sync_after_install(
    root: &Path,
    schema_version: i64,
    sync_directory: &mut impl FnMut(&Path) -> std::io::Result<()>,
) -> Result<()> {
    sync_directory(root).map_err(|error| {
        CommonplaceError::Storage(format!(
            "schema freeze at version {schema_version} may already be installed, but store directory {} durability could not be confirmed: {error}. Rerun schema freeze; it is idempotent",
            root.display()
        ))
    })
}

fn path_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CommonplaceError::Io(error).context(format!(
            "cannot inspect schema freeze path {}",
            path.display()
        ))),
    }
}

fn marker_error(path: &Path, error: std::io::Error) -> CommonplaceError {
    CommonplaceError::Storage(format!(
        "schema freeze marker is unavailable at {}: {error}",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::{MARKER_NAME, PENDING_NAME, freeze, freeze_with};

    #[test]
    fn retry_recovers_rename_and_post_rename_sync_failures() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let error = freeze_with(
            root,
            4,
            |_from, _to| Err(std::io::Error::other("rename failed")),
            super::super::sync_directory,
        )
        .unwrap_err();
        assert!(error.to_string().contains("rename failed"));
        assert!(error.to_string().contains("Rerun schema freeze"));
        assert!(root.join(PENDING_NAME).is_file());
        assert!(!root.join(MARKER_NAME).exists());
        assert!(freeze(root, 4).unwrap().created);

        let other = tempfile::tempdir().unwrap();
        let root = other.path();
        let error = freeze_with(
            root,
            7,
            |from, to| std::fs::rename(from, to),
            |_path| Err(std::io::Error::other("sync failed")),
        )
        .unwrap_err();
        assert!(error.to_string().contains("may already be installed"));
        assert!(root.join(MARKER_NAME).is_file());
        assert!(!root.join(PENDING_NAME).exists());
        assert!(!freeze(root, 7).unwrap().created);
    }
}
