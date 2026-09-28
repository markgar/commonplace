mod common;

use std::time::Duration;

use common::Store;
use commonplace::app::{ingest, record};
use commonplace::domain::documents::DocumentInput;
use commonplace::providers::embeddings::{
    EMBEDDING_DIMENSIONS, EMBEDDING_IDENTITY, EmbeddingModel,
};
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
                metadata: Default::default(),
            }),
        }]
        .into_iter(),
        &mut Model,
        &ingest::OperationConfig::default(),
    )
    .unwrap();
    serde_json::to_value(result).unwrap()["items"][0]["passage_ids"][0].clone()
}

fn record(store: &Store, input: &Value) -> Value {
    let path = store.input(input);
    store.success(&["record", path.to_str().unwrap()])
}

fn reject(store: &Store, input: &Value, code: &str, exit: i32) -> Value {
    let path = store.input(input);
    store.failure(&["record", path.to_str().unwrap()], code, exit)
}

fn count(store: &Store, table: &str) -> i64 {
    store
        .database()
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn types(store: &Store) {
    store.apply(
        &json!({"entity_types":[{"name":"person"},{"name":"lead"},{"name":"unused"}],
        "identifier_schemes":[{"name":"email"}]}),
        false,
    );
}

#[test]
fn multiple_types_exact_old_new_evidence_canonical_get_query_and_reopen() {
    let store = Store::new();
    types(&store);
    let old = evidence(&store, "Riley 🦀\r\nleads.\0");
    let new = evidence(&store, "Riley leads the team.");
    let mut exact = store.success(&["get", old.as_str().unwrap()])["result"].clone();
    exact.as_object_mut().unwrap().remove("kind");
    let result = record(
        &store,
        &json!({"created_by":"test","items":[
            {"kind":"entity","ref":"riley","name":"Riley","aliases":["R"],
                "identifiers":[{"scheme":"email","value":"r@example.test"}]},
            {"kind":"type_membership","entity":{"ref":"riley"},"entity_type":"person",
                "support":[{"passage_id":old,"quote":exact["text"],"start_byte":exact["start_byte"],"end_byte":exact["end_byte"]},{"passage_id":new}]},
            {"kind":"type_membership","entity":{"identifier":{"scheme":"email","value":"r@example.test"}},"entity_type":"lead"}
        ]}),
    );
    assert_eq!(
        result["result"]["summary"],
        json!({"items":3,"entities_created":1,"metadata_changed":0,"memberships_created":2})
    );
    assert_eq!(result["result"]["knowledge_version"], 1);
    assert_eq!(result["result"]["items"][1]["support"][0], exact);
    let entity = store.success(&["get", "entity:1"])["result"].clone();
    assert_eq!(
        entity["active_type_membership_ids"],
        json!(["knowledge:1", "knowledge:2"])
    );
    assert_eq!(entity["active_type_ids"].as_array().unwrap().len(), 2);
    assert_eq!(entity["active_fact_ids"], json!([]));
    assert_eq!(entity["aliases"][0]["alias"], "R");
    let knowledge = store.success(&["get", "knowledge:1"])["result"].clone();
    assert_eq!(knowledge["support"][0], exact);
    assert_eq!(knowledge["withdrawn_at"], Value::Null);
    assert_eq!(
        store.success(&["get", "knowledge:2"])["result"]["support"],
        json!([])
    );
    let query = "PREFIX c:<urn:commonplace:property:> SELECT ?k ?p ?r ?d ?text ?start ?end WHERE {
        ?k c:kind \"type_membership\"; c:evidence ?p.
        ?p c:revision ?r; c:text ?text; c:start_byte ?start; c:end_byte ?end.
        ?r c:document ?d } ORDER BY ?p";
    let before = store.success(&["graph", "query", query])["result"].clone();
    assert_eq!(before["rows"].as_array().unwrap().len(), 2);
    assert_eq!(
        before["columns"],
        json!(["k", "p", "r", "d", "text", "start", "end"])
    );
    assert_eq!(before["rows"][0][4]["value"], exact["text"]);
    assert_eq!(
        before["rows"][0][5]["value"],
        exact["start_byte"].to_string()
    );
    assert_eq!(before["rows"][0][6]["value"], exact["end_byte"].to_string());
    assert_eq!(
        store.success(&[
            "graph",
            "query",
            "PREFIX c:<urn:commonplace:property:> SELECT ?s WHERE {?s c:name \"unused\"}"
        ])["result"]["rows"],
        json!([])
    );
    store.success(&["graph", "rebuild"]);
    assert_eq!(store.success(&["graph", "query", query])["result"], before);
    assert_eq!(store.success(&["get", "knowledge:1"])["result"], knowledge);
    let again = record(
        &store,
        &json!({"items":[{"kind":"type_membership","entity":{"name":"R"},"entity_type":"person"}]}),
    );
    assert_eq!(again["result"]["items"][0]["knowledge_id"], "knowledge:3");
    assert_eq!(again["result"]["knowledge_version"], 2);
}

