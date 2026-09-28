mod common;

use std::time::Duration;

use common::Store;
use commonplace::app::{ingest, remove, search};
use commonplace::domain::documents::DocumentInput;
use commonplace::domain::remove::RemoveInput;
use commonplace::domain::search::SearchRequest;
use commonplace::providers::embeddings::{
    EMBEDDING_DIMENSIONS, EMBEDDING_IDENTITY, EmbeddingModel,
};
use commonplace::providers::reranker::Reranker;
use commonplace::storage::database::SqliteDatabase;
use commonplace::{Result, graph::GraphRuntime};
use serde_json::{Value, json};

struct Model;
impl EmbeddingModel for Model {
    fn identity(&self) -> &'static str {
        EMBEDDING_IDENTITY
    }
    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
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
struct Ranker;
impl Reranker for Ranker {
    fn rerank(&mut self, _: &str, texts: &[&str]) -> Result<Vec<f32>> {
        Ok(vec![0.0; texts.len()])
    }
}

fn ingest(store: &Store, key: &str, text: &str) -> Value {
    let result = ingest::ingest(
        &store.root,
        [ingest::InputItem {
            input: key.into(),
            source_key: Some(key.into()),
            document: Ok(DocumentInput {
                source_key: key.into(),
                text: text.into(),
                title: None,
                source_type: "test".into(),
                occurred_at: None,
                metadata: Default::default(),
            }),
        }]
        .into_iter(),
        &mut Model,
        &ingest::OperationConfig::default(),
    )
    .unwrap();
    assert_eq!(result.summary.failed, 0);
    serde_json::to_value(result).unwrap()["items"][0].clone()
}

fn record(store: &Store, items: Value) -> Value {
    let path = store.input(&json!({"items":items}));
    store.success(&["record", path.to_str().unwrap()])
}

