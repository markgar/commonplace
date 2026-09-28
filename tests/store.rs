mod common;

use std::fs;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use common::Store;
use commonplace::storage::database::SqliteDatabase;
use serde_json::json;

#[test]
fn incompatible_or_incomplete_stores_fail_before_mutation() {
    for damage in ["config", "format", "versions", "fts", "vector", "database"] {
        let store = Store::new();
        match damage {
            "config" => {
                std::fs::write(store.root.join("config.json"), br#"{"format":"future","database":"commonplace.sqlite3","graph":"graph/current.grafeo"}"#).unwrap();
            }
            "format" | "versions" => {
                let db = store.database();
                db.execute_batch("PRAGMA ignore_check_constraints = ON")
                    .unwrap();
                if damage == "format" {
                    db.execute("UPDATE store_state SET format = 'future'", [])
                        .unwrap();
                } else {
                    db.execute("UPDATE store_state SET schema_version = -1", [])
                        .unwrap();
                }
            }
            "fts" | "vector" => {
                // Open through the real adapter so sqlite-vec is registered for DROP.
                drop(SqliteDatabase::read(&store.root).unwrap());
                let db = store.database();
                let table = if damage == "fts" {
                    "passage_fts"
                } else {
                    "passage_vectors"
                };
                db.execute_batch(&format!(
                    "DROP TABLE {table}; CREATE TABLE {table} (fake TEXT)"
                ))
                .unwrap();
            }
            "database" => std::fs::remove_file(store.root.join("commonplace.sqlite3")).unwrap(),
            _ => unreachable!(),
        }
        let before = store.files();
        let path = store.input(&Store::vocabulary());
        for arguments in [
            vec!["schema", "apply", path.to_str().unwrap()],
            vec!["schema", "apply", path.to_str().unwrap(), "--check"],
            vec!["schema", "show"],
            vec!["init"],
        ] {
            store.failure(&arguments, "conflict", 3);
            store.assert_files(&before);
        }
        assert!(!store.root.join("writer.lock").exists());
    }
}

#[test]
fn absent_store_is_not_created() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("missing");
    for arguments in [
        vec!["schema", "show"],
        vec!["schema", "apply", "--describe"],
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_commonplace"))
            .arg("--store")
            .arg(&root)
            .args(&arguments)
            .output()
            .unwrap();
        assert_eq!(output.status.success(), arguments.contains(&"--describe"));
        assert!(!root.exists());
    }
}