#[test]
fn bare_empty_and_metadata_only_do_not_publish_or_repair_graph() {
    let store = Store::new();
    types(&store);
    let graph = store.graph_files();
    assert_eq!(
        record(&store, &json!({"items":[]}))["result"]["knowledge_version"],
        0
    );
    record(
        &store,
        &json!({"items":[{"kind":"entity","name":"Riley","aliases":["Old"],"identifiers":[{"scheme":"email","value":"old"}]}]}),
    );
    assert_eq!(store.graph_files(), graph);
    assert_eq!(
        store.success(&["graph", "query", "SELECT ?s WHERE {?s ?p ?o}"])["result"]["rows"],
        json!([])
    );
    let change = json!({"items":[{"kind":"entity_metadata","entity_id":"entity:1",
        "remove_aliases":["Old"],"add_aliases":["New"],"remove_identifiers":[{"scheme":"email","value":"old"}],"add_identifiers":[{"scheme":"email","value":"new"}]}]});
    assert_eq!(
        record(&store, &change)["result"]["summary"]["metadata_changed"],
        1
    );
    assert_eq!(store.graph_files(), graph);
    let no_op =
        json!({"items":[{"kind":"entity_metadata","entity_id":"entity:1","add_aliases":["New"]}]});
    assert_eq!(
        record(&store, &no_op)["result"]["summary"]["metadata_changed"],
        0
    );
    std::fs::rename(
        store.root.join("graph/current"),
        store.directory.path().join("saved"),
    )
    .unwrap();
    assert_eq!(record(&store, &no_op)["result"]["knowledge_version"], 0);
    assert!(!store.root.join("graph/current").exists());
    store.success(&["graph", "rebuild"]);
    store
        .database()
        .execute("UPDATE store_state SET knowledge_version=5", [])
        .unwrap();
    let stale = store.graph_files();
    assert_eq!(record(&store, &no_op)["result"]["knowledge_version"], 5);
    assert_eq!(store.graph_files(), stale);
    store.failure(
        &["graph", "query", "SELECT ?s WHERE {?s ?p ?o}"],
        "graph_unavailable",
        1,
    );
}

