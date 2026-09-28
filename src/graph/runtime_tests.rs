use super::*;
use oxigraph::sparql::{QueryResults, SparqlEvaluator};

fn store() -> (tempfile::TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("store");
    crate::app::init::initialize(&root).unwrap();
    (directory, root)
}

fn bytes(path: &Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn visit(
        root: &Path,
        path: &Path,
        result: &mut std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>,
    ) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, result);
            } else {
                result.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut result = std::collections::BTreeMap::new();
    visit(path, path, &mut result);
    result
}

#[test]
fn snapshot_reads_callers_pending_transaction_not_another_connection() {
    let (_directory, root) = store();
    let mut writer = SqliteDatabase::write(&root, Duration::ZERO).unwrap();
    let transaction = writer.transaction().unwrap();
    transaction.execute_batch(
        "INSERT INTO entity_types VALUES (1,'person',NULL,1);
         INSERT INTO entities VALUES (1,'Pending','2026-01-01T00:00:00Z',NULL);
         INSERT INTO knowledge_items VALUES (1,'type_membership',1,'2026-01-01T00:00:00Z',NULL,NULL,NULL);
         INSERT INTO entity_type_memberships VALUES (1,1,1);
         UPDATE store_state SET schema_version=1,knowledge_version=1;"
    ).unwrap();
    let snapshot = GraphSnapshot::read(&transaction).unwrap();
    assert_eq!(snapshot.knowledge_version, 1);
    projection::build(&snapshot, &root.join("graph/candidate")).unwrap();
    let candidate = projection::open_verified(&root.join("graph/candidate"), 1).unwrap();
    assert_eq!(candidate.len().unwrap(), 10);
    let committed = SqliteDatabase::read(&root).unwrap();
    assert_eq!(graph_snapshot::version(committed.connection()).unwrap(), 0);
    drop(candidate);
    drop(transaction);
    drop(writer);
    assert_eq!(
        GraphRuntime::open(&root)
            .unwrap()
            .query("SELECT ?s WHERE {?s ?p ?o}", QueryConfig::default())
            .unwrap()
            .rows
            .len(),
        0
    );
}

#[test]
fn activation_failures_restore_exact_previous_directory() {
    for fault in ["first-rename", "second-rename", "sync"] {
        let (_directory, root) = store();
        let graph = root.join("graph");
        let _writer = SqliteDatabase::write(&root, Duration::ZERO).unwrap();
        let _lease = publication_lock(&root, true, Duration::ZERO).unwrap();
        let sqlite = SqliteDatabase::read(&root).unwrap();
        let snapshot = GraphSnapshot::read(sqlite.connection()).unwrap();
        projection::build(&snapshot, &graph.join("candidate")).unwrap();
        let before = bytes(&graph.join("current"));
        let mut renames = 0;
        let mut syncs = 0;
        let error = activate(
            &graph,
            |from, to| {
                renames += 1;
                if (fault == "first-rename" && renames == 1)
                    || (fault == "second-rename" && renames == 2)
                {
                    return Err(std::io::Error::other("injected rename failure"));
                }
                fs::rename(from, to)
            },
            |path| {
                syncs += 1;
                if fault == "sync" && syncs == 1 {
                    return Err(graph_error("injected sync failure"));
                }
                sync_directory(path)
            },
        )
        .unwrap_err();
        assert_eq!(error.code(), "graph_unavailable");
        assert_eq!(bytes(&graph.join("current")), before);
        drop(projection::open_verified(&graph.join("current"), 0).unwrap());
        assert!(!graph.join("previous").exists());
    }
}

#[test]
fn failed_restoration_is_explicit_and_missing_current_stays_unavailable() {
    let (_directory, root) = store();
    let graph = root.join("graph");
    {
        let _writer = SqliteDatabase::write(&root, Duration::ZERO).unwrap();
        let _lease = publication_lock(&root, true, Duration::ZERO).unwrap();
        let sqlite = SqliteDatabase::read(&root).unwrap();
        projection::build(
            &GraphSnapshot::read(sqlite.connection()).unwrap(),
            &graph.join("candidate"),
        )
        .unwrap();
        let mut calls = 0;
        let error = activate(
            &graph,
            |from, to| {
                calls += 1;
                if calls >= 2 {
                    return Err(std::io::Error::other(
                        "injected activation/restoration failure",
                    ));
                }
                fs::rename(from, to)
            },
            sync_directory,
        )
        .unwrap_err();
        assert!(error.to_string().contains("restoration failed"));
    }
    assert!(GraphRuntime::open(&root).is_err());
    assert!(!graph.join("current").exists());
    rebuild(&root, Duration::ZERO).unwrap();
    GraphRuntime::open(&root).unwrap();
}

#[test]
fn activation_failure_without_old_current_preserves_absence() {
    let (_directory, root) = store();
    let graph = root.join("graph");
    let _writer = SqliteDatabase::write(&root, Duration::ZERO).unwrap();
    let _lease = publication_lock(&root, true, Duration::ZERO).unwrap();
    fs::rename(graph.join("current"), graph.join("candidate")).unwrap();
    let mut calls = 0;
    let error = activate(
        &graph,
        |from, to| fs::rename(from, to),
        |path| {
            calls += 1;
            if calls == 1 {
                return Err(graph_error("injected sync failure"));
            }
            sync_directory(path)
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("previous state restored"));
    assert!(!graph.join("current").exists());
    assert!(graph.join("candidate").is_dir());
}

#[test]
fn result_snapshot_outlives_native_store_but_not_publication_lease() {
    let (_directory, root) = store();
    let GraphRuntime {
        store,
        _lease: lease,
    } = GraphRuntime::open(&root).unwrap();
    let results = SparqlEvaluator::new()
        .parse_query("SELECT ?s WHERE {GRAPH ?g {?s ?p ?o}}")
        .unwrap()
        .on_store(&store)
        .execute()
        .unwrap();
    drop(store);
    let contender = OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.join("graph/publication.lock"))
        .unwrap();
    assert!(matches!(
        contender.try_lock(),
        Err(fs::TryLockError::WouldBlock)
    ));
    let QueryResults::Solutions(mut solutions) = results else {
        panic!("SELECT")
    };
    assert!(solutions.next().unwrap().is_ok());
    drop(solutions);
    assert!(matches!(
        contender.try_lock(),
        Err(fs::TryLockError::WouldBlock)
    ));
    drop(lease);
    contender.try_lock().unwrap();
}
