mod common;

use commonplace::Result;
use commonplace::app::{ingest, search};
use commonplace::domain::documents::{DocumentInput, TemporalState};
use commonplace::domain::search::SearchRequest;
use commonplace::providers::embeddings::{
    EMBEDDING_DIMENSIONS, EMBEDDING_IDENTITY, EmbeddingModel,
};
use commonplace::providers::reranker::Reranker;
use commonplace::storage::{database::SqliteDatabase, evidence, search as queries};
use serde_json::{Value, json};

use common::Store;

struct Model;

impl EmbeddingModel for Model {
    fn identity(&self) -> &'static str {
        EMBEDDING_IDENTITY
    }

    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|_| vector()).collect())
    }
}

impl Reranker for Model {
    fn rerank(&mut self, _: &str, texts: &[&str]) -> Result<Vec<f32>> {
        Ok(vec![0.0; texts.len()])
    }
}

fn vector() -> Vec<f32> {
    let mut vector = vec![0.0; EMBEDDING_DIMENSIONS];
    vector[0] = 1.0;
    vector
}

fn document(
    key: &str,
    text: &str,
    state: TemporalState,
    time: Option<&str>,
    kind: &str,
) -> DocumentInput {
    DocumentInput {
        source_key: key.into(),
        text: text.into(),
        title: None,
        source_type: kind.into(),
        temporal_state: state,
        occurred_at: time.map(str::to_owned),
        metadata: serde_json::Map::new(),
    }
}

fn publish(store: &Store, documents: Vec<DocumentInput>) -> ingest::IngestResult {
    ingest::ingest(
        &store.root,
        documents.into_iter().map(|document| ingest::InputItem {
            input: document.source_key.clone(),
            source_key: Some(document.source_key.clone()),
            document: Ok(document),
        }),
        &mut Model,
        &ingest::OperationConfig::default(),
    )
    .unwrap()
}

fn request(since: Option<&str>, types: &[&str], limit: usize) -> SearchRequest {
    SearchRequest {
        query: "needle".into(),
        since: since.map(str::to_owned),
        source_types: types.iter().map(|value| (*value).into()).collect(),
        limit,
    }
}

fn search(store: &Store, request: &SearchRequest) -> Value {
    serde_json::to_value(search::search(&store.root, request, &mut Model, &mut Model).unwrap())
        .unwrap()
}