fn ids(store: &Store, sql: &str) -> Vec<i64> {
    let session = SqliteDatabase::read(&store.root).unwrap();
    session
        .connection()
        .prepare(sql)
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn state(store: &Store) -> Vec<Vec<i64>> {
    [
        "SELECT document_id FROM documents ORDER BY document_id",
        "SELECT revision_id FROM document_revisions ORDER BY revision_id",
        "SELECT passage_id FROM passages ORDER BY passage_id",
        "SELECT rowid FROM passage_fts ORDER BY rowid",
        "SELECT passage_id FROM passage_vectors ORDER BY passage_id",
        "SELECT passage_id FROM knowledge_item_evidence ORDER BY knowledge_item_id,passage_id",
        "SELECT knowledge_version FROM store_state",
    ]
    .iter()
    .map(|sql| ids(store, sql))
    .collect()
}

#[test]
fn removes_all_revisions_retains_knowledge_unrelated_evidence_and_new_identity() {
    let store = Store::new();
    store.apply(&json!({"entity_types":[{"name":"person"}],"predicates":[
        {"name":"knows","object_kind":"entity","subject_types":["person"],"object_types":["person"]},
        {"name":"decision","object_kind":"string","subject_types":["person"]}
    ]}), false);
    let old = ingest(&store, "opaque:key", "old needle evidence");
    let new = ingest(&store, "opaque:key", "new needle evidence");
    let unrelated = ingest(&store, "other:key", "unrelated evidence");
    let old_passage = &old["passage_ids"][0];
    let new_passage = &new["passage_ids"][0];
    let other_passage = &unrelated["passage_ids"][0];
    record(
        &store,
        json!([
            {"kind":"entity","ref":"e","name":"Retained"},
            {"kind":"type_membership","entity":{"ref":"e"},"entity_type":"person",
                "support":[{"passage_id":old_passage},{"passage_id":new_passage}]},
            {"kind":"type_membership","entity":{"ref":"e"},"entity_type":"person",
                "support":[{"passage_id":old_passage},{"passage_id":other_passage}]},
            {"kind":"type_membership","entity":{"ref":"e"},"entity_type":"person",
                "support":[{"passage_id":other_passage}]},
            {"kind":"type_membership","entity":{"ref":"e"},"entity_type":"person",
                "support":[{"passage_id":old_passage}]},
            {"kind":"fact","subject":{"ref":"e"},"predicate":"knows","object":{"entity":{"ref":"e"}},
                "support":[{"passage_id":old_passage},{"passage_id":new_passage}]},
            {"kind":"fact","subject":{"ref":"e"},"predicate":"decision","object":{"literal":"approved"},
                "support":[{"passage_id":old_passage},{"passage_id":new_passage}]},
            {"kind":"fact","subject":{"ref":"e"},"predicate":"knows","object":{"entity":{"ref":"e"}},
                "support":[{"passage_id":old_passage},{"passage_id":other_passage}]},
            {"kind":"fact","subject":{"ref":"e"},"predicate":"decision","object":{"literal":"approved"},
                "support":[{"passage_id":old_passage},{"passage_id":other_passage}]},
            {"kind":"fact","subject":{"ref":"e"},"predicate":"knows","object":{"entity":{"ref":"e"}},
                "support":[{"passage_id":other_passage}]},
            {"kind":"fact","subject":{"ref":"e"},"predicate":"decision","object":{"literal":"approved"},
                "support":[{"passage_id":other_passage}]}
        ]),
    );
    // Retained withdrawn rows predate this operation; removal does not author withdrawal.
    store.database().execute(
        "UPDATE knowledge_items SET withdrawn_at='2026-01-01T00:00:00Z',withdrawn_by='test' WHERE knowledge_item_id=4", [],
    ).unwrap();
    store.success(&["graph", "rebuild"]);
    let knowledge_before: Vec<_> = (1..=10)
        .map(|id| store.success(&["get", &format!("knowledge:{id}")])["result"].clone())
        .collect();
    let unrelated_before = store.success(&["get", unrelated["document_id"].as_str().unwrap()]);
    let result = store.success(&["remove", "--source-key", "opaque:key", "--json"]);
    assert_eq!(
        result,
        json!({
            "operation":"remove","contract_version":"1","status":"complete",
            "result":{"source_key":"opaque:key","document_id":old["document_id"],
                "deleted_revisions":2,"deleted_passages":2,"detached_evidence":10,
                "affected_knowledge_ids":["knowledge:1","knowledge:2","knowledge:4","knowledge:5","knowledge:6","knowledge:7","knowledge:8"],
                "knowledge_version":2}
        })
    );
    for id in [
        &old["document_id"],
        &old["revision_id"],
        &new["revision_id"],
        old_passage,
        new_passage,
    ] {
        store.failure(&["get", id.as_str().unwrap()], "not_found", 2);
    }
    let remaining = state(&store);
    assert_eq!(remaining[0], vec![2]);
    assert_eq!(remaining[1], vec![3]);
    for index in [2, 3, 4, 5] {
        assert_eq!(
            remaining[index],
            if index == 5 { vec![3; 6] } else { vec![3] }
        );
    }
    assert_eq!(remaining[6], vec![2]);
    for (index, before) in knowledge_before.iter().enumerate() {
        let mut expected = before.clone();
        expected["support"]
            .as_array_mut()
            .unwrap()
            .retain(|e| e["source_key"] != "opaque:key");
        assert_eq!(
            store.success(&["get", &format!("knowledge:{}", index + 1)])["result"],
            expected
        );
    }
    assert_eq!(
        store.success(&["get", unrelated["document_id"].as_str().unwrap()]),
        unrelated_before
    );
    assert_eq!(ids(&store, "SELECT entity_id FROM entities"), vec![1]);
    assert_eq!(
        ids(&store, "SELECT entity_type_id FROM entity_types"),
        vec![1]
    );
    assert_eq!(
        ids(
            &store,
            "SELECT knowledge_item_id FROM facts ORDER BY knowledge_item_id"
        ),
        (5..=10).collect::<Vec<_>>()
    );
    assert_eq!(
        store.success(&["get", "entity:1"])["result"]["active_fact_ids"],
        json!([
            "knowledge:5",
            "knowledge:6",
            "knowledge:7",
            "knowledge:8",
            "knowledge:9",
            "knowledge:10"
        ])
    );
    let fact_query = "PREFIX c:<urn:commonplace:property:>
        SELECT ?k ?s ?predicate ?object ?json ?p WHERE {
            ?k c:kind \"fact\"; c:subject ?s; c:predicate ?predicate; c:object ?object .
            OPTIONAL {?k c:literal_json ?json} OPTIONAL {?k c:evidence ?p}
        } ORDER BY ?k";
    let fact_graph = store.success(&["graph", "query", fact_query])["result"].clone();
    assert_eq!(fact_graph["rows"].as_array().unwrap().len(), 6);
    for index in 0..6 {
        let row = fact_graph["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row[0]["value"] == format!("urn:commonplace:knowledge:{}", index + 5))
            .unwrap();
        assert_eq!(row[1]["value"], "urn:commonplace:entity:1");
        if index % 2 == 0 {
            assert_eq!(
                row[3],
                json!({"type":"uri","value":"urn:commonplace:entity:1"})
            );
            assert_eq!(row[4], Value::Null);
        } else {
            assert_eq!(
                row[3],
                json!({"type":"literal","value":"approved",
                "datatype":"http://www.w3.org/2001/XMLSchema#string","language":null})
            );
            assert_eq!(row[4]["value"], "\"approved\"");
        }
        if index < 2 {
            assert_eq!(row[5], Value::Null);
        } else {
            assert_eq!(
                row[5]["value"],
                format!("urn:commonplace:{}", other_passage.as_str().unwrap())
            );
        }
    }
    let query = "PREFIX c:<urn:commonplace:property:>
        SELECT ?k ?p WHERE {?k c:kind \"type_membership\" OPTIONAL {?k c:evidence ?p}} ORDER BY ?k";
    let graph = store.success(&["graph", "query", query])["result"].clone();
    assert_eq!(graph["rows"].as_array().unwrap().len(), 3);
    assert_eq!(graph["rows"][0][0]["value"], "urn:commonplace:knowledge:1");
    assert_eq!(graph["rows"][0][1], Value::Null);
    for row in graph["rows"].as_array().unwrap().iter().skip(1) {
        assert_eq!(
            row[1]["value"],
            format!("urn:commonplace:{}", other_passage.as_str().unwrap())
        );
    }
    assert_eq!(
        store.success(&[
            "graph",
            "query",
            "PREFIX c:<urn:commonplace:property:> SELECT ?d WHERE {?d c:source_key \"opaque:key\"}"
        ])["result"]["rows"],
        json!([])
    );
    let found = search::search(
        &store.root,
        &SearchRequest {
            query: "needle".into(),
            since: None,
            source_types: vec![],
            limit: 10,
        },
        &mut Model,
        &mut Ranker,
    )
    .unwrap();
    let found = serde_json::to_value(found).unwrap();
    assert_eq!(found["items"].as_array().unwrap().len(), 1);
    assert_eq!(found["items"][0]["source_key"], "other:key");
    let all_query = "SELECT ?s ?p ?o WHERE {?s ?p ?o} ORDER BY ?s ?p ?o";
    let canonical_graph = store.success(&["graph", "query", all_query])["result"].clone();
    assert_eq!(canonical_graph["truncated"], false);
    store.success(&["init"]);
    store.success(&["graph", "rebuild"]);
    assert_eq!(store.success(&["graph", "query", query])["result"], graph);
    assert_eq!(
        store.success(&["graph", "query", all_query])["result"],
        canonical_graph
    );
    assert_eq!(
        store.success(&["graph", "query", fact_query])["result"],
        fact_graph
    );
    let before = state(&store);
    store.failure(&["remove", "--source-key", "opaque:key"], "not_found", 2);
    assert_eq!(state(&store), before);
    let reingested = ingest(&store, "opaque:key", "old needle evidence");
    assert_ne!(reingested["document_id"], old["document_id"]);
    assert_ne!(reingested["revision_id"], old["revision_id"]);
    assert_ne!(reingested["passage_ids"], old["passage_ids"]);
    assert_eq!(
        store.success(&["get", "knowledge:1"])["result"]["support"],
        json!([])
    );
    assert_eq!(store.success(&["graph", "query", query])["result"], graph);
    assert_eq!(
        store.success(&["graph", "query", all_query])["result"],
        canonical_graph
    );
    assert_eq!(
        store.success(&["graph", "query", fact_query])["result"],
        fact_graph
    );
    for (index, before) in knowledge_before.iter().enumerate() {
        let mut expected = before.clone();
        expected["support"]
            .as_array_mut()
            .unwrap()
            .retain(|e| e["source_key"] != "opaque:key");
        assert_eq!(
            store.success(&["get", &format!("knowledge:{}", index + 1)])["result"],
            expected
        );
    }
}

#[test]
fn descriptions_exact_keys_empty_sources_and_numeric_affected_order() {
    let directory = tempfile::tempdir().unwrap();
    let absent = directory.path().join("absent");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_commonplace"))
        .args([
            "--store",
            absent.to_str().unwrap(),
            "remove",
            "--describe",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(!absent.exists());
    let description: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(description["operation"], "remove.describe");
    let schema = jsonschema::validator_for(&description["result"]["input_schema"]).unwrap();
    assert!(schema.is_valid(&description["result"]["example"]));
    for invalid in [
        json!({}),
        json!({"source_key":""}),
        json!({"source_key":"a\0b"}),
        json!({"source_key":"x","extra":1}),
    ] {
        assert!(!schema.is_valid(&invalid));
    }
    for args in [
        vec!["remove"],
        vec!["remove", "--describe", "--source-key", "x"],
        vec!["remove", "--source-key", "x", "--source-key", "y"],
        vec!["remove", "input.json"],
    ] {
        assert!(
            !std::process::Command::new(env!("CARGO_BIN_EXE_commonplace"))
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let store = Store::new();
    store.failure(&["remove", "--source-key", ""], "invalid_input", 2);
    assert!(
        remove::remove(
            &store.root,
            RemoveInput {
                source_key: "x\0y".into()
            },
            Duration::ZERO
        )
        .is_err()
    );
    ingest(&store, " Exact/../Key ", "");
    for key in ["Exact/../Key", " Exact/../key ", "Key"] {
        store.failure(&["remove", "--source-key", key], "not_found", 2);
    }
    let removed = store.success(&["remove", "--source-key", " Exact/../Key "]);
    assert_eq!(removed["result"]["deleted_revisions"], 1);
    assert_eq!(removed["result"]["deleted_passages"], 0);
    assert_eq!(removed["result"]["detached_evidence"], 0);
    assert_eq!(removed["result"]["affected_knowledge_ids"], json!([]));
    assert_eq!(removed["result"]["knowledge_version"], 1);
    let evidence = ingest(&store, "many", "shared");
    store.apply(&json!({"entity_types":[{"name":"person"}]}), false);
    let mut items = vec![json!({"kind":"entity","ref":"e","name":"E"})];
    for _ in 0..12 {
        items.push(
            json!({"kind":"type_membership","entity":{"ref":"e"},"entity_type":"person",
            "support":[{"passage_id":evidence["passage_ids"][0]}]}),
        );
    }
    record(&store, json!(items));
    let removed = store.success(&["remove", "--source-key", "many"]);
    assert_eq!(
        removed["result"]["affected_knowledge_ids"],
        json!(
            (1..=12)
                .map(|id| format!("knowledge:{id}"))
                .collect::<Vec<_>>()
        )
    );
    assert_eq!(removed["result"]["detached_evidence"], 12);
}

#[test]
fn file_keys_and_directory_absence_do_not_remove_or_touch_original_files() {
    let store = Store::new();
    let directory = store.directory.path().join("files");
    std::fs::create_dir(&directory).unwrap();
    let first = directory.join("first.md");
    let second = directory.join("second.md");
    std::fs::write(&first, "").unwrap();
    std::fs::write(&second, "").unwrap();
    let added = store.success(&["ingest", directory.to_str().unwrap()]);
    let doc = added["result"]["items"][0]["document_id"].as_str().unwrap();
    let before = store.success(&["get", doc]);
    let key = before["result"]["source_key"].as_str().unwrap();
    assert!(key.starts_with("file://"));
    std::fs::remove_file(&first).unwrap();
    store.success(&["ingest", directory.to_str().unwrap()]);
    assert_eq!(store.success(&["get", doc]), before);
    store.success(&["remove", "--source-key", key]);
    store.failure(&["get", doc], "not_found", 2);
    let other_doc = added["result"]["items"][1]["document_id"].as_str().unwrap();
    let other = store.success(&["get", other_doc]);
    store.success(&[
        "remove",
        "--source-key",
        other["result"]["source_key"].as_str().unwrap(),
    ]);
    assert!(second.is_file());
}

#[test]
fn late_sqlite_graph_and_version_failures_restore_complete_deletion() {
    for failure in ["delete", "version", "graph", "overflow"] {
        let store = Store::new();
        let evidence = ingest(&store, "source", "old evidence");
        ingest(&store, "source", "current evidence");
        store.apply(&json!({"entity_types":[{"name":"person"}]}), false);
        record(
            &store,
            json!([
                {"kind":"entity","ref":"e","name":"E"},
                {"kind":"type_membership","entity":{"ref":"e"},"entity_type":"person",
                    "support":[{"passage_id":evidence["passage_ids"][0]}]}
            ]),
        );
        match failure {
            "delete" => store.database().execute_batch(
                "CREATE TRIGGER reject_delete BEFORE DELETE ON documents BEGIN SELECT RAISE(ABORT,'late delete'); END;").unwrap(),
            "version" => store.database().execute_batch(
                "CREATE TRIGGER reject_version BEFORE UPDATE ON store_state BEGIN SELECT RAISE(ABORT,'late version'); END;").unwrap(),
            "graph" => std::fs::write(store.root.join("graph/candidate"), "leftover").unwrap(),
            "overflow" => {
                store.database().execute_batch("UPDATE store_state SET knowledge_version=9223372036854775807").unwrap();
                store.success(&["graph","rebuild"]);
            }
            _ => unreachable!(),
        }
        let before = state(&store);
        let graph = store.graph_files();
        let knowledge = store.success(&["get", "knowledge:1"]);
        store.failure(
            &["remove", "--source-key", "source"],
            if failure == "graph" {
                "graph_unavailable"
            } else {
                "internal_error"
            },
            1,
        );
        assert_eq!(state(&store), before, "{failure}");
        assert_eq!(store.graph_files(), graph, "{failure}");
        assert_eq!(store.success(&["get", "knowledge:1"]), knowledge);
        GraphRuntime::open(&store.root).unwrap();
    }
}

#[test]
fn reader_and_writer_leases_prevent_removal_without_partial_changes() {
    let store = Store::new();
    ingest(&store, "source", "evidence");
    let before = state(&store);
    let graph = store.graph_files();
    let reader = common::LockHolder::start_named(&store, "remove_reader", "reader");
    let result = remove::remove(
        &store.root,
        RemoveInput {
            source_key: "source".into(),
        },
        Duration::from_millis(30),
    );
    assert_eq!(result.unwrap_err().code(), "conflict");
    assert_eq!(state(&store), before);
    assert_eq!(store.graph_files(), graph);
    drop(reader);
    let writer = common::LockHolder::start_named(&store, "remove_writer", "writer");
    let result = remove::remove(
        &store.root,
        RemoveInput {
            source_key: "source".into(),
        },
        Duration::from_millis(30),
    );
    assert_eq!(result.unwrap_err().code(), "conflict");
    assert_eq!(state(&store), before);
    drop(writer);
    store.success(&["remove", "--source-key", "source"]);
}

#[test]
#[ignore = "child helper invoked by publication lease test"]
fn remove_reader() {
    let root = std::path::PathBuf::from(std::env::var_os("COMMONPLACE_TEST_STORE").unwrap());
    let _reader = GraphRuntime::open(&root).unwrap();
    std::fs::write(
        std::env::var_os("COMMONPLACE_TEST_READY").unwrap(),
        b"ready",
    )
    .unwrap();
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

#[test]
#[ignore = "child helper invoked by writer contention test"]
fn remove_writer() {
    common::hold_writer_lock();
}
