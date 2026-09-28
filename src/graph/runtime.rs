use std::fs::{self, File, OpenOptions};
use std::path::Path;
use std::time::{Duration, Instant};

use oxigraph::store::Store;

use super::{QueryConfig, SelectResult, graph_error, projection, query};
use crate::storage::{
    StoreConfig,
    database::SqliteDatabase,
    graph_snapshot::{self, GraphSnapshot},
};
use crate::{CommonplaceError, Result};

const LOCK_TIMEOUT: Duration = Duration::from_secs(2);

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;

pub struct GraphRuntime {
    // Field order keeps the native store alive only while the lease is held.
    store: Store,
    _lease: File,
}

impl GraphRuntime {
    pub fn open(root: &Path) -> Result<Self> {
        StoreConfig::validate(root)?;
        let lease = publication_lock(root, false, LOCK_TIMEOUT)?;
        // Pin SQLite only after taking the lease; never compare to a pre-lease snapshot.
        let sqlite = SqliteDatabase::read(root)?;
        let version = graph_snapshot::version(sqlite.connection())?;
        let store = projection::open_verified(&root.join("graph/current"), version)?;
        Ok(Self {
            store,
            _lease: lease,
        })
    }

    pub fn query(&self, text: &str, config: QueryConfig) -> Result<SelectResult> {
        let parsed = query::parse(text)?;
        query::execute(&self.store, parsed, config)
    }
}

pub fn initialize(root: &Path) -> Result<()> {
    let graph = root.join("graph");
    fs::create_dir(&graph).map_err(graph_error)?;
    File::create_new(graph.join("publication.lock"))
        .map_err(graph_error)?
        .sync_all()
        .map_err(graph_error)?;
    let sqlite = SqliteDatabase::read(root)?;
    let snapshot = GraphSnapshot::read(sqlite.connection())?;
    projection::build(&snapshot, &graph.join("current"))?;
    sync_directory(&graph)?;
    Ok(())
}

pub fn rebuild(root: &Path, lock_timeout: Duration) -> Result<i64> {
    let _writer = SqliteDatabase::write(root, lock_timeout)?;
    let graph = root.join("graph");
    if !graph.try_exists()? {
        fs::create_dir(&graph).map_err(graph_error)?;
    }
    if fs::symlink_metadata(&graph)
        .map_err(graph_error)?
        .file_type()
        .is_symlink()
    {
        return Err(graph_error("graph directory must not be a symbolic link"));
    }
    let _lease = publication_lock(root, true, lock_timeout)?;
    remove_scratch(&graph.join("candidate"))?;
    remove_scratch(&graph.join("previous"))?;
    let sqlite = SqliteDatabase::read(root)?;
    let snapshot = GraphSnapshot::read(sqlite.connection())?;
    projection::build(&snapshot, &graph.join("candidate"))?;
    activate(&graph, |from, to| fs::rename(from, to), sync_directory)?;
    remove_scratch(&graph.join("previous")).map_err(|error| {
        graph_error(format!(
            "graph activated, but post-publication cleanup failed: {error}"
        ))
    })?;
    sync_directory(&graph)?;
    Ok(snapshot.knowledge_version)
}

fn publication_lock(root: &Path, exclusive: bool, timeout: Duration) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(exclusive)
        .create(exclusive)
        .truncate(false)
        .open(root.join("graph/publication.lock"))
        .map_err(graph_error)?;
    let start = Instant::now();
    loop {
        let result = if exclusive {
            file.try_lock()
        } else {
            file.try_lock_shared()
        };
        match result {
            Ok(()) => return Ok(file),
            Err(fs::TryLockError::WouldBlock) if start.elapsed() < timeout => {
                std::thread::sleep(
                    Duration::from_millis(10).min(timeout.saturating_sub(start.elapsed())),
                );
            }
            Err(fs::TryLockError::WouldBlock) => {
                return Err(CommonplaceError::Conflict(
                    "graph publication is busy; retry the operation".into(),
                ));
            }
            Err(fs::TryLockError::Error(error)) => return Err(graph_error(error)),
        }
    }
}

fn remove_scratch(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path).map_err(graph_error),
        Ok(_) => fs::remove_file(path).map_err(graph_error),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(graph_error(error)),
    }
}

fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(graph_error)
}

fn activate(
    graph: &Path,
    mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
    mut sync: impl FnMut(&Path) -> Result<()>,
) -> Result<()> {
    let current = graph.join("current");
    let previous = graph.join("previous");
    let candidate = graph.join("candidate");
    let had_current = current.try_exists().map_err(graph_error)?;
    if had_current {
        rename(&current, &previous).map_err(graph_error)?;
    }
    let mut activated = false;
    let activation = (|| {
        rename(&candidate, &current).map_err(graph_error)?;
        activated = true;
        sync(graph)
    })();
    if let Err(error) = activation {
        let restoration = (|| {
            if activated {
                rename(&current, &candidate).map_err(graph_error)?;
            }
            if had_current {
                rename(&previous, &current).map_err(graph_error)?;
            }
            sync(graph)
        })();
        return match restoration {
            Ok(()) => Err(graph_error(format!(
                "graph activation failed; previous state restored: {error}"
            ))),
            Err(restore_error) => Err(graph_error(format!(
                "graph activation failed: {error}; restoration failed: {restore_error}"
            ))),
        };
    }
    Ok(())
}