#[test]
fn explicit_cli_states_are_required_in_all_modes_and_descriptions() {
    let store = Store::new();
    let file = store.directory.path().join("empty.md");
    std::fs::write(&file, "").unwrap();
    let before = store.files();
    for args in [
        vec!["ingest", file.to_str().unwrap()],
        vec!["ingest", "--stdin", "--source-key", "key"],
        vec![
            "ingest",
            file.to_str().unwrap(),
            "--occurred-at",
            "2026-01-01T00:00:00Z",
        ],
        vec![
            "ingest",
            file.to_str().unwrap(),
            "--temporal-state",
            "dated",
        ],
        vec![
            "ingest",
            file.to_str().unwrap(),
            "--temporal-state",
            "dated",
            "--occurred-at",
            "yesterday",
        ],
        vec![
            "ingest",
            file.to_str().unwrap(),
            "--temporal-state",
            "timeless",
            "--occurred-at",
            "2026-01-01T00:00:00Z",
        ],
        vec![
            "ingest",
            "--stdin",
            "--source-key",
            "key",
            "--temporal-state",
            "unknown",
            "--occurred-at",
            "2026-01-01T00:00:00Z",
        ],
    ] {
        let error = store.failure(&args, "invalid_input", 2);
        assert!(!error["error"]["message"].as_str().unwrap().is_empty());
        store.assert_files(&before);
    }
    let date = "2026-01-01T02:00:00.000000001+02:00";
    for state in ["dated", "timeless", "unknown"] {
        let mut args = vec!["ingest", file.to_str().unwrap(), "--temporal-state", state];
        if state == "dated" {
            args.extend(["--occurred-at", date]);
        }
        let output = store.success(&args);
        let revision = store.success(&[
            "get",
            output["result"]["items"][0]["revision_id"]
                .as_str()
                .unwrap(),
        ]);
        assert_eq!(revision["result"]["temporal_state"], state);
        assert_eq!(
            revision["result"]["occurred_at"],
            if state == "dated" {
                json!("2026-01-01T00:00:00.000000001Z")
            } else {
                Value::Null
            }
        );
        let mut scan_args = vec![
            "ingest",
            store.directory.path().to_str().unwrap(),
            "--temporal-state",
            state,
        ];
        if state == "dated" {
            scan_args.extend(["--occurred-at", date]);
        }
        let scan = store.success(&scan_args);
        assert_eq!(scan["result"]["summary"]["unchanged"], 1);

        let key = format!("stdin-{state}");
        let mut args = vec![
            "ingest",
            "--stdin",
            "--source-key",
            &key,
            "--temporal-state",
            state,
        ];
        if state == "dated" {
            args.extend(["--occurred-at", date]);
        }
        let mut command = store.command();
        command.args(&args);
        let output = common::with_stdin(command, b"");
        assert!(output.status.success(), "{output:?}");
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        let revision = store.success(&[
            "get",
            result["result"]["items"][0]["revision_id"]
                .as_str()
                .unwrap(),
        ]);
        assert_eq!(revision["result"]["temporal_state"], state);
    }
    let scanned = store.success(&[
        "ingest",
        store.directory.path().to_str().unwrap(),
        "--temporal-state",
        "unknown",
    ]);
    assert_eq!(scanned["result"]["summary"]["unchanged"], 1);
    let description = store.success(&["ingest", "--describe"]);
    let mut options = description["result"]["example"].clone();
    options.as_object_mut().unwrap().remove("temporal_state");
    assert!(!jsonschema::is_valid(
        &description["result"]["input_schema"],
        &options
    ));
    let schema = &description["result"]["record_schema"];
    for example in description["result"]["record_examples"].as_array().unwrap() {
        assert!(jsonschema::is_valid(schema, example));
    }
    for record in [
        json!({"source_key":"x","text":""}),
        json!({"source_key":"x","text":"","temporal_state":null}),
        json!({"source_key":"x","text":"","temporal_state":"dated"}),
        json!({"source_key":"x","text":"","temporal_state":"timeless","occurred_at":null}),
        json!({"source_key":"x","text":"","temporal_state":"unknown","occurred_at":"2026-01-01T00:00:00Z"}),
    ] {
        assert!(!jsonschema::is_valid(schema, &record), "{record}");
    }
    let help = store.run(&["ingest", "--help"]);
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(
        text.contains("--temporal-state") && text.contains("timeless") && text.contains("unknown")
    );
    let help = store.run(&["search", "--help"]);
    assert!(String::from_utf8(help.stdout).unwrap().contains("timeless"));
}

