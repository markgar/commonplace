mod common;

use std::path::PathBuf;

use commonplace::app::{
    get,
    ingest::{self, InputItem, OperationConfig},
};
use commonplace::domain::documents::DocumentInput;
use commonplace::providers::embeddings::{
    EMBEDDING_DIMENSIONS, EMBEDDING_IDENTITY, EmbeddingModel,
};
use commonplace::storage::{database::SqliteDatabase, evidence};
use commonplace::{CommonplaceError, Result};
use serde_json::{Value, json};

use common::Store;
use commonplace::adapters::streams::JsonLines;

fn stream_run(store: &Store, args: &[&str], text: &[u8]) -> (i32, Value) {
    let mut command = store.command();
    command.args(args).env(
        "COMMONPLACE_MODEL_CACHE",
        store.directory.path().join("missing-cache"),
    );
    let output = common::with_stdin(command, text);
    assert!(output.stderr.is_empty(), "{output:?}");
    (
        output.status.code().unwrap(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}

#[test]
fn public_stream_modes_preserve_identity_defaults_and_file_behavior() {
    let store = Store::new();
    let graph = graph_files(&store);
    let (exit, first) = stream_run(
        &store,
        &[
            "ingest",
            "--temporal-state",
            "unknown",
            "--stdin",
            "--source-key",
            " notes/42 ",
        ],
        b"",
    );
    assert_eq!(exit, 0);
    assert_eq!(first["result"]["summary"]["added"], 1);
    let item = &first["result"]["items"][0];
    assert_eq!(item["input"], "stdin");
    assert_eq!(item["source_key"], " notes/42 ");
    let revision = store.success(&["get", item["revision_id"].as_str().unwrap()]);
    assert_eq!(revision["result"]["source_type"], "text");
    assert_eq!(revision["result"]["title"], Value::Null);
    assert_eq!(revision["result"]["occurred_at"], Value::Null);
    assert_eq!(revision["result"]["metadata"], json!({}));
    let record = br#"{"temporal_state":"unknown","source_key":" notes/42 ","text":""}"#;
    let (_, same) = stream_run(&store, &["ingest", "--jsonl", "-"], record);
    assert_eq!(same["result"]["summary"]["unchanged"], 1);
    assert_eq!(
        same["result"]["items"][0]["revision_id"],
        item["revision_id"]
    );
    let (_, updated) = stream_run(
        &store,
        &["ingest", "--jsonl", "-"],
        br#"{"temporal_state":"unknown","source_key":" notes/42 ","text":"","metadata":{"a":1}}"#,
    );
    assert_eq!(updated["result"]["summary"]["updated"], 1);
    assert_eq!(
        updated["result"]["items"][0]["document_id"],
        item["document_id"]
    );
    assert_ne!(
        updated["result"]["items"][0]["revision_id"],
        item["revision_id"]
    );
    let (_, new_key) = stream_run(
        &store,
        &[
            "ingest",
            "--temporal-state",
            "unknown",
            "--stdin",
            "--source-key",
            "notes/42",
        ],
        b"",
    );
    assert_ne!(
        new_key["result"]["items"][0]["document_id"],
        item["document_id"]
    );

    let file = store.directory.path().join("empty.md");
    std::fs::write(&file, "").unwrap();
    let file_result = store.success(&[
        "ingest",
        "--temporal-state",
        "unknown",
        file.to_str().unwrap(),
    ]);
    let file_item = &file_result["result"]["items"][0];
    let (_, same_file) = stream_run(
        &store,
        &[
            "ingest",
            "--temporal-state",
            "unknown",
            "--stdin",
            "--source-key",
            file_item["source_key"].as_str().unwrap(),
            "--source-type",
            "file",
            "--title",
            "empty.md",
        ],
        b"",
    );
    assert_eq!(same_file["result"]["summary"]["unchanged"], 1);
    assert_eq!(
        same_file["result"]["items"][0]["revision_id"],
        file_item["revision_id"]
    );
    let batch_path = store.directory.path().join("batch.jsonl");
    std::fs::write(&batch_path, record).unwrap();
    let disk = store.success(&["ingest", "--jsonl", batch_path.to_str().unwrap()]);
    assert_eq!(
        disk["result"]["items"][0]["document_id"],
        item["document_id"]
    );
    assert_eq!(graph_files(&store), graph);
    assert_indexes(&store);
}

#[test]
fn public_stream_failures_and_terminal_caps_preserve_prior_successes() {
    let store = Store::new();
    let records = b"{\"temporal_state\":\"unknown\",\"source_key\":\"a\",\"text\":\"\"}\n{bad}\n{\"temporal_state\":\"unknown\",\"source_key\":\"a\",\"text\":\"\"}\n{\"temporal_state\":\"unknown\",\"source_key\":\"b\",\"text\":\"\"}";
    let (exit, result) = stream_run(&store, &["ingest", "--jsonl", "-"], records);
    assert_eq!(exit, 2);
    assert_eq!(result["status"], "partial");
    assert_eq!(
        result["result"]["summary"],
        json!({"added":2,"updated":0,"unchanged":0,"failed":2})
    );
    assert_eq!(result["result"]["items"][1]["input"], "stdin:2");
    assert_eq!(result["result"]["items"][1]["source_key"], Value::Null);
    assert_eq!(result["result"]["items"][2]["source_key"], "a");
    for index in [1, 2] {
        let item = &result["result"]["items"][index];
        assert_eq!(item["document_id"], Value::Null);
        assert_eq!(item["revision_id"], Value::Null);
        assert_eq!(item["passage_ids"], json!([]));
        assert_eq!(item["error"]["code"], "invalid_input");
    }
    let (exit, capped) = stream_run(
        &store,
        &["ingest", "--jsonl", "-", "--max-documents", "1"],
        records,
    );
    assert_eq!(exit, 2);
    assert_eq!(
        capped["result"]["summary"],
        json!({"added":0,"updated":0,"unchanged":1,"failed":1})
    );
    assert_eq!(
        capped["result"]["items"][1]["error"]["code"],
        "limit_exceeded"
    );
    let mut oversized =
        b"{\"temporal_state\":\"unknown\",\"source_key\":\"a\",\"text\":\"\"}\n".to_vec();
    oversized.extend(vec![b'x'; 100000]);
    oversized.extend(b"\n{\"temporal_state\":\"unknown\",\"source_key\":\"never\",\"text\":\"\"}");
    let (exit, capped) = stream_run(
        &store,
        &["ingest", "--jsonl", "-", "--max-json-bytes", "64"],
        &oversized,
    );
    assert_eq!(exit, 2);
    assert_eq!(capped["result"]["items"].as_array().unwrap().len(), 2);
    assert_eq!(
        capped["result"]["items"][1]["error"]["code"],
        "limit_exceeded"
    );
    for (bytes, code) in [
        (b"\xff".as_slice(), "invalid_input"),
        (b"abcd", "limit_exceeded"),
    ] {
        let (exit, failed) = stream_run(
            &store,
            &[
                "ingest",
                "--temporal-state",
                "unknown",
                "--stdin",
                "--source-key",
                "failed",
                "--max-source-bytes",
                "3",
            ],
            bytes,
        );
        assert_eq!(exit, 2);
        assert_eq!(failed["status"], "failed");
        assert_eq!(failed["result"]["items"][0]["source_key"], "failed");
        assert_eq!(failed["result"]["items"][0]["error"]["code"], code);
    }
    let (_, empty) = stream_run(&store, &["ingest", "--jsonl", "-"], b"");
    assert_eq!(empty["status"], "complete");
    assert_eq!(empty["result"]["items"], json!([]));
    assert_indexes(&store);
}

#[test]
fn stream_options_and_descriptions_validate_without_input_or_store_mutation() {
    let store = Store::new();
    let before = store.files();
    for args in [
        vec!["ingest", "--temporal-state", "unknown", "--stdin"],
        vec!["ingest", "--temporal-state", "unknown", "--source-key", "a"],
        vec!["ingest", "--stdin", "--source-key", "a", "--jsonl", "-"],
        vec![
            "ingest",
            "--temporal-state",
            "unknown",
            "--stdin",
            "--source-key",
            "a",
            "file.txt",
        ],
        vec!["ingest", "--jsonl", "-", "--title", "ignored"],
        vec!["ingest", "--jsonl", "-", "--source-type", "file"],
        vec!["ingest", "--jsonl", "-", "--metadata", "{}"],
        vec!["ingest", "--jsonl", "-", "--recursive"],
    ] {
        let output = store.run(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty());
    }
    store.failure(
        &[
            "ingest",
            "--temporal-state",
            "unknown",
            "--stdin",
            "--source-key",
            "",
        ],
        "invalid_input",
        2,
    );
    let description = store.success(&["ingest", "--describe"]);
    let command_validator =
        jsonschema::validator_for(&description["result"]["input_schema"]).unwrap();
    let mut example = description["result"]["example"].clone();
    assert!(command_validator.is_valid(&example));
    example["paths"] = json!([]);
    assert!(!command_validator.is_valid(&example));
    example["stdin"] = json!(true);
    example["source_key"] = json!("key");
    assert!(command_validator.is_valid(&example));
    example["jsonl"] = json!("-");
    assert!(!command_validator.is_valid(&example));
    example["stdin"] = json!(false);
    example["source_key"] = Value::Null;
    example["temporal_state"] = Value::Null;
    assert!(command_validator.is_valid(&example));
    example["source_type"] = json!("file");
    assert!(!command_validator.is_valid(&example));
    let record_schema = &description["result"]["record_schema"];
    let record = &description["result"]["record_example"];
    assert!(jsonschema::is_valid(record_schema, record));
    assert!(
        commonplace::adapters::streams::parse_record(&serde_json::to_vec(record).unwrap()).is_ok()
    );
    store.assert_files(&before);
}

#[test]
fn normalized_stream_pipeline_retains_exact_revisions_and_independent_errors() {
    let store = Store::new();
    let graph = graph_files(&store);
    let mut model = DeterministicModel::default();
    let config = OperationConfig {
        maximum_source_bytes: 128,
        ..Default::default()
    };
    let text = "\u{feff}Exact\r\n\0e\u{301}\u{1f980}\n";
    let record = json!({"temporal_state":"unknown","source_key":"key","text":text,"metadata":{"b":[true,null],"a":1}});
    let ingest_record = |value: &Value, model: &mut DeterministicModel| {
        let bytes = serde_json::to_vec(value).unwrap();
        ingest::ingest(
            &store.root,
            JsonLines::new(bytes.as_slice(), "stdin".into(), 4096, 1000),
            model,
            &config,
        )
        .unwrap()
    };
    let added = ingest_record(&record, &mut model);
    let old_id = added.items[0].revision_id.unwrap();
    let old = serde_json::to_value(get::get(&store.root, &old_id.to_string()).unwrap()).unwrap();
    assert_eq!(old["text"], text);
    let calls = model.calls.len();
    let mut reordered = record.clone();
    reordered["metadata"] = json!({"a":1,"b":[true,null]});
    let same = ingest_record(&reordered, &mut model);
    assert_eq!(same.summary.unchanged, 1);
    assert_eq!(model.calls.len(), calls);
    reordered["metadata"]["a"] = json!(2);
    let updated = ingest_record(&reordered, &mut model);
    assert_eq!(updated.summary.updated, 1);
    assert_eq!(updated.items[0].document_id, added.items[0].document_id);
    assert_ne!(updated.items[0].revision_id, added.items[0].revision_id);
    let bad = [
        json!({"temporal_state":"unknown","source_key":"large","text":"x".repeat(129)}),
        json!({"temporal_state":"unknown","source_key":"float","text":"","metadata":{"x":1.5}}),
        json!({"temporal_state":"unknown","source_key":"time","text":"","occurred_at":"not a timestamp"}),
        json!({"temporal_state":"unknown","source_key":"model","text":"MODEL_FAILURE"}),
        json!({"temporal_state":"unknown","source_key":"after","text":"After failed records"}),
    ]
    .iter()
    .map(|v| format!("{v}\n"))
    .collect::<String>();
    let failures = ingest::ingest(
        &store.root,
        JsonLines::new(bad.as_bytes(), "stdin".into(), 4096, 1000),
        &mut model,
        &config,
    )
    .unwrap();
    assert_eq!(failures.summary.failed, 4);
    assert_eq!(failures.summary.added, 1);
    assert_eq!(failures.exit_code, 1);
    assert_eq!(model.loads, 1);
    let old_again =
        serde_json::to_value(get::get(&store.root, &old_id.to_string()).unwrap()).unwrap();
    assert_eq!(old_again, old);
    assert_eq!(graph_files(&store), graph);
    assert_indexes(&store);
}

#[test]
fn missing_store_never_consumes_lazy_input_and_stream_io_failure_is_terminal() {
    let temp = tempfile::tempdir().unwrap();
    let items = std::iter::once_with(|| panic!("must not consume stdin for missing store"));
    assert!(
        ingest::ingest(
            &temp.path().join("missing"),
            items,
            &mut DeterministicModel::default(),
            &OperationConfig::default()
        )
        .is_err()
    );

    struct FailingReader {
        bytes: std::io::Cursor<Vec<u8>>,
    }
    impl std::io::Read for FailingReader {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            unreachable!()
        }
    }
    impl std::io::BufRead for FailingReader {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            if self.bytes.position() as usize == self.bytes.get_ref().len() {
                Err(std::io::Error::other("injected stream failure"))
            } else {
                self.bytes.fill_buf()
            }
        }
        fn consume(&mut self, count: usize) {
            self.bytes.consume(count);
        }
    }
    let store = Store::new();
    let reader = FailingReader {
        bytes: std::io::Cursor::new(
            b"{\"temporal_state\":\"unknown\",\"source_key\":\"a\",\"text\":\"\"}\n".to_vec(),
        ),
    };
    let result = ingest::ingest(
        &store.root,
        JsonLines::new(reader, "stdin".into(), 1024, 10),
        &mut DeterministicModel::default(),
        &OperationConfig::default(),
    )
    .unwrap();
    assert_eq!(result.summary.added, 1);
    assert_eq!(result.summary.failed, 1);
    assert_eq!(result.exit_code, 1);
    assert_eq!(
        result.items[1].error.as_ref().unwrap().code,
        "internal_error"
    );
    assert_indexes(&store);
}

