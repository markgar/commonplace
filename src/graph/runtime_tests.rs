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

fn removal_store() -> (tempfile::TempDir, std::path::PathBuf) {
    let (directory, root) = store();
    let mut writer = SqliteDatabase::write(&root, Duration::ZERO).unwrap();
    let transaction = writer.transaction().unwrap();
    pending(&transaction);
    let mut vector = vec![0.0; crate::providers::embeddings::EMBEDDING_DIMENSIONS];
    vector[0] = 1.0;
    crate::storage::documents::publish(
        &transaction,
        &crate::domain::documents::DocumentInput {
            source_key: "source".into(),
            text: "evidence".into(),
            title: None,
            source_type: "test".into(),
            occurred_at: None,
            temporal_state: crate::domain::documents::TemporalState::Unknown,
            metadata: Default::default(),
        },
        None,
        Some(&crate::storage::documents::PreparedDocument {
            ranges: vec![crate::domain::passages::PassageRange {
                ordinal: 0,
                start_byte: 0,
                end_byte: 8,
            }],
            vectors: vec![vector],
        }),
        "2026-01-01T00:00:00Z",
    )
    .unwrap();
    transaction
        .execute(
            "INSERT INTO knowledge_item_evidence VALUES (1,1),(2,1),(3,1)",
            [],
        )
        .unwrap();
    transaction.commit().unwrap();
    drop(writer);
    rebuild(&root, Duration::ZERO).unwrap();
    (directory, root)
}

#[test]
fn removal_publication_failures_rollback_indexes_evidence_and_restore_or_fail_closed() {
    for phase in [
        PublishPhase::BeforeBuild,
        PublishPhase::AfterBuild,
        PublishPhase::BetweenRenames,
        PublishPhase::AfterActivation,
        PublishPhase::BeforeCommit,
        PublishPhase::BeforeRestore,
        PublishPhase::AfterCommit,
        PublishPhase::AfterCleanup,
    ] {
        let (_directory, root) = removal_store();
        let graph = root.join("graph");
        let before = bytes(&graph.join("current"));
        let mut writer = SqliteDatabase::write(&root, Duration::ZERO).unwrap();
        let transaction = writer.transaction().unwrap();
        let removed = crate::storage::documents::remove(&transaction, "source").unwrap();
        assert_eq!(removed.detached_evidence, 3);
        transaction
            .execute("UPDATE store_state SET knowledge_version=2", [])
            .unwrap();
        let receipt = "remove committed at knowledge_version 2 (removed doc:1 with source_key \"source\"; affected knowledge: [knowledge:1, knowledge:2, knowledge:3])";
        let error = publish_with(
            &root, &transaction, Duration::ZERO, receipt, crate::app::remove::RECOVERY_GUIDANCE,
            |at, tx| {
                if at == PublishPhase::AfterBuild {
                    let candidate = projection::open_verified(&graph.join("candidate"), 2)?;
                    assert_eq!(candidate.len().unwrap(), 30);
                    assert_eq!(committed_version(&root), 1);
                }
                if at == phase || (phase == PublishPhase::BeforeRestore && at == PublishPhase::BeforeCommit) {
                    if phase == PublishPhase::BeforeCommit {
                        tx.execute_batch(
                            "PRAGMA defer_foreign_keys=ON;
                            INSERT INTO entity_aliases VALUES (999,'bad','2026-01-01T00:00:00Z',NULL);"
                        ).unwrap();
                        return Ok(());
                    }
                    return Err(graph_error("injected removal failure"));
                }
                Ok(())
            },
        ).unwrap_err();
        let committed = matches!(
            phase,
            PublishPhase::AfterCommit | PublishPhase::AfterCleanup
        );
        if committed {
            assert_eq!(error.code(), "post_commit_cleanup");
            assert!(error.to_string().contains(receipt));
            assert!(error.to_string().contains("Do not retry remove"));
            assert!(error.to_string().contains("run graph rebuild"));
            assert!(!error.to_string().contains("retry record"));
        } else if !transaction.is_autocommit() {
            transaction.execute_batch("ROLLBACK").unwrap();
        }
        if phase == PublishPhase::BeforeCommit {
            assert!(error.to_string().contains("FOREIGN KEY constraint failed"));
            assert!(transaction.is_autocommit());
        }
        drop(transaction);
        drop(writer);
        assert_eq!(committed_version(&root), if committed { 2 } else { 1 });
        let session = SqliteDatabase::read(&root).unwrap();
        for table in [
            "documents",
            "document_revisions",
            "passages",
            "passage_fts",
            "passage_vectors",
            "knowledge_item_evidence",
        ] {
            let count: i64 = session
                .connection()
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            let expected = i64::from(!committed)
                * if table == "knowledge_item_evidence" {
                    3
                } else {
                    1
                };
            assert_eq!(count, expected, "{phase:?}: {table}");
        }
        for id in 1..=3 {
            let knowledge = crate::storage::knowledge::knowledge(
                session.connection(),
                crate::domain::ids::KnowledgeItemId::new(id).unwrap(),
            )
            .unwrap();
            let knowledge = serde_json::to_value(knowledge).unwrap();
            assert_eq!(knowledge["withdrawn_at"], serde_json::Value::Null);
            assert_eq!(
                knowledge["support"].as_array().unwrap().len(),
                usize::from(!committed)
            );
            assert_eq!(
                knowledge["subtype"],
                if id == 1 { "type_membership" } else { "fact" }
            );
        }
        drop(session);
        if phase == PublishPhase::BeforeRestore {
            assert!(error.to_string().contains("restoration failed"));
            assert!(GraphRuntime::open(&root).is_err());
        } else {
            GraphRuntime::open(&root).unwrap();
            if !committed {
                assert_eq!(bytes(&graph.join("current")), before);
            }
        }
        rebuild(&root, Duration::ZERO).unwrap();
        GraphRuntime::open(&root).unwrap();
        assert_eq!(committed_version(&root), if committed { 2 } else { 1 });
    }
}