#[test]
fn jsonl_temporal_failures_never_publish_and_valid_neighbors_survive() {
    let store = Store::new();
    let records = [
        json!({"source_key":"dated","text":"","temporal_state":"dated","occurred_at":"2026-01-01T01:00:00+01:00"}),
        json!({"source_key":"omitted","text":""}),
        json!({"source_key":"null","text":"","temporal_state":null}),
        json!({"source_key":"bad-state","text":"","temporal_state":"recent"}),
        json!({"source_key":"no-date","text":"","temporal_state":"dated"}),
        json!({"source_key":"null-date","text":"","temporal_state":"dated","occurred_at":null}),
        json!({"source_key":"invalid-date","text":"","temporal_state":"dated","occurred_at":"yesterday"}),
        json!({"source_key":"timeless-date","text":"","temporal_state":"timeless","occurred_at":"2026-01-01T00:00:00Z"}),
        json!({"source_key":"unknown-null","text":"","temporal_state":"unknown","occurred_at":null}),
        json!({"source_key":"timeless","text":"","temporal_state":"timeless"}),
        json!({"source_key":"unknown","text":"","temporal_state":"unknown"}),
    ];
    let text = records
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    for disk in [false, true] {
        let path = store.directory.path().join("temporal.jsonl");
        std::fs::write(&path, &text).unwrap();
        let output = if disk {
            store.run(&["ingest", "--jsonl", path.to_str().unwrap()])
        } else {
            let mut command = store.command();
            command.args(["ingest", "--jsonl", "-"]);
            common::with_stdin(command, text.as_bytes())
        };
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["status"], "partial");
        assert_eq!(result["result"]["summary"]["failed"], 8);
        for item in &result["result"]["items"].as_array().unwrap()[1..9] {
            assert_eq!(item["status"], "failed");
            assert_eq!(item["document_id"], Value::Null);
            assert_eq!(item["revision_id"], Value::Null);
            assert_eq!(item["passage_ids"], json!([]));
            assert_eq!(item["error"]["code"], "invalid_input");
            assert_eq!(item["error"]["stage"], "input");
        }
    }
    assert_eq!(
        store
            .database()
            .query_row("SELECT count(*) FROM documents", [], |row| row
                .get::<_, usize>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        store
            .database()
            .query_row("SELECT count(*) FROM document_revisions", [], |row| row
                .get::<_, usize>(
                0
            ))
            .unwrap(),
        3
    );
    for args in [
        vec!["ingest", "--jsonl", "-", "--temporal-state", "unknown"],
        vec![
            "ingest",
            "--jsonl",
            "-",
            "--occurred-at",
            "2026-01-01T00:00:00Z",
        ],
    ] {
        let output = store.run(&args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn since_filters_both_paths_before_limits_and_counts_current_sources_not_matches() {
    use TemporalState::{Dated, Timeless, Unknown};
    let store = Store::new();
    let mut documents = Vec::new();
    for index in 0..70 {
        documents.push(document(
            &format!("old-{index}"),
            "needle",
            Dated,
            Some("2026-01-01T00:00:00Z"),
            "note",
        ));
        documents.push(document(
            &format!("unknown-{index}"),
            "needle",
            Unknown,
            None,
            "note",
        ));
        documents.push(document(
            &format!("other-{index}"),
            "needle",
            Timeless,
            None,
            "other",
        ));
    }
    documents.extend([
        document(
            "whole",
            "needle",
            Dated,
            Some("2026-09-21T00:00:00Z"),
            "note",
        ),
        document(
            "nano",
            "needle",
            Dated,
            Some("2026-09-20T19:00:00.000000001-05:00"),
            "note",
        ),
        document(
            "fraction",
            "needle",
            Dated,
            Some("2026-09-21T00:00:00.1000Z"),
            "recap",
        ),
        document("background", "needle", Timeless, None, "note"),
        document("empty", "", Timeless, None, "note"),
        document("revised", "needle", Unknown, None, "note"),
        document("many", &"needle\n\n".repeat(400), Timeless, None, "note"),
    ]);
    let added = publish(&store, documents);
    assert_eq!(added.summary.failed, 0);
    let updated = publish(
        &store,
        vec![document(
            "revised",
            "needle",
            Dated,
            Some("2026-09-21T00:00:00.2Z"),
            "other",
        )],
    );
    assert_eq!(updated.summary.updated, 1);
    let selected = request(
        Some("2026-09-20T19:00:00.000000001-05:00"),
        &["note", "recap", "note"],
        50,
    );
    let result = search(&store, &selected);
    assert_eq!(
        result["temporal_filter"]["since"],
        "2026-09-21T00:00:00.000000001Z"
    );
    assert_eq!(result["temporal_filter"]["includes_timeless"], true);
    assert_eq!(
        result["temporal_filter"]["coverage"],
        json!({"eligible_dated":2,"timeless":3,"older_dated":71,"excluded_unknown":70})
    );
    assert_eq!(
        result["temporal_filter"]["diagnostics"][0]["code"],
        "unknown_dates_excluded"
    );
    let session = SqliteDatabase::read(&store.root).unwrap();
    for since in [
        "2026-09-21T00:00:00Z",
        "2026-09-21T00:00:00.000000001Z",
        "2026-09-21T00:00:00.100000000Z",
        "2026-09-21T00:00:00.100000001Z",
    ] {
        let request = request(Some(since), &["note", "recap"], 50);
        let filters = request.validate().unwrap();
        let expected_keys = match since {
            "2026-09-21T00:00:00Z" => vec!["whole", "nano", "fraction", "background", "many"],
            "2026-09-21T00:00:00.000000001Z" => vec!["nano", "fraction", "background", "many"],
            "2026-09-21T00:00:00.100000000Z" => vec!["fraction", "background", "many"],
            _ => vec!["background", "many"],
        };
        for candidates in [
            queries::lexical(session.connection(), &request.lexical_query(), &filters).unwrap(),
            queries::vector(session.connection(), &vector(), &filters).unwrap(),
        ] {
            assert!(!candidates.truncated);
            let mut keys = candidates
                .ids
                .into_iter()
                .map(|id| {
                    evidence::passage(session.connection(), id)
                        .unwrap()
                        .source_key
                })
                .collect::<Vec<_>>();
            keys.sort();
            keys.dedup();
            let mut expected = expected_keys.clone();
            expected.sort();
            assert_eq!(keys, expected);
        }
    }
    for limit in [0, 1, 50] {
        let result = search(
            &store,
            &request(selected.since.as_deref(), &["note", "recap"], limit),
        );
        assert_eq!(
            result["temporal_filter"]["coverage"],
            json!({"eligible_dated":2,"timeless":3,"older_dated":71,"excluded_unknown":70})
        );
        assert_eq!(
            result["temporal_filter"]["diagnostics"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        if limit == 0 {
            assert_eq!(result["items"], json!([]));
            assert_eq!(result["truncated"], true);
        }
    }
    let none = search(&store, &request(selected.since.as_deref(), &["absent"], 50));
    assert_eq!(none["items"], json!([]));
    assert_eq!(none["truncated"], false);
    assert_eq!(
        none["temporal_filter"]["coverage"],
        json!({"eligible_dated":0,"timeless":0,"older_dated":0,"excluded_unknown":0})
    );
    assert_eq!(none["temporal_filter"]["diagnostics"], json!([]));
    assert!(search(&store, &request(None, &[], 50))["temporal_filter"].is_null());
    let unknown_only = Store::new();
    publish(
        &unknown_only,
        vec![document("event", "needle", Unknown, None, "event")],
    );
    let uncovered = search(
        &unknown_only,
        &request(Some("2026-09-21T00:00:00Z"), &[], 50),
    );
    assert_eq!(uncovered["items"], json!([]));
    assert_eq!(uncovered["truncated"], false);
    assert_eq!(
        uncovered["temporal_filter"]["coverage"]["excluded_unknown"],
        1
    );
    assert_eq!(
        uncovered["temporal_filter"]["diagnostics"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn temporal_only_revisions_preserve_identity_idempotence_and_cited_historical_state() {
    use TemporalState::{Dated, Timeless, Unknown};
    let store = Store::new();
    let text = "needle\r\n\0e\u{301}🦀";
    let original = document(
        "stable",
        text,
        Dated,
        Some("2026-01-01T02:00:00+02:00"),
        "note",
    );
    let added = publish(&store, vec![original.clone()]);
    let passage = added.items[0].passage_ids[0];
    let revision = added.items[0].revision_id.unwrap();
    let before = store.success(&["get", &passage.to_string()]);
    let mut equivalent = original.clone();
    equivalent.occurred_at = Some("2026-01-01T00:00:00.000Z".into());
    let unchanged = publish(&store, vec![equivalent]);
    assert_eq!(unchanged.summary.unchanged, 1);
    assert_eq!(unchanged.items[0].revision_id, Some(revision));
    store.apply(&json!({"entity_types":[{"name":"person"}]}), false);
    let path = store.input(&json!({"items":[
        {"kind":"entity","ref":"person","name":"Ada"},
        {"kind":"type_membership","entity":{"ref":"person"},"entity_type":"person","support":[{"passage_id":passage}]}
    ]}));
    let record = store.success(&["record", path.to_str().unwrap()]);
    let knowledge = record["result"]["items"][1]["knowledge_id"]
        .as_str()
        .unwrap();
    for state in [Timeless, Unknown] {
        let changed = publish(&store, vec![document("stable", text, state, None, "note")]);
        assert_eq!(changed.summary.updated, 1);
        assert_eq!(changed.items[0].document_id, added.items[0].document_id);
        assert_ne!(changed.items[0].revision_id, Some(revision));
        assert_eq!(store.success(&["get", &passage.to_string()]), before);
        assert_eq!(
            store.success(&["get", knowledge])["result"]["support"][0]["temporal_state"],
            "dated"
        );
        let current = store.success(&["get", &changed.items[0].revision_id.unwrap().to_string()]);
        assert_eq!(current["result"]["temporal_state"], state.as_str());
        assert_ne!(
            current["result"]["revision_digest"],
            store.success(&["get", &revision.to_string()])["result"]["revision_digest"]
        );
        let repeated = publish(&store, vec![document("stable", text, state, None, "note")]);
        assert_eq!(repeated.summary.unchanged, 1);
        assert_eq!(repeated.items[0].revision_id, changed.items[0].revision_id);
    }

    let query = "PREFIX c: <urn:commonplace:property:> SELECT ?state ?time WHERE {?k c:evidence ?p . ?p c:revision ?r . ?r c:temporal_state ?state; c:occurred_at ?time}";
    for rebuild in [false, true] {
        if rebuild {
            store.success(&["graph", "rebuild"]);
        }
        let result = store.success(&["graph", "query", query]);
        assert_eq!(result["result"]["rows"].as_array().unwrap().len(), 1);
        assert_eq!(result["result"]["rows"][0][0]["value"], "dated");
        assert_eq!(
            result["result"]["rows"][0][1]["value"],
            "2026-01-01T00:00:00Z"
        );
    }
}

#[test]
fn sqlite_temporal_constraints_reject_inconsistent_states() {
    use TemporalState::{Dated, Timeless, Unknown};
    let store = Store::new();
    let invalid = publish(
        &store,
        vec![
            document("missing", "", Dated, None, "note"),
            document(
                "timeless-time",
                "",
                Timeless,
                Some("2026-01-01T00:00:00Z"),
                "note",
            ),
            document(
                "unknown-time",
                "",
                Unknown,
                Some("2026-01-01T00:00:00Z"),
                "note",
            ),
            document("bad-time", "", Dated, Some("yesterday"), "note"),
        ],
    );
    assert_eq!(invalid.summary.failed, 4);
    assert_eq!(
        store
            .database()
            .query_row("SELECT count(*) FROM documents", [], |row| row
                .get::<_, usize>(0))
            .unwrap(),
        0
    );
    let valid = publish(&store, vec![document("valid", "", Unknown, None, "note")]);
    let revision = valid.items[0].revision_id.unwrap().value();
    let connection = store.database();
    for sql in [
        "UPDATE document_revisions SET temporal_state=NULL WHERE revision_id=?1",
        "UPDATE document_revisions SET temporal_state='recent' WHERE revision_id=?1",
        "UPDATE document_revisions SET temporal_state='dated' WHERE revision_id=?1",
        "UPDATE document_revisions SET occurred_at='2026-01-01T00:00:00Z' WHERE revision_id=?1",
    ] {
        assert!(connection.execute(sql, [revision]).is_err(), "{sql}");
    }
    assert_eq!(
        store.success(&["get", &format!("revision:{revision}")])["result"]["temporal_state"],
        "unknown"
    );
}

#[test]
fn old_stores_fail_closed_without_format_conversion_or_application_lock_creation() {
    for populated in [false, true] {
        for config_version in [2, 3] {
            let store = Store::new();
            let db = store.database();
            db.execute_batch(
                    "DROP TABLE document_revisions;
                     CREATE TABLE document_revisions (
                        revision_id INTEGER PRIMARY KEY AUTOINCREMENT,
                        document_id INTEGER NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
                        revision_number INTEGER NOT NULL CHECK (revision_number > 0),
                        revision_digest TEXT NOT NULL CHECK (length(revision_digest) = 64),
                        text TEXT NOT NULL, title TEXT,
                        source_type TEXT NOT NULL CHECK (length(source_type) > 0),
                        occurred_at TEXT,
                        metadata_json TEXT NOT NULL CHECK (json_valid(metadata_json) AND json_type(metadata_json) = 'object'),
                        created_at TEXT NOT NULL, UNIQUE (document_id, revision_number)
                     ) STRICT;
                     DROP TABLE store_state;
                     CREATE TABLE store_state (
                        singleton INTEGER PRIMARY KEY CHECK (singleton=1),
                        format TEXT NOT NULL CHECK (format = 'commonplace-store/2'),
                        schema_version INTEGER NOT NULL DEFAULT 0 CHECK (schema_version>=0),
                        knowledge_version INTEGER NOT NULL DEFAULT 0 CHECK (knowledge_version>=0),
                        created_at TEXT NOT NULL
                     ) STRICT;
                     INSERT INTO store_state VALUES(1,'commonplace-store/2',0,0,'2026-01-01T00:00:00Z');"
                ).unwrap();
            if populated {
                db.execute_batch(
                        "INSERT INTO documents VALUES(1,'legacy-null','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
                         INSERT INTO document_revisions VALUES(1,1,1,printf('%064d',0),'old text',NULL,'note',NULL,'{}','2026-01-01T00:00:00Z');
                         INSERT INTO documents VALUES(2,'legacy-dated','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
                         INSERT INTO document_revisions VALUES(2,2,1,printf('%064d',0),'dated text',NULL,'note','2026-01-01T00:00:00Z','{}','2026-01-01T00:00:00Z');"
                    ).unwrap();
            }
            drop(db);
            std::fs::write(
                store.root.join("config.json"),
                serde_json::to_vec(&json!({
                    "format":format!("commonplace-config/{config_version}"),
                    "database":"commonplace.sqlite3","graph":"graph/current"
                }))
                .unwrap(),
            )
            .unwrap();
            let input = store.input(&json!({"items":[]}));
            let file = store.directory.path().join("empty.md");
            std::fs::write(&file, "").unwrap();
            let before = store.files();
            for args in [
                vec!["init"],
                vec!["get", "doc:1"],
                vec!["schema", "show"],
                vec!["schema", "freeze"],
                vec![
                    "ingest",
                    file.to_str().unwrap(),
                    "--temporal-state",
                    "unknown",
                ],
                vec!["search", "needle", "--since", "2026-01-01T00:00:00Z"],
                vec!["record", input.to_str().unwrap()],
                vec!["graph", "query", "SELECT ?s WHERE {?s ?p ?o}"],
                vec!["graph", "rebuild"],
            ] {
                let error = store.failure(&args, "conflict", 3);
                assert!(
                    error["error"]["message"]
                        .as_str()
                        .unwrap()
                        .contains("fresh")
                );
                store.assert_files(&before);
                assert!(!store.root.join("writer.lock").exists());
            }
            let read = store.database();
            assert_eq!(
                read.query_row("SELECT format FROM store_state", [], |row| row
                    .get::<_, String>(0))
                    .unwrap(),
                "commonplace-store/2"
            );
            assert_eq!(
                read.query_row("SELECT count(*) FROM documents", [], |row| row
                    .get::<_, usize>(0))
                    .unwrap(),
                if populated { 2 } else { 0 }
            );
        }
    }
}
