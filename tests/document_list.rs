mod common;

use common::Store;
use serde_json::json;
use std::fs;

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

    let default_page = store.success(&["document", "list"]);
    assert_eq!(default_page["result"]["truncated"], false);
    assert_eq!(
        default_page["result"]["items"].as_array().unwrap().len(),
        12
    );
    let thousand = store.success(&["document", "list", "--limit", "1000"]);
    assert_eq!(thousand["result"]["items"].as_array().unwrap().len(), 12);
    let exact_full = store.success(&["document", "list", "--limit", "12"]);
    assert_eq!(exact_full["result"]["truncated"], false);
    assert_eq!(exact_full["result"]["next_after"], serde_json::Value::Null);

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
            let db = store.database();
            let (source_key, created_at, last_ingested_at, revision_id, title, source_type, temporal_state, occurred_at, revision_created_at) = db.query_row(
                "SELECT d.source_key,d.created_at,d.last_ingested_at,r.revision_id,r.title,r.source_type,r.temporal_state,r.occurred_at,r.created_at
                 FROM documents d JOIN document_revisions r USING(document_id)
                 WHERE d.document_id=?1
                   AND r.revision_number=(SELECT max(revision_number) FROM document_revisions WHERE document_id=?1)",
                [document_id],
                |row| Ok((
                    row.get::<_,String>(0)?, row.get::<_,String>(1)?, row.get::<_,String>(2)?,
                    row.get::<_,i64>(3)?, row.get::<_,Option<String>>(4)?, row.get::<_,String>(5)?,
                    row.get::<_,String>(6)?, row.get::<_,Option<String>>(7)?, row.get::<_,String>(8)?,
                )),
            ).unwrap();
            assert_eq!(item["source_key"], source_key);
            assert_eq!(item["created_at"], created_at);
            assert_eq!(item["last_ingested_at"], last_ingested_at);
            assert_eq!(
                item["current_revision_id"],
                format!("revision:{revision_id}")
            );
            assert_eq!(
                item["title"],
                title
                    .as_deref()
                    .map_or(serde_json::Value::Null, |value| json!(value))
            );
            assert_eq!(item["source_type"], source_type);
            assert_eq!(item["temporal_state"], temporal_state);
            assert_eq!(
                item["occurred_at"],
                occurred_at
                    .as_deref()
                    .map_or(serde_json::Value::Null, |value| json!(value))
            );
            assert_eq!(item["current_revision_created_at"], revision_created_at);
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

    let before = store.files();
    let _read = store.success(&["document", "list", "--limit", "3"]);
    store.assert_files(&before);
    assert!(!store.root.join("writer.lock").exists());

    fs::remove_dir_all(store.root.join("graph/current")).unwrap();
    assert!(
        store.success(&["document", "list", "--limit", "3"])["result"]["items"]
            .as_array()
            .is_some()
    );
    store.failure(
        &["graph", "query", "SELECT ?document WHERE {?document ?p ?o}"],
        "graph_unavailable",
        1,
    );

    let deleted = Store::new();
    let db = deleted.database();
    db.execute_batch(
        "INSERT INTO documents(source_key,created_at,last_ingested_at)
         VALUES ('deleted-source','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
         INSERT INTO document_revisions(
             document_id,revision_number,revision_digest,text,title,source_type,
             temporal_state,occurred_at,metadata_json,created_at
         ) VALUES (
             1,1,'0000000000000000000000000000000000000000000000000000000000000000',
             'deleted','Deleted','note','unknown',NULL,'{}','2026-01-01T00:00:00Z'
         );",
    )
    .unwrap();
    db.execute("DELETE FROM documents WHERE document_id=1", [])
        .unwrap();
    drop(db);
    let deleted_cursor = deleted.success(&["document", "list", "--after", "doc:1"]);
    assert_eq!(deleted_cursor["result"]["items"], json!([]));
    assert_eq!(deleted_cursor["result"]["truncated"], false);
    assert_eq!(
        deleted_cursor["result"]["next_after"],
        serde_json::Value::Null
    );

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

    let invalid_temporal = Store::new();
    populate(&invalid_temporal, 1);
    let db = invalid_temporal.database();
    db.execute_batch("PRAGMA ignore_check_constraints = ON")
        .unwrap();
    db.execute(
        "UPDATE document_revisions
         SET temporal_state='bogus', occurred_at=NULL WHERE document_id=1",
        [],
    )
    .unwrap();
    drop(db);
    invalid_temporal.failure(&["document", "list"], "internal_error", 1);

    let missing_revision = Store::new();
    populate(&missing_revision, 1);
    let db = missing_revision.database();
    db.execute("DELETE FROM document_revisions WHERE document_id=1", [])
        .unwrap();
    drop(db);
    missing_revision.failure(&["document", "list"], "internal_error", 1);

    let invalid_id = Store::new();
    let db = invalid_id.database();
    db.execute_batch(
        "INSERT INTO documents(document_id,source_key,created_at,last_ingested_at)
         VALUES (0,'invalid-zero','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
         INSERT INTO document_revisions(
             document_id,revision_number,revision_digest,text,title,source_type,
             temporal_state,occurred_at,metadata_json,created_at
         ) VALUES (
             0,1,'0000000000000000000000000000000000000000000000000000000000000000',
             'invalid','Invalid','note','unknown',NULL,'{}','2026-01-01T00:00:00Z'
         );",
    )
    .unwrap();
    drop(db);
    invalid_id.failure(&["document", "list"], "internal_error", 1);
}

#[test]
fn listing_remains_independent_of_current_and_historical_graph_citations() {
    let store = Store::new();
    populate(&store, 12);
    store.apply(&Store::vocabulary(), false);

    let mut db = store.database();
    let transaction = db.transaction().unwrap();
    let historical_revision: i64 = transaction
        .query_row(
            "SELECT revision_id FROM document_revisions
             WHERE document_id=1 AND revision_number=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let current_revision: i64 = transaction
        .query_row(
            "SELECT revision_id FROM document_revisions
             WHERE document_id=1 AND revision_number=2",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let historical_passage: i64 = transaction
        .query_row(
            "SELECT passage_id FROM passages
             WHERE revision_id=?1",
            [current_revision],
            |row| row.get(0),
        )
        .unwrap();
    transaction
        .execute(
            "INSERT INTO passages(revision_id,ordinal,start_byte,end_byte,text)
             VALUES (?1,0,0,10,'alpha beta')",
            [historical_revision],
        )
        .unwrap();
    let historical_passage_id = transaction.last_insert_rowid();

    transaction
        .execute(
            "INSERT INTO entities(canonical_name,created_at,created_by)
             VALUES ('Ada','2026-11-01T10:00:00Z','test')",
            [],
        )
        .unwrap();
    let entity_id = transaction.last_insert_rowid();
    for knowledge_id in [1, 2] {
        transaction
            .execute(
                "INSERT INTO knowledge_items(
                    knowledge_item_id,kind,schema_version,created_at,created_by,
                    withdrawn_at,withdrawn_by
                 ) VALUES (?1,'type_membership',1,'2026-11-01T10:00:00Z','test',NULL,NULL)",
                [knowledge_id],
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO entity_type_memberships(
                    knowledge_item_id,entity_id,entity_type_id
                 ) VALUES (?1,?2,1)",
                (knowledge_id, entity_id),
            )
            .unwrap();
    }
    transaction
        .execute(
            "INSERT INTO knowledge_item_evidence(knowledge_item_id,passage_id)
             VALUES (1,?1),(2,?2)",
            (historical_passage, historical_passage_id),
        )
        .unwrap();
    transaction
        .execute(
            "UPDATE store_state SET knowledge_version=2 WHERE singleton=1",
            [],
        )
        .unwrap();
    transaction.commit().unwrap();
    drop(db);

    store.success(&["graph", "rebuild"]);
    let cited = store.success(&[
        "graph",
        "query",
        "PREFIX c:<urn:commonplace:property:> SELECT DISTINCT ?document WHERE {?knowledge c:evidence ?passage . ?passage c:revision ?revision . ?revision c:document ?document} ORDER BY ?document",
    ]);
    let rows = cited["result"]["rows"].as_array().unwrap();
    assert!(
        rows.iter()
            .any(|row| row[0]["value"] == "urn:commonplace:doc:1")
    );
    assert!(
        !rows
            .iter()
            .any(|row| row[0]["value"] == "urn:commonplace:doc:12")
    );

    let uncited = store.success(&[
        "graph",
        "query",
        "PREFIX c:<urn:commonplace:property:> SELECT DISTINCT ?knowledge WHERE {<urn:commonplace:doc:12> c:id ?id . ?knowledge c:evidence ?passage . ?passage c:revision ?revision . ?revision c:document <urn:commonplace:doc:12>} ",
    ]);
    assert_eq!(uncited["result"]["rows"], json!([]));

    let listing = store.success(&["document", "list", "--limit", "1000"]);
    let items = listing["result"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 12);
    assert!(items.iter().any(|item| item["document_id"] == "doc:1"));
    assert!(items.iter().any(|item| item["document_id"] == "doc:12"));
    let current = items
        .iter()
        .find(|item| item["document_id"] == "doc:1")
        .unwrap();
    assert_eq!(
        current["current_revision_id"],
        format!("revision:{current_revision}")
    );
}
