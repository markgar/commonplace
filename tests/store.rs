mod common;

use std::time::{Duration, Instant};

use common::Store;
use commonplace::storage::database::SqliteDatabase;
use serde_json::json;

#[test]
fn incompatible_or_incomplete_stores_fail_before_mutation() {
    for damage in [
        "config", "format", "versions", "fts", "vector", "database", "graph",
    ] {
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
            "graph" => std::fs::remove_file(store.root.join("graph/current.grafeo")).unwrap(),
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
