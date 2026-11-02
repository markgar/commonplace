mod common;

use common::Store;
use serde_json::json;

fn populate(store: &Store, count: i64) {
    let mut db = store.database();
    let transaction = db.transaction().unwrap();
    for document_id in 1..=count {
        let temporal = if document_id % 3 == 0 {
            ("timeless", None)
        } else if document_id % 3 == 1 {
            ("unknown", None)
        } else {
            ("dated", Some("2026-11-01T10:00:00Z"))
        };
        let text = if document_id == 2 { "" } else { "alpha beta" };
        transaction
            .execute(
                "INSERT INTO documents(source_key, created_at, last_ingested_at)
                 VALUES (?1, ?2, ?3)",
                (
                    format!("notes/{document_id}"),
                    "2026-11-01T10:00:00Z",
                    "2026-11-02T10:00:00Z",
                ),
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO document_revisions(
                    document_id, revision_number, revision_digest, text, title,
                    source_type, temporal_state, occurred_at, metadata_json, created_at
                 ) VALUES (?1, 1, '0000000000000000000000000000000000000000000000000000000000000000',
                    ?2, ?3, 'note', ?4, ?5, '{}', '2026-11-01T10:00:00Z')",
                (
                    document_id,
                    text,
                    format!("Title {document_id}"),
                    temporal.0,
                    temporal.1,
                ),
            )
            .unwrap();
        if document_id == 1 {
            transaction
                .execute(
                    "INSERT INTO document_revisions(
                        document_id, revision_number, revision_digest, text, title,
                        source_type, temporal_state, occurred_at, metadata_json, created_at
                     ) VALUES (1, 2,
                        '1111111111111111111111111111111111111111111111111111111111111111',
                        'current text', 'Current title', 'note', 'timeless', NULL, '{}',
                        '2026-11-02T10:00:00Z')",
                    [],
                )
                .unwrap();
            let revision_id: i64 = transaction
                .query_row(
                    "SELECT revision_id FROM document_revisions
                     WHERE document_id = 1 AND revision_number = 2",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            transaction
                .execute(
                    "INSERT INTO passages(revision_id, ordinal, start_byte, end_byte, text)
                     VALUES (?1, 0, 0, 12, 'current text')",
                    [revision_id],
                )
                .unwrap();
        }
    }
    transaction.commit().unwrap();
}