#[test]
fn invalid_late_items_ambiguous_names_identifiers_and_refs_are_atomic() {
    let store = Store::new();
    types(&store);
    record(
        &store,
        &json!({"items":[
        {"kind":"entity","name":"Same","aliases":["Alias"],"identifiers":[{"scheme":"email","value":"owned"}]},
        {"kind":"entity","name":"Other","aliases":["Alias"]}]}),
    );
    let graph = store.graph_files();
    for (bad, code, exit) in [
        (
            json!({"kind":"type_membership","entity":{"name":"Alias"},"entity_type":"person"}),
            "conflict",
            3,
        ),
        (
            json!({"kind":"entity","name":"Duplicate","identifiers":[{"scheme":"email","value":"owned"}]}),
            "conflict",
            3,
        ),
        (
            json!({"kind":"type_membership","entity":{"ref":"missing"},"entity_type":"person"}),
            "invalid_input",
            2,
        ),
        (
            json!({"kind":"entity","ref":"new","name":"Duplicate ref"}),
            "invalid_input",
            2,
        ),
        (
            json!({"kind":"type_membership","entity":{"id":"entity:1"},"entity_type":"absent"}),
            "invalid_input",
            2,
        ),
        (
            json!({"kind":"entity_metadata","entity_id":"entity:1","add_aliases":["x"],"remove_aliases":["x"]}),
            "invalid_input",
            2,
        ),
        (
            json!({"kind":"entity_metadata","entity_id":"entity:2","remove_identifiers":[{"scheme":"email","value":"owned"}]}),
            "not_found",
            2,
        ),
    ] {
        let result = reject(
            &store,
            &json!({"items":[
            {"kind":"entity","ref":"new","name":"Must rollback"},
            {"kind":"type_membership","entity":{"ref":"new"},"entity_type":"person"},
            bad]}),
            code,
            exit,
        );
        assert!(
            result["error"]["message"]
                .as_str()
                .unwrap()
                .contains("items[2]")
        );
        assert_eq!(count(&store, "entities"), 2);
        assert_eq!(count(&store, "knowledge_items"), 0);
        assert_eq!(store.graph_files(), graph);
    }
    reject(
        &store,
        &json!({"items":[
        {"kind":"type_membership","entity":{"ref":"forward"},"entity_type":"person"},
        {"kind":"entity","ref":"forward","name":"Forward"}]}),
        "invalid_input",
        2,
    );
    reject(
        &store,
        &json!({"items":[{"kind":"type_membership","entity":{"ref":"new"},"entity_type":"person"}]}),
        "invalid_input",
        2,
    );
}

#[test]
fn malformed_support_rolls_back_all_changes() {
    let store = Store::new();
    types(&store);
    let id = evidence(&store, "é\r\n\0");
    for support in [
        json!([{"passage_id":id,"quote":"é\n\0"}]),
        json!([{"passage_id":id,"quote":"é"}]),
        json!([{"passage_id":id,"start_byte":1,"end_byte":5}]),
        json!([{"passage_id":id,"start_byte":0}]),
        json!([{"passage_id":id,"start_byte":0,"end_byte":4}]),
        json!([{"passage_id":id},{"passage_id":id}]),
    ] {
        reject(
            &store,
            &json!({"items":[{"kind":"entity","ref":"e","name":"E"},
            {"kind":"type_membership","entity":{"ref":"e"},"entity_type":"person","support":support}]}),
            "invalid_input",
            2,
        );
        assert_eq!(count(&store, "entities"), 0);
        assert_eq!(count(&store, "knowledge_items"), 0);
    }
    reject(
        &store,
        &json!({"items":[{"kind":"entity","ref":"e","name":"E"},
        {"kind":"type_membership","entity":{"ref":"e"},"entity_type":"person","support":[{"passage_id":"passage:999"}]}]}),
        "not_found",
        2,
    );
    assert_eq!(count(&store, "entities"), 0);
}

#[test]
fn descriptions_and_execution_reject_unsupported_unknown_duplicate_and_bad_shapes() {
    let store = Store::new();
    types(&store);
    let description = store.success(&["record", "--describe"]);
    let schema = &description["result"]["input_schema"];
    let validator = jsonschema::validator_for(schema).unwrap();
    assert!(validator.is_valid(&description["result"]["example"]));
    assert_eq!(description["result"]["vocabulary"]["schema_version"], 1);
    for input in [
        json!({"items":[{"kind":"fact"}]}),
        json!({"items":[{"kind":"entity","name":"E","unknown":1}]}),
        json!({"items":[{"kind":"type_membership","entity":{"id":"entity:1","name":"E"},"entity_type":"person"}]}),
        json!({"items":[{"kind":"entity","name":""}]}),
        json!({"items":[{"kind":"entity","name":"nul\0"}]}),
        json!({"items":[{"kind":"entity","ref":"bad-ref","name":"E"}]}),
        json!({"items":[{"kind":"entity","name":"a".repeat(1025)}]}),
        json!({"created_by":"x".repeat(129),"items":[]}),
        json!({"items":vec![json!({"kind":"entity","name":"E"});1001]}),
    ] {
        assert!(!validator.is_valid(&input), "{input}");
        reject(&store, &input, "invalid_input", 2);
    }
    let path = store.directory.path().join("duplicate.json");
    for raw in [
        r#"{"items":[],"items":[]}"#,
        r#"{"items":[{"kind":"entity","name":"first","name":"second"}]}"#,
        r#"{"items":[{"kind":"type_membership","entity":{"id":"entity:1","id":"entity:2"},"entity_type":"person"}]}"#,
    ] {
        std::fs::write(&path, raw).unwrap();
        store.failure(&["record", path.to_str().unwrap()], "invalid_input", 2);
    }
    std::fs::write(&path, vec![b' '; 1024 * 1024 + 1]).unwrap();
    store.failure(&["record", path.to_str().unwrap()], "limit_exceeded", 2);
    record(
        &store,
        &json!({"items":[{"kind":"entity","name":"é".repeat(1024)}]}),
    );
    assert_eq!(count(&store, "entities"), 1);
}