#[derive(Default)]
struct DeterministicModel {
    calls: Vec<usize>,
    loads: usize,
    root: Option<PathBuf>,
    on_embed: Option<Box<dyn FnMut()>>,
}

impl EmbeddingModel for DeterministicModel {
    fn identity(&self) -> &'static str {
        EMBEDDING_IDENTITY
    }
    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if self.loads == 0 {
            self.loads = 1;
        }
        self.calls.push(texts.len());
        if let Some(root) = &self.root {
            let mut writer = SqliteDatabase::write(root, std::time::Duration::ZERO)?;
            writer.transaction()?.commit().unwrap();
        }
        if let Some(hook) = self.on_embed.as_mut() {
            hook();
        }
        if texts.iter().any(|text| text.contains("MODEL_FAILURE")) {
            return Err(CommonplaceError::ModelUnavailable(
                "injected local inference failure".into(),
            ));
        }
        Ok(texts
            .iter()
            .map(|_| {
                let mut vector = vec![0.0; EMBEDDING_DIMENSIONS];
                vector[0] = 1.0;
                vector
            })
            .collect())
    }
}

fn input(key: &str, text: &str) -> InputItem {
    InputItem {
        input: key.into(),
        source_key: Some(key.into()),
        document: Ok(DocumentInput {
            source_key: key.into(),
            text: text.into(),
            title: Some("A title".into()),
            source_type: "file".into(),
            occurred_at: None,
            temporal_state: commonplace::domain::documents::TemporalState::Unknown,
            metadata: serde_json::Map::new(),
        }),
    }
}