#[test]
fn exhaustive_listing_traverses_numeric_ids_and_current_revisions() {
    let store = Store::new();
    let empty = store.success(&["document", "list"]);
    assert_eq!(empty["result"]["items"], json!([]));
    assert_eq!(empty["result"]["truncated"], false);
    assert_eq!(empty["result"]["next_after"], serde_json::Value::Null);

    let help = store
        .command()
        .args(["document", "list", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    let help_text = String::from_utf8(help.stdout).unwrap();
    assert!(help_text.contains("--after"));
    assert!(help_text.contains("--limit"));

    populate(&store, 12);

    let mut seen = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let page = if let Some(cursor) = &after {
            store.success(&["document", "list", "--limit", "3", "--after", cursor])
        } else {
            store.success(&["document", "list", "--limit", "3"])
        };
        assert_eq!(page["operation"], "document.list");
        assert_eq!(page["contract_version"], "1");
        assert_eq!(page["status"], "complete");
        for item in page["result"]["items"].as_array().unwrap() {
            seen.push(item["document_id"].as_str().unwrap().to_owned());
            let keys = item
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(
                keys,
                [
                    "document_id",
                    "source_key",
                    "current_revision_id",
                    "title",
                    "source_type",
                    "temporal_state",
                    "occurred_at",
                    "created_at",
                    "last_ingested_at",
                    "current_revision_created_at",
                ]
                .into_iter()
                .collect()
            );
            let document_id = item["document_id"]
                .as_str()
                .unwrap()
                .strip_prefix("doc:")
                .unwrap()
                .parse::<i64>()
                .unwrap();
            assert_eq!(item["source_key"], format!("notes/{document_id}"));
        }
        if page["result"]["truncated"].as_bool().unwrap() {
            after = page["result"]["next_after"].as_str().map(str::to_owned);
        } else {
            assert!(page["result"]["next_after"].is_null());
            break;
        }
    }
    assert_eq!(
        seen,
        (1..=12).map(|id| format!("doc:{id}")).collect::<Vec<_>>()
    );
    assert_eq!(seen.iter().filter(|id| id.as_str() == "doc:9").count(), 1);
    assert_eq!(seen.iter().filter(|id| id.as_str() == "doc:10").count(), 1);

    let first = store.success(&["document", "list", "--limit", "1"]);
    assert_eq!(first["result"]["truncated"], true);
    assert_eq!(first["result"]["next_after"], "doc:1");
    assert_eq!(first["result"]["items"][0]["document_id"], "doc:1");
    assert_eq!(first["result"]["items"][0]["temporal_state"], "timeless");
    assert_eq!(first["result"]["items"][0]["title"], "Current title");
    let authority = store.success(&["get", "doc:1"]);
    assert_eq!(
        authority["result"]["current_revision_id"],
        first["result"]["items"][0]["current_revision_id"]
    );
    let current_revision = store.success(&[
        "get",
        first["result"]["items"][0]["current_revision_id"]
            .as_str()
            .unwrap(),
    ]);
    assert_eq!(current_revision["result"]["title"], "Current title");
    assert_eq!(current_revision["result"]["text"], "current text");
    assert_eq!(
        current_revision["result"]["passage_ids"],
        json!(["passage:1"])
    );

    let mut db = store.database();
    let transaction = db.transaction().unwrap();
    transaction
        .execute(
            "INSERT INTO documents(source_key, created_at, last_ingested_at)
             VALUES ('notes/13', '2026-11-03T10:00:00Z', '2026-11-03T10:00:00Z')",
            [],
        )
        .unwrap();
    transaction
        .execute(
            "INSERT INTO document_revisions(
                document_id, revision_number, revision_digest, text, title,
                source_type, temporal_state, occurred_at, metadata_json, created_at
             ) VALUES (13, 1,
                '2222222222222222222222222222222222222222222222222222222222222222',
                'new source', 'New source', 'note', 'unknown', NULL, '{}',
                '2026-11-03T10:00:00Z')",
            [],
        )
        .unwrap();
    transaction.commit().unwrap();
    drop(db);
    let changed = store.success(&["document", "list", "--after", "doc:12"]);
    assert_eq!(changed["result"]["items"][0]["document_id"], "doc:13");
    assert_eq!(changed["result"]["truncated"], false);
    assert_eq!(changed["result"]["next_after"], serde_json::Value::Null);
}

#[test]
fn listing_validates_cursors_limits_and_corrupt_state() {
    let store = Store::new();
    populate(&store, 12);

    assert!(
        store.success(&["document", "list", "--limit", "1000"])["result"]["items"]
            .as_array()
            .is_some()
    );
    for limit in ["0", "1001"] {
        store.failure(&["document", "list", "--limit", limit], "limit_exceeded", 2);
    }
    for cursor in ["revision:1", "entity:1", "doc:0", "doc:-1", "not-an-id"] {
        store.failure(&["document", "list", "--after", cursor], "invalid_input", 2);
    }
    let exhausted = store.success(&["document", "list", "--after", "doc:999"]);
    assert_eq!(exhausted["result"]["items"], json!([]));
    assert_eq!(exhausted["result"]["truncated"], false);
    assert_eq!(exhausted["result"]["next_after"], serde_json::Value::Null);

    let db = store.database();
    db.execute_batch("PRAGMA ignore_check_constraints = ON")
        .unwrap();
    db.execute(
        "UPDATE document_revisions
         SET temporal_state = 'unknown', occurred_at = '2026-11-01T10:00:00Z'
         WHERE document_id = 1",
        [],
    )
    .unwrap();
    drop(db);
    store.failure(&["document", "list"], "internal_error", 1);

    let db = store.database();
    db.execute_batch("PRAGMA ignore_check_constraints = OFF")
        .unwrap();
    db.execute("DELETE FROM document_revisions WHERE document_id = 2", [])
        .unwrap();
    drop(db);
    store.failure(&["document", "list"], "internal_error", 1);
}
