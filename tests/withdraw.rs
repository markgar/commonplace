mod common;

use std::time::Duration;

use common::Store;
use commonplace::Result;
use commonplace::app::{ingest, withdraw};
use commonplace::domain::documents::DocumentInput;
use commonplace::domain::withdraw::WithdrawInput;
use commonplace::graph::GraphRuntime;
use commonplace::providers::embeddings::{
    EMBEDDING_DIMENSIONS, EMBEDDING_IDENTITY, EmbeddingModel,
};
use commonplace::storage::database::SqliteDatabase;
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

fn evidence(store: &Store, text: &str) -> Value {
    let result = ingest::ingest(
        &store.root,
        [ingest::InputItem {
            input: "notes".into(),
            source_key: Some("notes".into()),
            document: Ok(DocumentInput {
                source_key: "notes".into(),
                text: text.into(),
                title: None,
                source_type: "test".into(),
                occurred_at: None,
                temporal_state: commonplace::domain::documents::TemporalState::Unknown,
                metadata: Default::default(),
            }),
        }]
        .into_iter(),
        &mut Model,
        &ingest::OperationConfig::default(),
    )
    .unwrap();
    assert_eq!(result.summary.failed, 0);
    serde_json::to_value(result).unwrap()["items"][0]["passage_ids"][0].clone()
}

fn record(store: &Store, items: Value) -> Value {
    let path = store.input(&json!({"created_by":"author","items":items}));
    store.success(&["record", path.to_str().unwrap()])["result"].clone()
}

fn withdraw(store: &Store, ids: Value) -> Value {
    let path = store.input(&json!({"withdrawn_by":"reviewer","knowledge_ids":ids}));
    store.success(&["withdraw", path.to_str().unwrap(), "--json"])["result"].clone()
}

fn reject(store: &Store, input: Value, code: &str, exit: i32) -> Value {
    let path = store.input(&input);
    store.failure(&["withdraw", path.to_str().unwrap()], code, exit)
}

fn get(store: &Store, id: &str) -> Value {
    store.success(&["get", id])["result"].clone()
}

fn graph(store: &Store) -> Value {
    store.success(&[
        "graph",
        "query",
        "SELECT ?s ?p ?o WHERE {?s ?p ?o} ORDER BY ?s ?p ?o",
    ])["result"]
        .clone()
}

fn fixture() -> Store {
    let store = Store::new();
    store.apply(&Store::vocabulary(), false);
    let old = evidence(&store, "Exact old café evidence.");
    let new = evidence(&store, "Exact current evidence.");
    record(
        &store,
        json!([
            {"kind":"entity","ref":"p","name":"Person"},
            {"kind":"entity","ref":"c","name":"Company"},
            {"kind":"type_membership","entity":{"ref":"p"},"entity_type":"person","support":[{"passage_id":old}]},
            {"kind":"type_membership","entity":{"ref":"c"},"entity_type":"company"},
            {"kind":"fact","subject":{"ref":"p"},"predicate":"works_at","object":{"entity":{"ref":"c"}},"support":[{"passage_id":old},{"passage_id":new}]},
            {"kind":"fact","subject":{"ref":"p"},"predicate":"note","object":{"literal":"approved"},"support":[{"passage_id":old}]},
            {"kind":"fact","subject":{"ref":"p"},"predicate":"age","object":{"literal":i64::MIN}},
            {"kind":"fact","subject":{"ref":"p"},"predicate":"active","object":{"literal":true}},
            {"kind":"fact","subject":{"ref":"p"},"predicate":"born","object":{"literal":"2026-09-28T08:00:00-05:00"}}
        ]),
    );
    store
}

