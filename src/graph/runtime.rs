use std::fs::{self, File, OpenOptions};
use std::path::Path;
use std::time::{Duration, Instant};

use oxigraph::store::Store;
use rusqlite::Transaction;

use super::{QueryConfig, SelectResult, graph_error, projection, query};
use crate::storage::{
    StoreConfig,
    database::SqliteDatabase,
    graph_snapshot::{self, GraphSnapshot},
};
use crate::{CommonplaceError, Result};

const LOCK_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PublishPhase {
    BeforeBuild,
    AfterBuild,
    BetweenRenames,
    AfterActivation,
    BeforeCommit,
    BeforeRestore,
    AfterCommit,
    AfterCleanup,
}

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
    fs::create_dir(&graph).map_err(|error| graph_io("create graph directory", &graph, error))?;
    File::create_new(graph.join("publication.lock"))
        .map_err(|error| {
            graph_io(
                "create publication lock",
                &graph.join("publication.lock"),
                error,
            )
        })?
        .sync_all()
        .map_err(|error| {
            graph_io(
                "synchronize publication lock",
                &graph.join("publication.lock"),
                error,
            )
        })?;
    let sqlite = SqliteDatabase::read(root)?;
    let snapshot = GraphSnapshot::read(sqlite.connection())?;
    projection::build(&snapshot, &graph.join("current"))?;
    sync_directory(&graph)?;
    Ok(())
}

pub fn rebuild(root: &Path, lock_timeout: Duration) -> Result<i64> {
    let _writer = SqliteDatabase::write(root, lock_timeout)?;
    let graph = root.join("graph");
    if !graph
        .try_exists()
        .map_err(|error| graph_io("inspect graph directory", &graph, error))?
    {
        fs::create_dir(&graph)
            .map_err(|error| graph_io("create graph directory", &graph, error))?;
    }
    validate_graph_directory(&graph)?;
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

pub(crate) fn publish(
    root: &Path,
    transaction: &Transaction<'_>,
    timeout: Duration,
    receipt: &str,
    recovery_guidance: &str,
) -> Result<()> {
    publish_with(
        root,
        transaction,
        timeout,
        receipt,
        recovery_guidance,
        |_, _| Ok(()),
    )
}

fn publish_with(
    root: &Path,
    transaction: &Transaction<'_>,
    timeout: Duration,
    receipt: &str,
    recovery_guidance: &str,
    mut hook: impl FnMut(PublishPhase, &Transaction<'_>) -> Result<()>,
) -> Result<()> {
    let graph = root.join("graph");
    validate_graph_directory(&graph)?;
    for scratch in ["candidate", "previous"] {
        match fs::symlink_metadata(graph.join(scratch)) {
            Ok(_) => {
                return Err(graph_error(format!(
                    "leftover graph/{scratch} requires explicit recovery"
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(graph_io(
                    "inspect graph scratch",
                    &graph.join(scratch),
                    error,
                ));
            }
        }
    }
    // Validate the committed graph under a reader lease, then close it before publication.
    drop(GraphRuntime::open(root)?);
    let prepared = (|| {
        hook(PublishPhase::BeforeBuild, transaction)?;
        let snapshot = GraphSnapshot::read(transaction)?;
        projection::build(&snapshot, &graph.join("candidate"))?;
        hook(PublishPhase::AfterBuild, transaction)
    })();
    if let Err(error) = prepared {
        return discard_candidate(&graph, error);
    }
    let lease = match publication_lock(root, true, timeout) {
        Ok(lease) => lease,
        Err(error) => return discard_candidate(&graph, error),
    };
    let activation = activate(
        &graph,
        |from, to| {
            if from == graph.join("candidate") {
                hook(PublishPhase::BetweenRenames, transaction).map_err(std::io::Error::other)?;
            }
            fs::rename(from, to)
        },
        sync_directory,
    );
    if let Err(error) = activation {
        // A failed restore retains scratch as explicit recovery evidence.
        if graph.join("previous").try_exists().map_err(graph_error)? {
            return Err(error);
        }
        return discard_candidate(&graph, error);
    }
    let committed = (|| {
        hook(PublishPhase::AfterActivation, transaction)?;
        hook(PublishPhase::BeforeCommit, transaction)?;
        transaction
            .execute_batch("COMMIT")
            .map_err(crate::storage::database::storage_error)
    })();
    if let Err(error) = committed {
        let rollback = if transaction.is_autocommit() {
            Ok(())
        } else {
            transaction
                .execute_batch("ROLLBACK")
                .map_err(crate::storage::database::storage_error)
        };
        let restoration = hook(PublishPhase::BeforeRestore, transaction).and_then(|()| {
            restore(
                &graph,
                true,
                true,
                |from, to| fs::rename(from, to),
                sync_directory,
            )
        });
        let mut message = format!("knowledge commit failed: {error}");
        let rollback_failed = rollback.is_err();
        if let Err(error) = rollback {
            message.push_str(&format!("; SQLite rollback failed: {error}"));
        }
        if let Err(error) = restoration {
            message.push_str(&format!("; graph restoration failed: {error}"));
            return Err(graph_error(message));
        }
        let cause = if rollback_failed {
            graph_error(message)
        } else {
            error.context("knowledge commit failed; SQLite rolled back and previous graph restored")
        };
        let result = discard_candidate(&graph, cause);
        drop(lease);
        return result;
    }
    let cleanup = (|| {
        hook(PublishPhase::AfterCommit, transaction)?;
        remove_scratch(&graph.join("previous"))?;
        hook(PublishPhase::AfterCleanup, transaction)?;
        sync_directory(&graph)
    })();
    cleanup.map_err(|error| {
        CommonplaceError::PostCommitCleanup(format!(
            "{receipt}; graph cleanup failed: {error}. {} and run graph rebuild.",
            recovery_guidance.trim_end_matches('.')
        ))
    })
}

fn discard_candidate(graph: &Path, error: CommonplaceError) -> Result<()> {
    match remove_scratch(&graph.join("candidate")).and_then(|()| sync_directory(graph)) {
        Ok(()) => Err(error),
        Err(cleanup) => Err(graph_error(format!(
            "{error}; candidate cleanup failed: {cleanup}"
        ))),
    }
}

fn validate_graph_directory(graph: &Path) -> Result<()> {
    if fs::symlink_metadata(graph)
        .map_err(|error| graph_io("inspect graph directory", graph, error))?
        .file_type()
        .is_symlink()
    {
        return Err(graph_error("graph directory must not be a symbolic link"));
    }
    Ok(())
}

fn publication_lock(root: &Path, exclusive: bool, timeout: Duration) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(exclusive)
        .create(exclusive)
        .truncate(false)
        .open(root.join("graph/publication.lock"))
        .map_err(|error| {
            graph_io(
                "open publication lock",
                &root.join("graph/publication.lock"),
                error,
            )
        })?;
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
                return Err(CommonplaceError::Conflict(format!(
                    "graph publication is busy at {}; retry the operation",
                    root.join("graph/publication.lock").display()
                )));
            }
            Err(fs::TryLockError::Error(error)) => {
                return Err(graph_io(
                    "acquire publication lock",
                    &root.join("graph/publication.lock"),
                    error,
                ));
            }
        }
    }
}

fn remove_scratch(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path)
            .map_err(|error| graph_io("remove graph scratch directory", path, error)),
        Ok(_) => {
            fs::remove_file(path).map_err(|error| graph_io("remove graph scratch", path, error))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(graph_io("inspect graph scratch", path, error)),
    }
}