fn run(
    store: &Store,
    items: Vec<InputItem>,
    model: &mut DeterministicModel,
) -> ingest::IngestResult {
    ingest::ingest(
        &store.root,
        items.into_iter(),
        model,
        &OperationConfig {
            embedding_batch_size: 2,
            ..Default::default()
        },
    )
    .unwrap()
}

fn assert_indexes(store: &Store) {
    let session = SqliteDatabase::read(&store.root).unwrap();
    let read_ids = |sql| {
        session
            .connection()
            .prepare(sql)
            .unwrap()
            .query_map([], |row| row.get::<_, i64>(0))
            .unwrap()
            .map(std::result::Result::unwrap)
            .collect::<Vec<_>>()
    };
    let current = read_ids("SELECT passage_id FROM passages p JOIN document_revisions r USING(revision_id)
        WHERE r.revision_number = (SELECT max(revision_number) FROM document_revisions WHERE document_id = r.document_id)
        ORDER BY passage_id");
    assert_eq!(
        read_ids("SELECT rowid FROM passage_fts ORDER BY rowid"),
        current
    );
    assert_eq!(
        read_ids("SELECT passage_id FROM passage_vectors ORDER BY passage_id"),
        current
    );
    assert!(
        session
            .connection()
            .query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
            .is_err()
    );
}

fn graph_files(store: &Store) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    store
        .files()
        .into_iter()
        .filter(|(path, _)| path.starts_with(store.root.join("graph")))
        .collect()
}