#[test]
fn withdrawn_memberships_remain_readable_and_bad_subtypes_fail_closed() {
    let store = Store::new();
    types(&store);
    record(
        &store,
        &json!({"items":[{"kind":"entity","ref":"e","name":"E"},
        {"kind":"type_membership","entity":{"ref":"e"},"entity_type":"person"}]}),
    );
    store
        .database()
        .execute(
            "UPDATE knowledge_items SET withdrawn_at='2026-01-01T00:00:00Z',withdrawn_by='test'",
            [],
        )
        .unwrap();
    assert_eq!(
        store.success(&["get", "knowledge:1"])["result"]["withdrawn_by"],
        "test"
    );
    assert_eq!(
        store.success(&["get", "entity:1"])["result"]["active_type_ids"],
        json!([])
    );
    store
        .database()
        .execute("DELETE FROM entity_type_memberships", [])
        .unwrap();
    store.failure(&["get", "knowledge:1"], "internal_error", 1);
    store.failure(&["get", "entity:999"], "not_found", 2);
    store.failure(&["get", "knowledge:999"], "not_found", 2);
}

#[test]
fn readers_and_writer_processes_exclude_authoring_publication() {
    let store = Store::new();
    types(&store);
    let request = json!({"items":[{"kind":"entity","ref":"e","name":"E"},
        {"kind":"type_membership","entity":{"ref":"e"},"entity_type":"person"}]});
    let first = common::LockHolder::start_named(&store, "record_reader", "reader1");
    let second = common::LockHolder::start_named(&store, "record_reader", "reader2");
    let graph = store.graph_files();
    let error = record::record(
        &store.root,
        serde_json::from_value(request.clone()).unwrap(),
        Duration::from_millis(30),
    )
    .unwrap_err();
    assert_eq!(error.code(), "conflict");
    assert_eq!(count(&store, "entities"), 0);
    assert_eq!(store.graph_files(), graph);
    drop(first);
    drop(second);
    let writer = common::LockHolder::start_named(&store, "record_writer", "writer");
    let error = record::record(
        &store.root,
        serde_json::from_value(request.clone()).unwrap(),
        Duration::from_millis(30),
    )
    .unwrap_err();
    assert_eq!(error.code(), "conflict");
    drop(writer);
    record(&store, &request);
    assert_eq!(count(&store, "knowledge_items"), 1);
}