#[test]
fn withdrawal_publication_failures_preserve_history_and_restore_or_fail_closed() {
    use crate::domain::ids::KnowledgeItemId;
    use crate::storage::knowledge;

    for phase in [
        PublishPhase::BeforeBuild,
        PublishPhase::AfterBuild,
        PublishPhase::BetweenRenames,
        PublishPhase::AfterActivation,
        PublishPhase::BeforeCommit,
        PublishPhase::BeforeRestore,
        PublishPhase::AfterCommit,
        PublishPhase::AfterCleanup,
    ] {
        let (_directory, root) = removal_store();
        let graph = root.join("graph");
        let before = bytes(&graph.join("current"));
        let ids: Vec<_> = (1..=3)
            .map(|id| KnowledgeItemId::new(id).unwrap())
            .collect();
        let history: Vec<_> = {
            let read = SqliteDatabase::read(&root).unwrap();
            ids.iter()
                .map(|id| {
                    serde_json::to_value(knowledge::knowledge(read.connection(), *id).unwrap())
                        .unwrap()
                })
                .collect()
        };
        let mut writer = SqliteDatabase::write(&root, Duration::ZERO).unwrap();
        let transaction = writer.transaction().unwrap();
        let withdrawn =
            knowledge::withdraw(&transaction, &ids, "2026-09-28T13:00:00Z", Some("reviewer"))
                .unwrap();
        assert_eq!(withdrawn.len(), 3);
        transaction
            .execute("UPDATE store_state SET knowledge_version=2", [])
            .unwrap();
        let receipt =
            "withdraw committed at knowledge_version 2 (knowledge:1, knowledge:2, knowledge:3)";
        let error = publish_with(
            &root, &transaction, Duration::ZERO, receipt, crate::app::withdraw::RECOVERY_GUIDANCE,
            |at, tx| {
                if at == PublishPhase::AfterBuild {
                    let candidate = projection::open_verified(&graph.join("candidate"), 2)?;
                    assert_eq!(candidate.len().unwrap(), 1, "only version metadata remains");
                    assert_eq!(committed_version(&root), 1);
                }
                if at == phase || (phase == PublishPhase::BeforeRestore && at == PublishPhase::BeforeCommit) {
                    if phase == PublishPhase::BeforeCommit {
                        tx.execute_batch(
                            "PRAGMA defer_foreign_keys=ON;
                             INSERT INTO entity_aliases VALUES (999,'bad','2026-01-01T00:00:00Z',NULL);"
                        ).unwrap();
                        return Ok(());
                    }
                    return Err(graph_error("injected withdrawal failure"));
                }
                Ok(())
            },
        ).unwrap_err();
        let committed = matches!(
            phase,
            PublishPhase::AfterCommit | PublishPhase::AfterCleanup
        );
        if committed {
            assert_eq!(error.code(), "post_commit_cleanup");
            assert!(error.to_string().contains(receipt));
            assert!(error.to_string().contains("Do not retry withdraw"));
            assert!(error.to_string().contains("run graph rebuild"));
            assert!(!error.to_string().contains("retry record"));
            assert!(!error.to_string().contains("retry remove"));
        } else if !transaction.is_autocommit() {
            transaction.execute_batch("ROLLBACK").unwrap();
        }
        if phase == PublishPhase::BeforeCommit {
            assert!(error.to_string().contains("FOREIGN KEY constraint failed"));
            assert!(transaction.is_autocommit());
        }
        drop(transaction);
        drop(writer);
        assert_eq!(committed_version(&root), if committed { 2 } else { 1 });
        let read = SqliteDatabase::read(&root).unwrap();
        for (id, prior) in ids.iter().zip(&history) {
            let mut expected = prior.clone();
            if committed {
                expected["withdrawn_at"] = serde_json::json!("2026-09-28T13:00:00Z");
                expected["withdrawn_by"] = serde_json::json!("reviewer");
            }
            assert_eq!(
                serde_json::to_value(knowledge::knowledge(read.connection(), *id).unwrap())
                    .unwrap(),
                expected,
                "{phase:?}: {id}",
            );
        }
        for (table, expected) in [
            ("documents", 1),
            ("document_revisions", 1),
            ("passages", 1),
            ("passage_fts", 1),
            ("passage_vectors", 1),
            ("knowledge_item_evidence", 3),
        ] {
            assert_eq!(
                read.connection()
                    .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                expected,
                "{phase:?}: {table}"
            );
        }
        drop(read);
        if phase == PublishPhase::BeforeRestore {
            assert!(error.to_string().contains("restoration failed"));
            assert!(GraphRuntime::open(&root).is_err());
        } else {
            GraphRuntime::open(&root).unwrap();
            if !committed {
                assert_eq!(bytes(&graph.join("current")), before);
            }
        }
        rebuild(&root, Duration::ZERO).unwrap();
        let reopened = GraphRuntime::open(&root).unwrap();
        let active = reopened
            .query(
                "SELECT ?k WHERE {?k <urn:commonplace:property:kind> ?kind}",
                QueryConfig::default(),
            )
            .unwrap();
        assert_eq!(active.rows.len(), if committed { 0 } else { 3 });
        assert_eq!(committed_version(&root), if committed { 2 } else { 1 });
    }
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
        let error = publish_with(
            &root,
            &transaction,
            Duration::ZERO,
            "receipt",
            crate::app::record::RECOVERY_GUIDANCE,
            |at, tx| {
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
            },
        )
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
        crate::app::record::RECOVERY_GUIDANCE,
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
            crate::app::record::RECOVERY_GUIDANCE,
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
    publish_with(
        &root,
        &transaction,
        Duration::ZERO,
        "receipt",
        crate::app::record::RECOVERY_GUIDANCE,
        |at, _| {
            if format!("{at:?}") == phase {
                std::process::exit(77);
            }
            Ok(())
        },
    )
    .unwrap();
    panic!("expected exit phase");
}