fn source_state(store: &Store) -> Value {
    let read = SqliteDatabase::read(&store.root).unwrap();
    let db = read.connection();
    let ids = |sql: &str| -> Vec<i64> {
        db.prepare(sql)
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    json!({
        "document":get(store,"doc:1"),
        "old":get(store,"revision:1"),
        "current":get(store,"revision:2"),
        "fts":ids("SELECT rowid FROM passage_fts ORDER BY rowid"),
        "vectors":ids("SELECT passage_id FROM passage_vectors ORDER BY passage_id")
    })
}

#[test]
fn retained_subtypes_literals_evidence_provenance_and_active_graph_survive_reopen() {
    let store = fixture();
    let sources = source_state(&store);
    let untouched = get(&store, "knowledge:2");
    let before: Vec<_> = (1..=7)
        .map(|id| get(&store, &format!("knowledge:{id}")))
        .collect();
    let duplicate = record(
        &store,
        json!([
            {"kind":"fact","subject":{"id":"entity:1"},"predicate":"works_at","object":{"entity":{"id":"entity:2"}}}
        ]),
    );
    let duplicate_id = duplicate["items"][0]["knowledge_id"].as_str().unwrap();
    let result = withdraw(
        &store,
        json!([
            "knowledge:3",
            "knowledge:4",
            "knowledge:5",
            "knowledge:6",
            "knowledge:7"
        ]),
    );
    assert_eq!(result["summary"], json!({"items":5,"withdrawn":5}));
    assert_eq!(result["knowledge_version"], 3);
    let timestamp = result["items"][0]["withdrawn_at"].clone();
    assert!(timestamp.as_str().unwrap().ends_with('Z'));
    for (index, prior) in before.iter().skip(2).enumerate() {
        let mut expected = prior.clone();
        expected["withdrawn_at"] = timestamp.clone();
        expected["withdrawn_by"] = json!("reviewer");
        assert_eq!(
            get(&store, prior["knowledge_id"].as_str().unwrap()),
            expected
        );
        expected.as_object_mut().unwrap().remove("kind");
        expected["index"] = json!(index);
        assert_eq!(result["items"][index], expected);
    }
    assert_eq!(
        get(&store, "entity:1")["active_fact_ids"],
        json!([duplicate_id])
    );
    assert_eq!(
        get(&store, "entity:2")["active_fact_ids"],
        json!([duplicate_id])
    );
    assert_eq!(get(&store, "knowledge:2"), untouched);
    assert_eq!(source_state(&store), sources);
    let active = graph(&store);
    let serialized = active.to_string();
    for id in 3..=7 {
        assert!(!serialized.contains(&format!("urn:commonplace:knowledge:{id}\"")));
    }
    assert!(serialized.contains(&format!("urn:commonplace:{duplicate_id}")));
    let last = withdraw(&store, json!(["knowledge:1", duplicate_id]));
    assert_eq!(last["knowledge_version"], 4);
    assert_eq!(last["items"][0]["subtype"], "type_membership");
    assert_eq!(last["items"][0]["support"], before[0]["support"]);
    assert_eq!(get(&store, "entity:1")["active_type_ids"], json!([]));
    assert_eq!(
        get(&store, "entity:1")["active_type_membership_ids"],
        json!([])
    );
    assert_eq!(get(&store, "entity:1")["active_fact_ids"], json!([]));
    let remaining = graph(&store);
    assert!(!remaining.to_string().contains("urn:commonplace:entity:1"));
    assert!(!remaining.to_string().contains("urn:commonplace:predicate:"));
    assert_eq!(
        store.success(&["graph", "rebuild"])["result"]["knowledge_version"],
        4
    );
    assert_eq!(store.success(&["init"])["status"], "unchanged");
    assert_eq!(graph(&store), remaining);
    assert_eq!(get(&store, "knowledge:3")["object"], before[2]["object"]);
    assert_eq!(source_state(&store), sources);
}

#[test]
fn complete_batch_checks_both_endpoint_directions_and_allowed_alternatives() {
    for object in [false, true] {
        for reverse in [false, true] {
            let store = fixture();
            let membership = if object { "knowledge:2" } else { "knowledge:1" };
            let before = graph(&store);
            let error = reject(
                &store,
                json!({"knowledge_ids":[membership]}),
                "invalid_input",
                2,
            );
            assert!(
                error["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("knowledge:3")
            );
            assert_eq!(graph(&store), before);
            assert_eq!(get(&store, membership)["withdrawn_at"], Value::Null);
            let mut ids = if object {
                vec![membership, "knowledge:3"]
            } else {
                vec![
                    membership,
                    "knowledge:3",
                    "knowledge:4",
                    "knowledge:5",
                    "knowledge:6",
                    "knowledge:7",
                ]
            };
            if reverse {
                ids.reverse();
            }
            let result = withdraw(&store, json!(ids));
            assert_eq!(result["knowledge_version"], 2);
            for (index, id) in ids.iter().enumerate() {
                assert_eq!(result["items"][index]["knowledge_id"], *id);
            }
        }
    }
    for same_type in [false, true] {
        let store = fixture();
        if !same_type {
            store.apply(&json!({
                "entity_types":[{"name":"alternate"}],
                "predicates":[
                    {"name":"works_at","object_kind":"entity","subject_types":["alternate"],"object_types":["alternate"]},
                    {"name":"age","object_kind":"integer","subject_types":["alternate"]},
                    {"name":"active","object_kind":"boolean","subject_types":["alternate"]},
                    {"name":"born","object_kind":"timestamp","subject_types":["alternate"]},
                    {"name":"note","object_kind":"string","subject_types":["alternate"]}
                ]
            }), false);
        }
        record(
            &store,
            json!([
                {"kind":"type_membership","entity":{"id":"entity:1"},"entity_type":if same_type {"person"} else {"alternate"}},
                {"kind":"type_membership","entity":{"id":"entity:2"},"entity_type":if same_type {"company"} else {"alternate"}}
            ]),
        );
        let fact = get(&store, "knowledge:3");
        withdraw(&store, json!(["knowledge:1", "knowledge:2"]));
        assert_eq!(get(&store, "knowledge:3"), fact);
        assert_eq!(
            get(&store, "entity:1")["active_fact_ids"]
                .as_array()
                .unwrap()
                .len(),
            5
        );
        assert_eq!(
            get(&store, "entity:2")["active_fact_ids"],
            json!(["knowledge:3"])
        );
        store.success(&["graph", "rebuild"]);
    }
    let store = fixture();
    withdraw(&store, json!(["knowledge:3"]));
    let before = graph(&store);
    let error = reject(
        &store,
        json!({"knowledge_ids":["knowledge:1"]}),
        "invalid_input",
        2,
    );
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("knowledge:4")
    );
    assert_eq!(graph(&store), before);
    assert_eq!(get(&store, "knowledge:1")["withdrawn_at"], Value::Null);
}

#[test]
fn invalid_late_ids_and_already_withdrawn_reject_before_any_mutation() {
    let store = fixture();
    withdraw(&store, json!(["knowledge:7"]));
    // Even a trigger on the first valid ID must not run before complete preflight.
    store.database().execute_batch("CREATE TRIGGER no_update BEFORE UPDATE ON knowledge_items BEGIN SELECT RAISE(ABORT,'mutation before validation'); END;").unwrap();
    let before = graph(&store);
    for (id, code) in [
        ("knowledge:999", "not_found"),
        ("knowledge:7", "invalid_input"),
    ] {
        let error = reject(&store, json!({"knowledge_ids":["knowledge:3",id]}), code, 2);
        assert!(
            error["error"]["message"]
                .as_str()
                .unwrap()
                .contains("knowledge_ids[1]")
        );
        assert_eq!(get(&store, "knowledge:3")["withdrawn_at"], Value::Null);
        assert_eq!(graph(&store), before);
    }
    reject(
        &store,
        json!({"knowledge_ids":["knowledge:1"]}),
        "invalid_input",
        2,
    );
}

#[test]
fn descriptions_and_input_limits_reject_invalid_requests_without_store() {
    let store = Store::new();
    let missing = store.directory.path().join("missing");
    let output =
        common::isolated_command(env!("CARGO_BIN_EXE_commonplace"), store.directory.path())
            .arg("--store")
            .arg(&missing)
            .args(["withdraw", "--describe", "--json"])
            .output()
            .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(!missing.exists());
    let description: Value = serde_json::from_slice(&output.stdout).unwrap();
    let validator = jsonschema::validator_for(&description["result"]["input_schema"]).unwrap();
    assert!(validator.is_valid(&description["result"]["example"]));
    assert_eq!(description["operation"], "withdraw.describe");
    let example = &description["result"]["output_example"];
    assert_eq!(example["operation"], "withdraw");
    assert_eq!(example["result"]["items"][0]["support"], json!([]));
    for request in [
        json!({"knowledge_ids":[]}),
        json!({"knowledge_ids":["knowledge:1","knowledge:1"]}),
        json!({"knowledge_ids":["knowledge:1"],"withdrawn_by":null}),
        json!({"knowledge_ids":["knowledge:1"],"withdrawn_by":""}),
        json!({"knowledge_ids":["knowledge:1"],"withdrawn_by":"é".repeat(129)}),
        json!({"knowledge_ids":["knowledge:1"],"unknown":true}),
        json!({"knowledge_ids":(1..=1001).map(|id|format!("knowledge:{id}")).collect::<Vec<_>>()}),
    ] {
        reject(&store, request, "invalid_input", 2);
    }
    for id in [
        "entity:1",
        "knowledge:0",
        "knowledge:-1",
        "knowledge:+1",
        "knowledge:01",
        "knowledge:1\n",
        "knowledge:9223372036854775808",
    ] {
        reject(&store, json!({"knowledge_ids":[id]}), "invalid_input", 2);
    }
    reject(
        &store,
        json!({"knowledge_ids":["knowledge:9223372036854775807"],"withdrawn_by":"é".repeat(128)}),
        "not_found",
        2,
    );
    for raw in [
        r#"{"knowledge_ids":["knowledge:1"],"knowledge_ids":["knowledge:2"]}"#,
        r#"{"knowledge_ids":["knowledge:1"],"withdrawn_by":"a","withdrawn_by":"b"}"#,
        "",
        "[]",
        "\u{feff}{\"knowledge_ids\":[\"knowledge:1\"]}",
    ] {
        let path = store.directory.path().join("input.json");
        std::fs::write(&path, raw).unwrap();
        store.failure(&["withdraw", path.to_str().unwrap()], "invalid_input", 2);
    }
    let mut bytes = br#"{"knowledge_ids":["knowledge:1"]}"#.to_vec();
    bytes.resize(1048576, b' ');
    let path = store.directory.path().join("input.json");
    std::fs::write(&path, &bytes).unwrap();
    store.failure(&["withdraw", path.to_str().unwrap()], "not_found", 2);
    bytes.push(b' ');
    std::fs::write(&path, &bytes).unwrap();
    store.failure(&["withdraw", path.to_str().unwrap()], "limit_exceeded", 2);
    assert!(
        !store
            .run(&["withdraw", path.to_str().unwrap(), "--describe"])
            .status
            .success()
    );
    assert!(!store.run(&["withdraw"]).status.success());
}

#[test]
fn maximum_batch_is_atomic_and_omitted_label_is_null() {
    let store = Store::new();
    store.apply(&json!({"entity_types":[{"name":"person"}]}), false);
    record(&store, json!([{"kind":"entity","name":"P"}]));
    record(
        &store,
        json!(
            (0..1000)
                .map(|_| json!({
                    "kind":"type_membership","entity":{"id":"entity:1"},"entity_type":"person"
                }))
                .collect::<Vec<_>>()
        ),
    );
    let ids: Vec<_> = (1..=1000)
        .rev()
        .map(|id| format!("knowledge:{id}"))
        .collect();
    let path = store.input(&json!({"knowledge_ids":ids}));
    let result = store.success(&["withdraw", path.to_str().unwrap()]);
    assert_eq!(
        result["result"]["summary"],
        json!({"items":1000,"withdrawn":1000})
    );
    assert_eq!(result["result"]["knowledge_version"], 2);
    let items = result["result"]["items"].as_array().unwrap();
    for (index, item) in items.iter().enumerate() {
        assert_eq!(item["knowledge_id"], ids[index]);
        assert_eq!(item["index"], index);
        assert_eq!(item["withdrawn_by"], Value::Null);
        assert_eq!(item["withdrawn_at"], items[0]["withdrawn_at"]);
    }
    assert_eq!(graph(&store)["rows"], json!([]));
}

#[test]
fn source_removal_before_and_after_withdrawal_preserves_lifecycle() {
    for remove_first in [false, true] {
        let store = fixture();
        if remove_first {
            store.success(&["remove", "--source-key", "notes"]);
        }
        let prior = get(&store, "knowledge:3");
        let result = withdraw(&store, json!(["knowledge:3", "knowledge:4"]));
        assert_eq!(result["items"][0]["support"], prior["support"]);
        if !remove_first {
            store.success(&["remove", "--source-key", "notes"]);
        }
        let retained = get(&store, "knowledge:3");
        assert_eq!(retained["support"], json!([]));
        assert_eq!(retained["withdrawn_at"], result["items"][0]["withdrawn_at"]);
        assert_eq!(retained["withdrawn_by"], "reviewer");
        assert_eq!(retained["object"], prior["object"]);
        let before = graph(&store);
        store.success(&["graph", "rebuild"]);
        assert_eq!(graph(&store), before);
        assert_eq!(get(&store, "knowledge:3"), retained);
    }
}

#[test]
fn late_storage_graph_and_corrupt_history_failures_preserve_lifecycle() {
    for failure in [
        "update", "version", "overflow", "graph", "subtype", "literal", "evidence",
    ] {
        let store = fixture();
        match failure {
            "update" => store.database().execute_batch("CREATE TRIGGER reject_late BEFORE UPDATE ON knowledge_items WHEN OLD.knowledge_item_id=4 BEGIN SELECT RAISE(ABORT,'late update'); END;").unwrap(),
            "version" => store.database().execute_batch("CREATE TRIGGER reject_version BEFORE UPDATE ON store_state BEGIN SELECT RAISE(ABORT,'late version'); END;").unwrap(),
            "overflow" => {
                store.database().execute_batch("UPDATE store_state SET knowledge_version=9223372036854775807").unwrap();
                store.success(&["graph","rebuild"]);
            }
            "graph" => std::fs::write(store.root.join("graph/candidate"),"leftover").unwrap(),
            "subtype" => store.database().execute_batch("DELETE FROM facts WHERE knowledge_item_id=4").unwrap(),
            "literal" => store.database().execute_batch("UPDATE facts SET literal_json='  \"approved\"' WHERE knowledge_item_id=4").unwrap(),
            "evidence" => store.database().execute_batch("UPDATE passages SET end_byte=999 WHERE passage_id=1").unwrap(),
            _ => unreachable!(),
        }
        let before = store.graph_files();
        reject(
            &store,
            json!({"knowledge_ids":["knowledge:3","knowledge:4"]}),
            if failure == "graph" {
                "graph_unavailable"
            } else {
                "internal_error"
            },
            1,
        );
        assert_eq!(
            store
                .database()
                .query_row(
                    "SELECT count(*) FROM knowledge_items WHERE withdrawn_at IS NOT NULL",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        assert_eq!(store.graph_files(), before, "{failure}");
    }
}

#[test]
fn reader_and_writer_leases_prevent_withdrawal_without_partial_changes() {
    let store = fixture();
    let before = get(&store, "knowledge:3");
    let graph = store.graph_files();
    for helper in ["withdraw_reader", "withdraw_writer"] {
        let holder = common::LockHolder::start_named(&store, helper, helper);
        let input: WithdrawInput =
            serde_json::from_value(json!({"knowledge_ids":["knowledge:3"]})).unwrap();
        let error = withdraw::withdraw(&store.root, input, Duration::from_millis(30)).unwrap_err();
        assert_eq!(error.code(), "conflict");
        assert_eq!(get(&store, "knowledge:3"), before);
        assert_eq!(store.graph_files(), graph);
        drop(holder);
    }
    withdraw(&store, json!(["knowledge:3"]));
}

#[test]
fn public_response_delivery_failure_reports_committed_withdrawal_without_retry() {
    use std::process::Stdio;

    let store = fixture();
    let input = store.input(&json!({"knowledge_ids":["knowledge:3"]}));
    let holder = common::LockHolder::start_named(&store, "withdraw_writer", "delivery-writer");
    let mut child = store
        .command()
        .args(["withdraw", input.to_str().unwrap()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    drop(holder);
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["operation"], "withdraw");
    assert_eq!(error["error"]["code"], "internal_error");
    let message = error["error"]["message"].as_str().unwrap();
    assert!(message.contains("withdraw committed at knowledge_version 2 (knowledge:3)"));
    assert!(message.contains("Do not retry withdraw"));
    assert!(!message.contains("rebuild"));
    assert!(get(&store, "knowledge:3")["withdrawn_at"].is_string());
    GraphRuntime::open(&store.root).unwrap();
}

#[test]
#[ignore = "child helper invoked by publication lease test"]
fn withdraw_reader() {
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
fn withdraw_writer() {
    common::hold_writer_lock();
}