#[test]
#[ignore = "child helper invoked by reader/publication test"]
fn record_reader() {
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
fn record_writer() {
    common::hold_writer_lock();
}

#[test]
fn leftover_scratch_rejects_authoring_until_explicit_rebuild() {
    let store = Store::new();
    types(&store);
    let scratch = store.root.join("graph/previous");
    std::fs::create_dir(&scratch).unwrap();
    std::fs::write(scratch.join("marker"), b"keep").unwrap();
    let before = store.graph_files();
    let request = json!({"items":[{"kind":"entity","ref":"e","name":"E"},
        {"kind":"type_membership","entity":{"ref":"e"},"entity_type":"person"}]});
    reject(&store, &request, "graph_unavailable", 1);
    assert_eq!(count(&store, "entities"), 0);
    assert_eq!(store.graph_files(), before);
    store.success(&["graph", "rebuild"]);
    record(&store, &request);
    let session = SqliteDatabase::read(&store.root).unwrap();
    assert_eq!(
        session
            .connection()
            .query_row("SELECT knowledge_version FROM store_state", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn exact_input_limits_and_complete_cumulative_entity_reads() {
    let store = Store::new();
    types(&store);
    let description = store.success(&["record", "--describe"]);
    let validator = jsonschema::validator_for(&description["result"]["input_schema"]).unwrap();
    for length in [999, 1000, 1001] {
        for input in [
            json!({"items":(0..length).map(|_|json!({"kind":"entity","name":"E"})).collect::<Vec<_>>()}),
            json!({"items":[{"kind":"entity","name":"E","aliases":(0..length).map(|i|format!("a{i}")).collect::<Vec<_>>()}]}),
            json!({"items":[{"kind":"entity","name":"E","identifiers":(0..length).map(|i|json!({"scheme":"email","value":i.to_string()})).collect::<Vec<_>>()}]}),
            json!({"items":[{"kind":"type_membership","entity":{"id":"entity:1"},"entity_type":"person",
                "support":(1..=length).map(|i|json!({"passage_id":format!("passage:{i}")})).collect::<Vec<_>>()}]}),
        ] {
            assert_eq!(validator.is_valid(&input), length <= 1000);
            let typed: commonplace::domain::knowledge::RecordInput =
                serde_json::from_value(input).unwrap();
            assert_eq!(typed.validate().is_ok(), length <= 1000);
        }
    }
    for length in [127, 128, 129] {
        assert_eq!(
            validator.is_valid(&json!({"items":[],"created_by":"é".repeat(length)})),
            length <= 128
        );
    }
    let path = store.directory.path().join("limit.json");
    let mut bytes = br#"{"items":[]}"#.to_vec();
    bytes.resize(1024 * 1024, b' ');
    std::fs::write(&path, &bytes).unwrap();
    store.success(&["record", path.to_str().unwrap()]);
    bytes.push(b' ');
    std::fs::write(&path, bytes).unwrap();
    store.failure(&["record", path.to_str().unwrap()], "limit_exceeded", 2);

    record(
        &store,
        &json!({"items":[{"kind":"entity","name":"All types"}]}),
    );
    let input = json!({"items":(0..1000).map(|_|json!({
        "kind":"type_membership","entity":{"id":"entity:1"},"entity_type":"person"
    })).collect::<Vec<_>>()});
    let full = record(&store, &input);
    assert_eq!(full["result"]["items"].as_array().unwrap().len(), 1000);
    record(
        &store,
        &json!({"items":[{"kind":"type_membership","entity":{"id":"entity:1"},"entity_type":"person"}]}),
    );
    let entity = store.success(&["get", "entity:1"]);
    assert_eq!(
        entity["result"]["active_type_membership_ids"]
            .as_array()
            .unwrap()
            .len(),
        1001
    );
    assert_eq!(
        entity["result"]["active_type_ids"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        entity["result"]["active_type_membership_ids"][1000],
        "knowledge:1001"
    );
}

#[test]
fn metadata_noop_preserves_provenance_and_membership_schema_version() {
    let store = Store::new();
    types(&store);
    record(
        &store,
        &json!({"created_by":"first","items":[{"kind":"entity","ref":"r","name":"R",
        "aliases":["R"],"identifiers":[{"scheme":"email","value":"r"}]},
        {"kind":"type_membership","entity":{"ref":"r"},"entity_type":"person"}]}),
    );
    let before = store.success(&["get", "entity:1"]);
    let graph = store.graph_files();
    record(
        &store,
        &json!({"created_by":"second","items":[{"kind":"entity_metadata","entity_id":"entity:1",
        "add_aliases":["R"],"add_identifiers":[{"scheme":"email","value":"r"}]}]}),
    );
    assert_eq!(store.success(&["get", "entity:1"]), before);
    assert_eq!(store.graph_files(), graph);
    store.apply(&json!({"entity_types":[{"name":"later"}]}), false);
    record(
        &store,
        &json!({"items":[{"kind":"type_membership","entity":{"name":"R"},"entity_type":"later"}]}),
    );
    assert_eq!(
        store.success(&["get", "knowledge:1"])["result"]["schema_version"],
        1
    );
    assert_eq!(
        store.success(&["get", "knowledge:2"])["result"]["schema_version"],
        2
    );
}