fn sync_directory(path: &Path) -> Result<()> {
    crate::storage::sync_directory(path)
        .map_err(|error| graph_io("synchronize graph directory", path, error))
}

fn graph_io(operation: &str, path: &Path, error: std::io::Error) -> CommonplaceError {
    graph_error(format!("{operation} {}: {error}", path.display()))
}

fn activate(
    graph: &Path,
    mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
    mut sync: impl FnMut(&Path) -> Result<()>,
) -> Result<()> {
    let current = graph.join("current");
    let previous = graph.join("previous");
    let candidate = graph.join("candidate");
    let had_current = current
        .try_exists()
        .map_err(|error| graph_io("inspect current graph", &current, error))?;
    if had_current {
        rename(&current, &previous)
            .map_err(|error| graph_io("retain current graph as previous", graph, error))?;
    }
    let mut activated = false;
    let activation = (|| {
        rename(&candidate, &current)
            .map_err(|error| graph_io("activate candidate graph as current", graph, error))?;
        activated = true;
        sync(graph)
    })();
    if let Err(error) = activation {
        let restoration = restore(graph, had_current, activated, &mut rename, &mut sync);
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

fn restore(
    graph: &Path,
    had_current: bool,
    activated: bool,
    mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
    mut sync: impl FnMut(&Path) -> Result<()>,
) -> Result<()> {
    if activated {
        rename(&graph.join("current"), &graph.join("candidate"))
            .map_err(|error| graph_io("restore current graph to candidate", graph, error))?;
    }
    if had_current {
        rename(&graph.join("previous"), &graph.join("current"))
            .map_err(|error| graph_io("restore previous graph to current", graph, error))?;
    }
    sync(graph)
}
