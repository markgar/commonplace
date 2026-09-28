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
    pending(&transaction);
    let snapshot = GraphSnapshot::read(&transaction).unwrap();
    assert_eq!(snapshot.knowledge_version, 1);
    projection::build(&snapshot, &root.join("graph/candidate")).unwrap();
    let candidate = projection::open_verified(&root.join("graph/candidate"), 1).unwrap();
    assert_eq!(candidate.len().unwrap(), 30);
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

fn pending(transaction: &Transaction<'_>) {
    transaction.execute_batch(
        "INSERT INTO entity_types VALUES (1,'person',NULL,1);
         INSERT INTO entities VALUES (1,'Pending','2026-01-01T00:00:00Z',NULL);
         INSERT INTO knowledge_items VALUES (1,'type_membership',1,'2026-01-01T00:00:00Z',NULL,NULL,NULL);
         INSERT INTO entity_type_memberships VALUES (1,1,1);
         INSERT INTO predicates VALUES (1,'knows','entity',NULL,1),(2,'decision','string',NULL,1);
         INSERT INTO predicate_entity_types VALUES (1,'subject',1),(1,'object',1),(2,'subject',1);
         INSERT INTO knowledge_items VALUES (2,'fact',1,'2026-01-01T00:00:00Z',NULL,NULL,NULL),
           (3,'fact',1,'2026-01-01T00:00:00Z',NULL,NULL,NULL);
         INSERT INTO facts VALUES (2,1,1,1,NULL),(3,1,2,NULL,'\"approved\"');
         UPDATE store_state SET schema_version=1,knowledge_version=1;"
    ).unwrap();
}

fn committed_version(root: &Path) -> i64 {
    graph_snapshot::version(SqliteDatabase::read(root).unwrap().connection()).unwrap()
}