#[test]
fn read_sessions_are_consistent_and_writers_use_configured_timeout() {
    let store = Store::new();
    let read = SqliteDatabase::read(&store.root).unwrap();
    let version = || {
        read.connection()
            .query_row("SELECT schema_version FROM store_state", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
    };
    assert_eq!(version(), 0);
    store.apply(&Store::vocabulary(), false);
    // This reader pins the old WAL snapshot; new CLI readers must see the commit.
    let checked = store.apply(&Store::vocabulary(), true);
    assert_eq!(checked["result"]["schema_version"], 1);
    assert_eq!(checked["result"]["changed"], false);
    assert_eq!(version(), 0);
    assert!(
        read.connection()
            .execute("UPDATE store_state SET schema_version = 3", [])
            .is_err()
    );
    drop(read);
    assert_eq!(
        store.success(&["schema", "show"])["result"]["schema_version"],
        1
    );
    let _writer = SqliteDatabase::write(&store.root, Duration::ZERO).unwrap();
    let start = Instant::now();
    let error = match SqliteDatabase::write(&store.root, Duration::from_millis(50)) {
        Ok(_) => panic!("contending writer succeeded"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "conflict");
    assert!(start.elapsed() >= Duration::from_millis(50));
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn concurrent_public_read_commands_share_wal_without_busy_conflicts() {
    const READERS_PER_WAVE: usize = 64;
    const WAVES: usize = 16;
    const COMMANDS: &[&[&str]] = &[
        &["get", "doc:1"],
        &["get", "revision:1"],
        &["get", "passage:1"],
        &["get", "entity:1"],
        &["get", "knowledge:1"],
        &["schema", "show"],
    ];

    let store = Store::new();
    store
        .database()
        .execute_batch(
            "BEGIN IMMEDIATE;
             UPDATE store_state
                SET schema_version = 1, knowledge_version = 1
              WHERE singleton = 1;
             INSERT INTO documents(source_key, created_at, last_ingested_at)
             VALUES ('synthetic', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
             INSERT INTO document_revisions(
                 document_id, revision_number, revision_digest, text, title,
                 source_type, occurred_at, metadata_json, created_at
             ) VALUES (
                 1, 1,
                 '0000000000000000000000000000000000000000000000000000000000000000',
                 'alpha beta gamma delta', 'Synthetic', 'test', NULL, '{}',
                 '2026-01-01T00:00:00Z'
             );
             INSERT INTO passages(revision_id, ordinal, start_byte, end_byte, text)
             VALUES (1, 0, 0, 5, 'alpha');
             INSERT INTO entity_types(name, introduced_version) VALUES ('person', 1);
             INSERT INTO entities(canonical_name, created_at, created_by)
             VALUES ('Synthetic Person', '2026-01-01T00:00:00Z', 'test');
             INSERT INTO knowledge_items(kind, schema_version, created_at, created_by)
             VALUES ('type_membership', 1, '2026-01-01T00:00:00Z', 'test');
             INSERT INTO entity_type_memberships(
                 knowledge_item_id, entity_id, entity_type_id
             ) VALUES (1, 1, 1);
             INSERT INTO knowledge_item_evidence(knowledge_item_id, passage_id)
             VALUES (1, 1);
             COMMIT;",
        )
        .unwrap();
    store.success(&["graph", "rebuild"]);
    let before = store.files();
    let database = store.root.join("commonplace.sqlite3");

    for wave in 0..WAVES {
        let connection = store.database();
        connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
        drop(connection);

        let wal = database.with_extension("sqlite3-wal");
        match fs::metadata(&wal) {
            Ok(metadata) => {
                assert_eq!(
                    metadata.len(),
                    0,
                    "wave {wave} retained committed WAL frames"
                );
                fs::remove_file(&wal).unwrap();
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("inspect WAL before wave {wave}: {error}"),
        }
        let shm = database.with_extension("sqlite3-shm");
        match fs::remove_file(&shm) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("remove disposable SHM before wave {wave}: {error}"),
        }

        let barrier = Arc::new(Barrier::new(READERS_PER_WAVE));
        let outputs = thread::scope(|scope| {
            let store = &store;
            let handles = (0..READERS_PER_WAVE)
                .map(|index| {
                    let barrier = Arc::clone(&barrier);
                    let arguments = COMMANDS[index % COMMANDS.len()];
                    scope.spawn(move || {
                        barrier.wait();
                        (arguments, store.run(arguments))
                    })
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<Vec<_>>()
        });

        for (arguments, output) in outputs {
            assert!(
                output.status.success(),
                "wave {wave}, command {arguments:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                output.stderr.is_empty(),
                "wave {wave}, command {arguments:?}"
            );
            let response: serde_json::Value =
                serde_json::from_slice(&output.stdout).expect("JSON result");
            if arguments[0] == "schema" {
                assert_eq!(response["operation"], "schema.show");
                assert_eq!(response["result"]["schema_version"], 1);
            } else {
                let id = arguments[1];
                let (kind, field) = match id.split_once(':').unwrap().0 {
                    "doc" => ("document", "document_id"),
                    "revision" => ("revision", "revision_id"),
                    "passage" => ("passage", "passage_id"),
                    "entity" => ("entity", "entity_id"),
                    "knowledge" => ("knowledge", "knowledge_id"),
                    tag => panic!("unexpected ID tag {tag}"),
                };
                assert_eq!(response["operation"], "get");
                assert_eq!(response["result"]["kind"], kind);
                assert_eq!(response["result"][field], id);
            }
        }
    }

    store.assert_files(&before);
}

#[test]
fn sqlite_constraints_and_rollback_are_active_in_write_sessions() {
    let store = Store::new();
    {
        let mut session = SqliteDatabase::write(&store.root, Duration::ZERO).unwrap();
        let transaction = session.transaction().unwrap();
        assert_eq!(
            transaction
                .pragma_query_value(None, "foreign_keys", |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert!(
            transaction
                .execute(
                    "INSERT INTO predicate_entity_types VALUES (99, 'subject', 99)",
                    []
                )
                .is_err()
        );
        assert!(
            transaction
                .execute(
                    "INSERT INTO entity_types (name, introduced_version) VALUES ('person', 0)",
                    []
                )
                .is_err()
        );
        assert!(transaction.execute("INSERT INTO predicates (name, object_kind, introduced_version) VALUES ('age', 'float', 1)", []).is_err());
        transaction
            .execute(
                "INSERT INTO entity_types (name, introduced_version) VALUES ('person', 1)",
                [],
            )
            .unwrap();
        assert!(
            transaction
                .execute(
                    "INSERT INTO entity_types (name, introduced_version) VALUES ('person', 1)",
                    []
                )
                .is_err()
        );
        // Dropping an uncommitted transaction rolls back its valid inserts too.
    }
    assert_eq!(
        store.success(&["schema", "show"])["result"]["entity_types"],
        json!([])
    );
}