#[test]
fn complete_pipeline_retains_exact_revisions_and_current_indexes() {
    let store = Store::new();
    let graph = graph_files(&store);
    let text = format!(
        "\u{feff}CRLF\r\n\r\nNUL\0e\u{301}🦀\n\n{}",
        "🦀".repeat(800)
    );
    let mut model = DeterministicModel {
        root: Some(store.root.clone()),
        ..Default::default()
    };
    let first = run(
        &store,
        vec![input("a", &text), input("b", "second"), input("empty", "")],
        &mut model,
    );
    assert_eq!(first.summary.added, 3);
    assert_eq!(first.status(), "complete");
    assert_eq!(model.loads, 1);
    assert!(model.calls.iter().all(|size| *size <= 2));
    let calls = model.calls.len();
    let repeated = run(
        &store,
        vec![input("a", &text), input("b", "second")],
        &mut model,
    );
    assert_eq!(repeated.summary.unchanged, 2);
    assert_eq!(model.calls.len(), calls);
    let mut metadata_only = input("a", &text);
    metadata_only
        .document
        .as_mut()
        .unwrap()
        .metadata
        .insert("edition".into(), json!(2));
    let updated = run(&store, vec![metadata_only], &mut model);
    assert_eq!(updated.summary.updated, 1);
    let old_id = first.items[0].revision_id.unwrap();
    let new_id = updated.items[0].revision_id.unwrap();
    assert_ne!(old_id, new_id);
    let doc_id = first.items[0].document_id.unwrap();
    let doc: Value =
        serde_json::to_value(get::get(&store.root, &doc_id.to_string()).unwrap()).unwrap();
    assert_eq!(doc["revision_ids"], json!([old_id, new_id]));
    assert_eq!(doc["current_revision_id"], json!(new_id));
    for revision_id in [old_id, new_id] {
        let revision: Value =
            serde_json::to_value(get::get(&store.root, &revision_id.to_string()).unwrap()).unwrap();
        assert_eq!(revision["text"], text);
        for passage_id in revision["passage_ids"].as_array().unwrap() {
            let passage = store.success(&["get", passage_id.as_str().unwrap()]);
            let result = &passage["result"];
            let start = result["start_byte"].as_u64().unwrap() as usize;
            let end = result["end_byte"].as_u64().unwrap() as usize;
            assert_eq!(result["text"], &text[start..end]);
            assert_eq!(result["revision_id"], json!(revision_id));
            assert_eq!(result["source_key"], "a");
            assert_eq!(result["title"], "A title");
        }
    }
    assert_indexes(&store);
    assert_eq!(graph_files(&store), graph);
    let db = store.database();
    let timestamps: (String, String, String) = db
        .query_row(
            "SELECT d.created_at, d.last_ingested_at, r.created_at FROM documents d
         JOIN document_revisions r USING(document_id) WHERE d.source_key='b'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(timestamps.0, timestamps.2);
    assert!(timestamps.1 >= timestamps.0);
    assert_eq!(
        db.query_row("SELECT knowledge_version FROM store_state", [], |row| row
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
}

#[test]
fn failures_are_independent_and_search_publication_rolls_back() {
    let store = Store::new();
    let mut model = DeterministicModel::default();
    let original = run(&store, vec![input("a", "original")], &mut model);
    let original_revision = original.items[0].revision_id;
    // The first replacement passage has already reached both virtual tables when the second fails.
    store.database().execute_batch(
        "CREATE TRIGGER reject_second_passage BEFORE INSERT ON passages WHEN NEW.ordinal = 1 BEGIN
           INSERT INTO passage_vectors(passage_id, embedding) VALUES (NEW.passage_id, X'00000000');
         END;"
    ).unwrap();
    let failed = run(
        &store,
        vec![
            input("good", "good"),
            input("a", &"x".repeat(2048)),
            input("model", "MODEL_FAILURE"),
            input("good", "duplicate"),
            input("after", "after"),
        ],
        &mut model,
    );
    assert_eq!(failed.status(), "partial");
    assert_eq!(failed.exit_code, 1);
    assert_eq!(failed.summary.added, 2);
    assert_eq!(failed.summary.failed, 3);
    assert_eq!(failed.items[1].error.as_ref().unwrap().stage, "publish");
    assert_eq!(
        failed.items[2].error.as_ref().unwrap().code,
        "model_unavailable"
    );
    assert_eq!(
        failed.items[3].error.as_ref().unwrap().code,
        "invalid_input"
    );
    let current = commonplace::storage::documents::current(
        SqliteDatabase::read(&store.root).unwrap().connection(),
        "a",
    )
    .unwrap()
    .unwrap();
    assert_eq!(Some(current.revision_id), original_revision);
    assert_eq!(current.text, "original");
    assert_eq!(
        store
            .database()
            .query_row("SELECT count(*) FROM document_revisions", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_indexes(&store);
}

#[test]
fn rechecks_current_revision_under_lock_and_preserves_read_snapshot() {
    let store = Store::new();
    let mut model = DeterministicModel::default();
    let first = run(&store, vec![input("a", "old")], &mut model);
    let id = first.items[0].document_id.unwrap();
    let snapshot = SqliteDatabase::read(&store.root).unwrap();
    let root = store.root.clone();
    model.on_embed = Some(Box::new(move || {
        let mut other = DeterministicModel::default();
        let result = ingest::ingest(
            &root,
            vec![input("a", "concurrent")].into_iter(),
            &mut other,
            &OperationConfig::default(),
        )
        .unwrap();
        assert_eq!(result.summary.updated, 1);
    }));
    let attempted = run(&store, vec![input("a", "stale")], &mut model);
    assert_eq!(attempted.exit_code, 3);
    assert_eq!(attempted.items[0].error.as_ref().unwrap().code, "conflict");
    assert_eq!(
        evidence::document(snapshot.connection(), id)
            .unwrap()
            .revision_ids
            .len(),
        1
    );
    let fresh = SqliteDatabase::read(&store.root).unwrap();
    assert_eq!(
        evidence::document(fresh.connection(), id)
            .unwrap()
            .revision_ids
            .len(),
        2
    );
    assert_indexes(&store);
}

#[test]
fn configured_source_passage_and_request_bounds_are_enforced() {
    let store = Store::new();
    let mut model = DeterministicModel::default();
    let config = OperationConfig {
        maximum_source_bytes: 1025,
        maximum_documents: 2,
        maximum_passages: 1,
        ..Default::default()
    };
    let result = ingest::ingest(
        &store.root,
        vec![input("a", &"a".repeat(1025)), input("b", &"b".repeat(1026))].into_iter(),
        &mut model,
        &config,
    )
    .unwrap();
    assert_eq!(result.summary.failed, 2);
    assert_eq!(result.items[0].error.as_ref().unwrap().stage, "prepare");
    assert_eq!(result.items[1].error.as_ref().unwrap().stage, "input");
    assert!(model.calls.is_empty());
    let error = ingest::ingest(
        &store.root,
        vec![input("a", ""), input("b", ""), input("c", "")].into_iter(),
        &mut model,
        &config,
    )
    .unwrap_err();
    assert_eq!(error.code(), "limit_exceeded");
    assert_eq!(
        store
            .database()
            .query_row("SELECT count(*) FROM documents", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn every_metadata_field_revises_and_empty_replacement_clears_only_current_indexes() {
    let store = Store::new();
    let mut model = DeterministicModel::default();
    let mut document = input("a", "retained").document.unwrap();
    let mut ids = Vec::new();
    for change in 0..5 {
        match change {
            1 => document.title = None,
            2 => document.source_type = "note".into(),
            3 => {
                document.temporal_state = commonplace::domain::documents::TemporalState::Dated;
                document.occurred_at = Some("2026-09-28T00:00:00Z".into());
            }
            4 => {
                document.metadata.insert("edition".into(), json!(2));
            }
            _ => {}
        }
        let result = run(
            &store,
            vec![InputItem {
                input: "a".into(),
                source_key: Some("a".into()),
                document: Ok(document.clone()),
            }],
            &mut model,
        );
        assert_eq!(result.summary.added + result.summary.updated, 1);
        ids.push(result.items[0].revision_id.unwrap());
    }
    let empty = run(&store, vec![input("a", "")], &mut model);
    assert_eq!(empty.summary.updated, 1);
    assert!(empty.items[0].passage_ids.is_empty());
    let session = SqliteDatabase::read(&store.root).unwrap();
    for id in ids {
        assert_eq!(
            evidence::revision(session.connection(), id).unwrap().text,
            "retained"
        );
    }
    assert_indexes(&store);
    assert_eq!(
        session
            .connection()
            .query_row("SELECT count(*) FROM passage_vectors", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn scan_absence_never_deletes_and_disappeared_inputs_fail_individually() {
    let store = Store::new();
    let notes = store.directory.path().join("notes");
    std::fs::create_dir(&notes).unwrap();
    let file = notes.join("a.md");
    std::fs::write(&file, "").unwrap();
    let added = store.success(&[
        "ingest",
        "--temporal-state",
        "unknown",
        notes.to_str().unwrap(),
    ]);
    let doc_id = added["result"]["items"][0]["document_id"].as_str().unwrap();
    let options = commonplace::adapters::sources::FileOptions {
        paths: vec![notes.clone()],
        ..Default::default()
    };
    let enumerated = commonplace::adapters::sources::enumerate(&options, 10).unwrap();
    std::fs::remove_file(&file).unwrap();
    let missing = commonplace::adapters::sources::read_file(
        &enumerated[0].path,
        "known-key".into(),
        &commonplace::adapters::sources::MetadataOverrides {
            title: None,
            source_type: "file".into(),
            temporal_state: commonplace::domain::documents::TemporalState::Unknown,
            occurred_at: None,
            metadata: serde_json::Map::new(),
        },
        100,
    )
    .unwrap_err();
    assert_eq!(missing.code(), "invalid_input");
    let scan = store.success(&[
        "ingest",
        "--temporal-state",
        "unknown",
        notes.to_str().unwrap(),
    ]);
    assert_eq!(
        scan["result"]["summary"],
        json!({"added":0,"updated":0,"unchanged":0,"failed":0})
    );
    assert_eq!(
        store.success(&["get", doc_id])["result"]["revision_ids"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn get_rejects_a_corrupt_quote_instead_of_returning_false_evidence() {
    let store = Store::new();
    let result = run(
        &store,
        vec![input("a", "quote")],
        &mut DeterministicModel::default(),
    );
    let passage = result.items[0].passage_ids[0];
    store
        .database()
        .execute(
            "UPDATE passages SET text='wrong' WHERE passage_id=?1",
            [passage.value()],
        )
        .unwrap();
    store.failure(&["get", &passage.to_string()], "internal_error", 1);
}

#[test]
fn public_binary_file_commands_and_descriptions_need_no_model_for_empty_sources() {
    let store = Store::new();
    let a = store.directory.path().join("a.md");
    let b = store.directory.path().join("b.txt");
    std::fs::write(&a, "").unwrap();
    std::fs::write(&b, "").unwrap();
    let added = store.success(&[
        "ingest",
        "--temporal-state",
        "unknown",
        a.to_str().unwrap(),
        b.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(added["result"]["summary"]["added"], 2);
    let unchanged = store.success(&["ingest", "--temporal-state", "unknown", a.to_str().unwrap()]);
    assert_eq!(unchanged["result"]["summary"]["unchanged"], 1);
    let updated = store.success(&[
        "ingest",
        "--temporal-state",
        "dated",
        a.to_str().unwrap(),
        "--metadata",
        "{\"k\":2}",
        "--occurred-at",
        "2026-01-01T02:00:00+02:00",
    ]);
    assert_eq!(updated["result"]["summary"]["updated"], 1);
    let revision = store.success(&[
        "get",
        updated["result"]["items"][0]["revision_id"]
            .as_str()
            .unwrap(),
    ]);
    assert_eq!(revision["result"]["text"], "");
    assert_eq!(revision["result"]["metadata"], json!({"k": 2}));
    assert_eq!(revision["result"]["occurred_at"], "2026-01-01T00:00:00Z");
    assert_eq!(revision["result"]["passage_ids"], json!([]));
    store.failure(&["get", "revision:9999"], "not_found", 2);
    store.failure(&["get", "entity:1"], "not_found", 2);
    store.failure(
        &[
            "ingest",
            "--temporal-state",
            "unknown",
            a.to_str().unwrap(),
            "--metadata",
            "{\"x\":1.5}",
        ],
        "invalid_input",
        2,
    );
    store.failure(
        &[
            "ingest",
            "--temporal-state",
            "dated",
            a.to_str().unwrap(),
            "--occurred-at",
            "yesterday",
        ],
        "invalid_input",
        2,
    );
    store.failure(
        &[
            "ingest",
            "--temporal-state",
            "unknown",
            a.to_str().unwrap(),
            "--max-documents",
            "0",
        ],
        "invalid_input",
        2,
    );
    store.failure(
        &[
            "ingest",
            "--temporal-state",
            "unknown",
            a.to_str().unwrap(),
            "--include",
            "[",
        ],
        "invalid_input",
        2,
    );
    store.failure(
        &[
            "ingest",
            "--temporal-state",
            "unknown",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            "--max-documents",
            "1",
        ],
        "limit_exceeded",
        2,
    );
    store.failure(
        &[
            "ingest",
            "--temporal-state",
            "unknown",
            a.to_str().unwrap(),
            "--metadata",
            "{\"x\":0}",
            "--max-json-bytes",
            "6",
        ],
        "limit_exceeded",
        2,
    );
    let before = store.files();
    let description = store.success(&["ingest", "--describe", "--json"]);
    let result = &description["result"];
    let validator = jsonschema::validator_for(&result["input_schema"]).unwrap();
    assert!(validator.is_valid(&result["example"]));
    assert_eq!(
        result["input_schema"]["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    let mut invalid = result["example"].clone();
    invalid["max_passages"] = json!(0);
    assert!(!validator.is_valid(&invalid));
    store.assert_files(&before);
    let output =
        common::isolated_command(env!("CARGO_BIN_EXE_commonplace"), store.directory.path())
            .args([
                "--store",
                "/nonexistent/commonplace",
                "ingest",
                "--describe",
            ])
            .output()
            .unwrap();
    assert!(output.status.success());
}

#[test]
fn binary_partial_failures_keep_prior_items_and_missing_models_are_explicit() {
    let store = Store::new();
    let empty = store.directory.path().join("empty.md");
    let invalid = store.directory.path().join("invalid.txt");
    let text = store.directory.path().join("text.md");
    std::fs::write(&empty, "").unwrap();
    std::fs::write(&invalid, [0xff]).unwrap();
    std::fs::write(&text, "actual source").unwrap();
    let output = store
        .command()
        .env(
            "COMMONPLACE_MODEL_CACHE",
            store.directory.path().join("missing-cache"),
        )
        .args([
            "ingest",
            "--temporal-state",
            "unknown",
            empty.to_str().unwrap(),
            invalid.to_str().unwrap(),
            text.to_str().unwrap(),
            empty.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "partial");
    assert_eq!(
        value["result"]["summary"],
        json!({"added":1,"updated":0,"unchanged":0,"failed":3})
    );
    assert_eq!(
        value["result"]["items"][1]["error"]["code"],
        "invalid_input"
    );
    assert_eq!(
        value["result"]["items"][2]["error"]["code"],
        "model_unavailable"
    );
    assert_eq!(
        value["result"]["items"][3]["error"]["code"],
        "invalid_input"
    );
    assert_indexes(&store);
}