#[test]
fn coordinated_build_activation_and_real_commit_failures_restore_old_state() {
    for phase in [
        PublishPhase::BeforeBuild,
        PublishPhase::AfterBuild,
        PublishPhase::BetweenRenames,
        PublishPhase::AfterActivation,
        PublishPhase::BeforeCommit,
    ] {
        let (_directory, root) = store();
        let graph = root.join("graph");
        let before = bytes(&graph.join("current"));
        let mut writer = SqliteDatabase::write(&root, Duration::ZERO).unwrap();
        let transaction = writer.transaction().unwrap();
        pending(&transaction);
        let error = publish_with(&root, &transaction, Duration::ZERO, "receipt", |at, tx| {
            if at == PublishPhase::AfterBuild {
                let candidate = projection::open_verified(&graph.join("candidate"), 1)?;
                assert_eq!(candidate.len().unwrap(), 30);
                assert_eq!(committed_version(&root), 0);
            }
            if at == phase {
                if at == PublishPhase::BeforeCommit {
                    // Introduce the deferred violation only after successful candidate verification.
                    tx.execute_batch(
                        "PRAGMA defer_foreign_keys=ON;
                        INSERT INTO entity_aliases VALUES (999,'bad','2026-01-01T00:00:00Z',NULL);",
                    )
                    .unwrap();
                    return Ok(());
                }
                return Err(graph_error("injected phase failure"));
            }
            Ok(())
        })
        .unwrap_err();
        if phase == PublishPhase::BeforeCommit {
            assert!(
                error.to_string().contains("FOREIGN KEY constraint failed"),
                "{error}"
            );
            assert!(transaction.is_autocommit());
        }
        drop(transaction);
        drop(writer);
        assert_eq!(committed_version(&root), 0);
        let read = SqliteDatabase::read(&root).unwrap();
        let facts: i64 = read
            .connection()
            .query_row("SELECT count(*) FROM facts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(facts, 0);
        drop(read);
        assert_eq!(bytes(&graph.join("current")), before);
        assert!(!graph.join("candidate").exists());
        assert!(!graph.join("previous").exists());
        GraphRuntime::open(&root).unwrap();
    }
}

#[test]
fn commit_restoration_failure_is_explicit_and_fail_closed() {
    let (_directory, root) = store();
    let mut writer = SqliteDatabase::write(&root, Duration::ZERO).unwrap();
    let transaction = writer.transaction().unwrap();
    pending(&transaction);
    let error = publish_with(
        &root,
        &transaction,
        Duration::ZERO,
        "receipt",
        |phase, _| {
            if matches!(
                phase,
                PublishPhase::BeforeCommit | PublishPhase::BeforeRestore
            ) {
                return Err(graph_error("injected commit or restoration failure"));
            }
            Ok(())
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("restoration failed"));
    assert!(transaction.is_autocommit());
    drop(transaction);
    drop(writer);
    assert_eq!(committed_version(&root), 0);
    assert!(GraphRuntime::open(&root).is_err());
    rebuild(&root, Duration::ZERO).unwrap();
    GraphRuntime::open(&root).unwrap();
}

#[test]
fn all_post_commit_cleanup_errors_preserve_committed_receipt_and_current() {
    for phase in [PublishPhase::AfterCommit, PublishPhase::AfterCleanup] {
        let (_directory, root) = store();
        let mut writer = SqliteDatabase::write(&root, Duration::ZERO).unwrap();
        let transaction = writer.transaction().unwrap();
        pending(&transaction);
        let error = publish_with(
            &root,
            &transaction,
            Duration::ZERO,
            "record committed at knowledge_version 1 (entity:1, knowledge:1, knowledge:2, knowledge:3)",
            |at, _| {
                if at == phase {
                    return Err(graph_error("injected cleanup/sync failure"));
                }
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(error.code(), "post_commit_cleanup");
        assert!(
            error
                .to_string()
                .contains("knowledge_version 1 (entity:1, knowledge:1, knowledge:2, knowledge:3)")
        );
        assert!(error.to_string().contains("Do not retry record"));
        assert!(transaction.is_autocommit());
        drop(transaction);
        drop(writer);
        assert_eq!(committed_version(&root), 1);
        let read = SqliteDatabase::read(&root).unwrap();
        let fact = crate::storage::knowledge::knowledge(
            read.connection(),
            crate::domain::ids::KnowledgeItemId::new(3).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            fact.detail,
            crate::domain::knowledge::KnowledgeDetail::Fact { .. }
        ));
        assert!(fact.support.is_empty());
        drop(read);
        let graph = GraphRuntime::open(&root).unwrap();
        let facts = graph
            .query(
                "SELECT ?k WHERE {?k <urn:commonplace:property:kind> \"fact\"}",
                QueryConfig::default(),
            )
            .unwrap();
        assert_eq!(facts.rows.len(), 2);
        drop(graph);
        rebuild(&root, Duration::ZERO).unwrap();
        assert_eq!(committed_version(&root), 1);
    }
}

#[test]
fn actual_process_exit_windows_fail_closed_and_rebuild_only_committed_state() {
    for phase in ["BetweenRenames", "AfterActivation", "AfterCommit"] {
        let (_directory, root) = store();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "graph::runtime::tests::publication_crash_child",
            ])
            .env("COMMONPLACE_CRASH_STORE", &root)
            .env("COMMONPLACE_CRASH_PHASE", phase)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(77));
        let committed = phase == "AfterCommit";
        assert_eq!(committed_version(&root), i64::from(committed));
        assert_eq!(GraphRuntime::open(&root).is_ok(), committed);
        rebuild(&root, Duration::ZERO).unwrap();
        assert_eq!(committed_version(&root), i64::from(committed));
        let graph = GraphRuntime::open(&root).unwrap();
        let result = graph
            .query("SELECT ?s WHERE {?s ?p ?o}", QueryConfig::default())
            .unwrap();
        assert_eq!(result.rows.is_empty(), !committed);
        assert!(!root.join("graph/previous").exists());
        assert!(!root.join("graph/candidate").exists());
    }
}

#[test]
#[ignore = "child helper explicitly invoked by process-crash test"]
fn publication_crash_child() {
    let root = std::path::PathBuf::from(std::env::var_os("COMMONPLACE_CRASH_STORE").unwrap());
    let phase = std::env::var("COMMONPLACE_CRASH_PHASE").unwrap();
    let mut writer = SqliteDatabase::write(&root, Duration::ZERO).unwrap();
    let transaction = writer.transaction().unwrap();
    pending(&transaction);
    publish_with(&root, &transaction, Duration::ZERO, "receipt", |at, _| {
        if format!("{at:?}") == phase {
            std::process::exit(77);
        }
        Ok(())
    })
    .unwrap();
    panic!("expected exit phase");
}
