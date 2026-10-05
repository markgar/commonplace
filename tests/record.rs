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
                temporal_state: commonplace::domain::documents::TemporalState::Unknown,
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
        json!({"items":3,"entities_created":1,"metadata_changed":0,"memberships_created":2,"facts_created":0})
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
    fact_types(&store);
    let request = json!({"items":[{"kind":"entity","ref":"e","name":"E"},
        {"kind":"type_membership","entity":{"ref":"e"},"entity_type":"person"},
        {"kind":"fact","subject":{"ref":"e"},"predicate":"decision","object":{"literal":"yes"}}]});
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
    assert_eq!(count(&store, "knowledge_items"), 2);
}

fn fact_types(store: &Store) {
    types(store);
    store.apply(&json!({"predicates":[
        {"name":"works_with","object_kind":"entity","subject_types":["person"],"object_types":["lead"]},
        {"name":"decision","object_kind":"string","subject_types":["person"]},
        {"name":"count","object_kind":"integer","subject_types":["person"]},
        {"name":"enabled","object_kind":"boolean","subject_types":["person"]},
        {"name":"decided_at","object_kind":"timestamp","subject_types":["person"]},
        {"name":"unused_predicate","object_kind":"string","subject_types":["person"]}
    ]}), false);
}

fn literal_fact(predicate: &str, literal: Value) -> Value {
    json!({"kind":"fact","subject":{"id":"entity:1"},"predicate":predicate,
        "object":{"literal":literal}})
}

#[test]
fn relationships_literals_resulting_types_exact_evidence_and_fact_only_publication() {
    let store = Store::new();
    fact_types(&store);
    let old = evidence(&store, "Riley 🦀\r\napproved.\0");
    let new = evidence(&store, "Riley approved the decision.");
    let exact = store.success(&["get", old.as_str().unwrap()])["result"].clone();
    let support = json!([
        {"passage_id":old,"quote":exact["text"],"start_byte":exact["start_byte"],"end_byte":exact["end_byte"]},
        {"passage_id":new}
    ]);
    let authored = record(
        &store,
        &json!({"created_by":"manual","items":[
            {"kind":"entity","ref":"riley","name":"Riley","aliases":["R"]},
            {"kind":"entity","ref":"lead","name":"Leader","identifiers":[{"scheme":"email","value":"lead@example.test"}]},
            {"kind":"fact","subject":{"ref":"riley"},"predicate":"works_with",
                "object":{"entity":{"ref":"lead"}},"support":support},
            {"kind":"fact","subject":{"ref":"riley"},"predicate":"decision",
                "object":{"literal":"approved"},"support":support},
            {"kind":"type_membership","entity":{"ref":"riley"},"entity_type":"person"},
            {"kind":"type_membership","entity":{"ref":"lead"},"entity_type":"lead"},
            {"kind":"type_membership","entity":{"ref":"riley"},"entity_type":"lead"}
        ]}),
    );
    let result = &authored["result"];
    assert_eq!(result["knowledge_version"], 1);
    assert_eq!(
        result["summary"],
        json!({"items":7,"entities_created":2,
        "metadata_changed":0,"memberships_created":3,"facts_created":2})
    );
    assert_eq!(
        result["items"][2]["object"],
        json!({"entity_id":"entity:2"})
    );
    assert_eq!(
        result["items"][3]["object"],
        json!({
        "literal_kind":"string","literal":"approved","literal_json":"\"approved\""})
    );
    assert_eq!(result["items"][2]["support"].as_array().unwrap().len(), 2);
    assert_eq!(result["items"][2]["support"][0]["text"], exact["text"]);
    for id in ["knowledge:1", "knowledge:2"] {
        let got = store.success(&["get", id])["result"].clone();
        assert_eq!(got["subtype"], "fact");
        assert_eq!(got["schema_version"], 2);
        assert_eq!(got["created_by"], "manual");
        assert_eq!(got["support"], result["items"][2]["support"]);
    }
    assert_eq!(
        store.success(&["get", "entity:2"])["result"]["active_fact_ids"],
        json!(["knowledge:1"])
    );
    let count_query = "PREFIX c:<urn:commonplace:property:> SELECT (COUNT(?k) AS ?n) WHERE { ?k c:kind \"fact\" }";
    assert_eq!(
        store.success(&["graph", "query", count_query])["result"]["rows"][0][0]["value"],
        "2"
    );
    let citation_query = "PREFIX c:<urn:commonplace:property:> SELECT ?k ?p ?r ?d ?text ?start ?end WHERE {
        ?k c:kind \"fact\"; c:evidence ?p . ?p c:revision ?r; c:text ?text; c:start_byte ?start; c:end_byte ?end .
        ?r c:document ?d } ORDER BY ?k ?p";
    let citations = store.success(&["graph", "query", citation_query])["result"].clone();
    assert_eq!(citations["rows"].as_array().unwrap().len(), 4);
    assert_eq!(citations["rows"][0][4]["value"], exact["text"]);
    assert_eq!(
        citations["rows"][0][5]["value"],
        exact["start_byte"].to_string()
    );
    assert_eq!(
        citations["rows"][0][6]["value"],
        exact["end_byte"].to_string()
    );

    let followup = json!({"items":[
        literal_fact("count", json!(i64::MIN)), literal_fact("count", json!(i64::MAX)),
        literal_fact("enabled", json!(true)), literal_fact("enabled", json!(false)),
        literal_fact("decided_at", json!("2026-09-28T12:25:23.123-05:00")),
        literal_fact("decision", json!("")), literal_fact("decision", json!("é\0\n\"\\"))
    ]});
    let more = record(&store, &followup);
    assert_eq!(more["result"]["knowledge_version"], 2);
    assert_eq!(more["result"]["summary"]["memberships_created"], 0);
    assert_eq!(more["result"]["summary"]["facts_created"], 7);
    assert_eq!(
        more["result"]["items"][4]["object"]["literal"],
        "2026-09-28T17:25:23.123Z"
    );
    assert_eq!(
        more["result"]["items"][0]["object"]["literal_json"],
        i64::MIN.to_string()
    );
    assert_eq!(
        more["result"]["items"][1]["object"]["literal"],
        json!(i64::MAX)
    );
    for item in more["result"]["items"].as_array().unwrap() {
        let id = item["knowledge_id"].as_str().unwrap();
        let got = store.success(&["get", id])["result"].clone();
        assert_eq!(got["object"], item["object"]);
        assert_eq!(got["support"], json!([]));
        let query = format!(
            "PREFIX c:<urn:commonplace:property:> SELECT ?object ?json ?kind WHERE {{
            <urn:commonplace:{id}> c:object ?object; c:literal_json ?json; c:literal_kind ?kind }}"
        );
        let result = store.success(&["graph", "query", &query]);
        let row = &result["result"]["rows"][0];
        assert_eq!(row[1]["value"], item["object"]["literal_json"]);
        assert_eq!(row[2]["value"], item["object"]["literal_kind"]);
        let kind = item["object"]["literal_kind"].as_str().unwrap();
        let datatype = if kind == "timestamp" {
            "dateTime"
        } else {
            kind
        };
        assert_eq!(
            row[0]["datatype"],
            format!("http://www.w3.org/2001/XMLSchema#{datatype}")
        );
        let literal = &item["object"]["literal"];
        assert_eq!(
            row[0]["value"],
            literal
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| literal.to_string())
        );
    }
    assert_eq!(
        store.success(&["graph", "query", count_query])["result"]["rows"][0][0]["value"],
        "9"
    );
    let repeated = record(&store, &followup);
    assert_ne!(
        repeated["result"]["items"][0]["knowledge_id"],
        more["result"]["items"][0]["knowledge_id"]
    );
    assert_eq!(repeated["result"]["knowledge_version"], 3);
    let resolved = record(
        &store,
        &json!({"items":[
            {"kind":"fact","subject":{"name":"R"},"predicate":"works_with",
                "object":{"entity":{"identifier":{"scheme":"email","value":"lead@example.test"}}}}
        ]}),
    );
    assert_eq!(
        resolved["result"]["items"][0]["subject_entity_id"],
        "entity:1"
    );
    assert_eq!(
        resolved["result"]["items"][0]["object"],
        json!({"entity_id":"entity:2"})
    );
    assert_eq!(resolved["result"]["items"][0]["support"], json!([]));
    let query = "SELECT ?s ?p ?o WHERE {?s ?p ?o} ORDER BY ?s ?p ?o";
    let before = store.success(&["graph", "query", query])["result"].clone();
    store.success(&["graph", "rebuild"]);
    assert_eq!(store.success(&["graph", "query", query])["result"], before);
    assert_eq!(
        store.success(&["graph", "query", citation_query])["result"],
        citations
    );
    assert_eq!(store.success(&["graph","query",
        "PREFIX c:<urn:commonplace:property:> SELECT ?s WHERE {?s c:name ?name FILTER(?name IN (\"unused\", \"unused_predicate\"))}"])["result"]["rows"], json!([]));
    store.apply(&json!({"entity_types":[{"name":"later"}]}), false);
    record(
        &store,
        &json!({"items":[literal_fact("decision",json!("later"))]}),
    );
    assert_eq!(
        store.success(&["get", "knowledge:1"])["result"]["schema_version"],
        2
    );
    let last: i64 = store
        .database()
        .query_row(
            "SELECT max(knowledge_item_id) FROM knowledge_items",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        store.success(&["get", &format!("knowledge:{last}")])["result"]["schema_version"],
        3
    );
    let timestamps: i64 = store.database().query_row(
        "SELECT count(DISTINCT created_at) FROM (
          SELECT created_at FROM entities UNION ALL SELECT created_at FROM knowledge_items WHERE knowledge_item_id<=5)",
        [], |r|r.get(0)).unwrap();
    assert_eq!(timestamps, 1);
}

#[test]
fn fact_validation_and_hydration_failures_roll_back_entire_request() {
    let store = Store::new();
    fact_types(&store);
    let passage = evidence(&store, "é\r\n\0");
    record(
        &store,
        &json!({"items":[{"kind":"entity","name":"Riley"},
        {"kind":"type_membership","entity":{"id":"entity:1"},"entity_type":"person"}]}),
    );
    let invalid = [
        literal_fact("unknown", json!("x")),
        literal_fact("count", json!("1")),
        literal_fact("enabled", json!("true")),
        literal_fact("decision", json!(false)),
        literal_fact("decided_at", json!("not-time")),
        literal_fact("decided_at", json!(0)),
        json!({"kind":"fact","subject":{"id":"entity:1"},"predicate":"works_with","object":{"literal":"entity:1"}}),
        json!({"kind":"fact","subject":{"id":"entity:1"},"predicate":"decision","object":{"entity":{"id":"entity:1"}}}),
        json!({"kind":"fact","subject":{"id":"entity:1"},"predicate":"works_with","object":{"entity":{"id":"entity:1"}}}),
        json!({"kind":"fact","subject":{"ref":"missing"},"predicate":"decision","object":{"literal":"x"}}),
        json!({"kind":"fact","subject":{"id":"entity:1"},"predicate":"decision","object":{"literal":"x"},
            "support":[{"passage_id":passage,"quote":"é"}]}),
        json!({"kind":"fact","subject":{"id":"entity:1"},"predicate":"decision","object":{"literal":"x"},
            "support":[{"passage_id":passage,"start_byte":1,"end_byte":5}]}),
        json!({"kind":"fact","subject":{"id":"entity:1"},"predicate":"decision","object":{"literal":"x"},
            "support":[{"passage_id":passage},{"passage_id":passage}]}),
    ];
    let graph = store.graph_files();
    reject(
        &store,
        &json!({"items":[
            {"kind":"entity","ref":"bare","name":"Bare"},
            {"kind":"fact","subject":{"ref":"bare"},"predicate":"decision","object":{"literal":"no type"}}
        ]}),
        "invalid_input",
        2,
    );
    assert_eq!(count(&store, "entities"), 1);
    for item in invalid {
        reject(
            &store,
            &json!({"items":[
                {"kind":"entity","name":"Must roll back"},
                {"kind":"entity_metadata","entity_id":"entity:1","add_aliases":["temporary"]},
                literal_fact("decision",json!("also roll back")), item
            ]}),
            "invalid_input",
            2,
        );
        assert_eq!(count(&store, "entities"), 1);
        assert_eq!(count(&store, "knowledge_items"), 1);
        assert_eq!(count(&store, "entity_aliases"), 0);
        assert_eq!(store.graph_files(), graph);
    }
    for item in [
        json!({"kind":"fact","subject":{"id":"entity:999"},"predicate":"decision","object":{"literal":"x"}}),
        json!({"kind":"fact","subject":{"id":"entity:1"},"predicate":"decision","object":{"literal":"x"},
            "support":[{"passage_id":"passage:999"}]}),
        json!({"kind":"fact","subject":{"id":"entity:1"},"predicate":"works_with","object":{"entity":{"id":"entity:999"}}}),
    ] {
        reject(
            &store,
            &json!({"items":[literal_fact("decision",json!("rollback")),item]}),
            "not_found",
            2,
        );
        assert_eq!(count(&store, "facts"), 0);
    }
    store
        .database()
        .execute(
            "UPDATE passages SET text='corrupt' WHERE passage_id=?1",
            [passage
                .as_str()
                .unwrap()
                .strip_prefix("passage:")
                .unwrap()
                .parse::<i64>()
                .unwrap()],
        )
        .unwrap();
    let item = json!({"kind":"fact","subject":{"id":"entity:1"},"predicate":"decision","object":{"literal":"x"},
        "support":[{"passage_id":passage}]});
    reject(
        &store,
        &json!({"items":[literal_fact("decision",json!("rollback")),item]}),
        "internal_error",
        1,
    );
    assert_eq!(count(&store, "facts"), 0);
    assert_eq!(store.graph_files(), graph);
    store
        .database()
        .execute(
            "UPDATE knowledge_items SET withdrawn_at='2026-01-01T00:00:00Z'",
            [],
        )
        .unwrap();
    reject(
        &store,
        &json!({"items":[literal_fact("decision",json!("no active type"))]}),
        "invalid_input",
        2,
    );
    assert_eq!(count(&store, "facts"), 0);
}

#[test]
fn exact_fact_numeric_tokens_and_generated_descriptions() {
    let store = Store::new();
    fact_types(&store);
    record(
        &store,
        &json!({"items":[{"kind":"entity","name":"Riley"},
        {"kind":"type_membership","entity":{"id":"entity:1"},"entity_type":"person"}]}),
    );
    let description = store.success(&["record", "--describe"]);
    let validator = jsonschema::validator_for(&description["result"]["input_schema"]).unwrap();
    for example in description["result"]["jsonl"]["example"]
        .as_array()
        .unwrap()
    {
        assert!(validator.is_valid(example));
    }
    let path = store.directory.path().join("tokens.json");
    for token in [
        "1.0",
        "1e0",
        "-0.0",
        "9223372036854775808",
        "-9223372036854775809",
        "18446744073709551615",
        "1e100",
        "null",
        "[]",
        "{}",
    ] {
        let raw = format!(
            r#"{{"items":[{{"kind":"fact","subject":{{"id":"entity:1"}},"predicate":"count","object":{{"literal":{token}}}}}]}}"#
        );
        std::fs::write(&path, &raw).unwrap();
        store.failure(&["record", path.to_str().unwrap()], "invalid_input", 2);
        store.failure(
            &["record", "--jsonl", path.to_str().unwrap()],
            "invalid_input",
            2,
        );
    }
    for raw in [
        r#"{"items":[{"kind":"fact","subject":{"id":"entity:1"},"predicate":"count","object":{"literal":1,"literal":2}}]}"#,
        r#"{"items":[{"kind":"fact","subject":{"id":"entity:1"},"predicate":"count","object":{"literal":1,"entity":{"id":"entity:1"}}}]}"#,
    ] {
        std::fs::write(&path, raw).unwrap();
        store.failure(&["record", path.to_str().unwrap()], "invalid_input", 2);
    }
    assert_eq!(count(&store, "facts"), 0);
    for integer in [i64::MIN, i64::MAX] {
        let input = json!({"items":[literal_fact("count",json!(integer))]});
        assert!(validator.is_valid(&input));
        record(&store, &input);
    }
    assert_eq!(count(&store, "facts"), 2);
}

#[test]
fn jsonl_fragments_are_one_request_with_global_limits_refs_and_creator() {
    let store = Store::new();
    fact_types(&store);
    let path = store.directory.path().join("request.jsonl");
    let first = json!({"created_by":"manual","items":[{"kind":"entity","ref":"r","name":"Riley"}]});
    let fact = json!({"kind":"fact","subject":{"ref":"r"},"predicate":"decision","object":{"literal":"approved"}});
    let second = json!({"created_by":"manual","items":[fact,
        {"kind":"type_membership","entity":{"ref":"r"},"entity_type":"person"}]});
    std::fs::write(&path, format!("{first}\r\n{second}\r\n")).unwrap();
    let result = store.success(&["record", "--jsonl", path.to_str().unwrap()]);
    assert_eq!(result["result"]["knowledge_version"], 1);
    assert_eq!(result["result"]["summary"]["items"], 3);
    assert_eq!(result["result"]["items"][1]["index"], 1);
    assert_eq!(
        store.success(&["get", "knowledge:1"])["result"]["created_by"],
        "manual"
    );
    let before = store.graph_files();
    for tail in [
        "",
        " ",
        "{",
        r#"{"items":[],"items":[]}"#,
        r#"{"items":[],"unknown":0}"#,
        r#"{"items":[]}"#,
        r#"{"created_by":"other","items":[]}"#,
        r#"{"created_by":"manual","items":[{"kind":"fact","subject":{"ref":"r"},"predicate":"decision","object":{"literal":false}}]}"#,
    ] {
        let raw = format!("{first}\n{tail}\n");
        std::fs::write(&path, raw).unwrap();
        let error = store.failure(
            &["record", "--jsonl", path.to_str().unwrap()],
            "invalid_input",
            2,
        );
        if tail
            != r#"{"created_by":"manual","items":[{"kind":"fact","subject":{"ref":"r"},"predicate":"decision","object":{"literal":false}}]}"#
        {
            assert!(
                error["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("line 2")
            );
        }
        assert_eq!(count(&store, "entities"), 1);
        assert_eq!(count(&store, "knowledge_items"), 2);
        assert_eq!(store.graph_files(), before);
    }
    for raw in [
        "",
        "\u{feff}{\"items\":[]}",
        "{\"items\":[]} {\"items\":[]}",
        "{\"items\":[]}\n\n",
    ] {
        std::fs::write(&path, raw).unwrap();
        store.failure(
            &["record", "--jsonl", path.to_str().unwrap()],
            "invalid_input",
            2,
        );
    }
    std::fs::write(&path, b"{\"items\":[]}\n\xff").unwrap();
    let error = store.failure(
        &["record", "--jsonl", path.to_str().unwrap()],
        "invalid_input",
        2,
    );
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("line 2")
    );
    std::fs::write(
        &path,
        r#"{"items":[{"kind":"type_membership","entity":{"ref":"r"},"entity_type":"person"}]}"#,
    )
    .unwrap();
    store.failure(
        &["record", "--jsonl", path.to_str().unwrap()],
        "invalid_input",
        2,
    );
    let mut full = b"{\"items\":[]}\n{\"items\":[]}".to_vec();
    full.resize(1024 * 1024 - 1, b' ');
    std::fs::write(&path, &full).unwrap();
    store.success(&["record", "--jsonl", path.to_str().unwrap()]);
    full.resize(1024 * 1024, b' ');
    std::fs::write(&path, &full).unwrap();
    store.success(&["record", "--jsonl", path.to_str().unwrap()]);
    full.push(b' ');
    std::fs::write(&path, &full).unwrap();
    store.failure(
        &["record", "--jsonl", path.to_str().unwrap()],
        "limit_exceeded",
        2,
    );
    assert_eq!(store.graph_files(), before);
    for raw in [
        r#"{"created_by":null,"items":[]}"#,
        r#"{"created_by":"","items":[]}"#,
    ] {
        std::fs::write(&path, raw).unwrap();
        store.failure(&["record", path.to_str().unwrap()], "invalid_input", 2);
        store.failure(
            &["record", "--jsonl", path.to_str().unwrap()],
            "invalid_input",
            2,
        );
    }
    std::fs::write(
        &path,
        "{\"created_by\":\"manual\",\"items\":[]}\n{\"created_by\":\"man\\u0075al\",\"items\":[]}",
    )
    .unwrap();
    store.success(&["record", "--jsonl", path.to_str().unwrap()]);
    for args in [
        vec![
            "record",
            path.to_str().unwrap(),
            "--jsonl",
            path.to_str().unwrap(),
        ],
        vec!["record", path.to_str().unwrap(), "--describe"],
        vec!["record", "--jsonl", path.to_str().unwrap(), "--describe"],
        vec!["record"],
    ] {
        let output = store.run(&args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
    for size in [999, 1000, 1001] {
        let a = json!({"items":(0..500).map(|_|literal_fact("decision",json!("x"))).collect::<Vec<_>>()});
        let b = json!({"items":(500..size).map(|_|literal_fact("decision",json!("x"))).collect::<Vec<_>>()});
        std::fs::write(&path, format!("{a}\n{b}")).unwrap();
        if size <= 1000 {
            let result = store.success(&["record", "--jsonl", path.to_str().unwrap()]);
            assert_eq!(result["result"]["items"].as_array().unwrap().len(), size);
        } else {
            let count_before = count(&store, "facts");
            store.failure(
                &["record", "--jsonl", path.to_str().unwrap()],
                "invalid_input",
                2,
            );
            assert_eq!(count(&store, "facts"), count_before);
        }
    }
}

#[test]
fn historical_facts_remain_readable_and_invalid_literals_fail_closed() {
    let store = Store::new();
    fact_types(&store);
    record(
        &store,
        &json!({"items":[{"kind":"entity","name":"Riley"},
        {"kind":"type_membership","entity":{"id":"entity:1"},"entity_type":"person"},
        literal_fact("count",json!(42))]}),
    );
    for literal in [
        "42.0",
        "9223372036854775808",
        "\"42\"",
        " 42",
        "true",
        "null",
    ] {
        store
            .database()
            .execute("UPDATE facts SET literal_json=?1", [literal])
            .unwrap();
        store.failure(&["get", "knowledge:2"], "internal_error", 1);
        store.failure(&["graph", "rebuild"], "internal_error", 1);
    }
    store
        .database()
        .execute_batch(
            "UPDATE facts SET literal_json='42';
        UPDATE knowledge_items SET withdrawn_at='2026-01-01T00:00:00Z',withdrawn_by='fixture';",
        )
        .unwrap();
    let got = store.success(&["get", "knowledge:2"])["result"].clone();
    assert_eq!(got["object"]["literal"], 42);
    assert_eq!(got["support"], json!([]));
    assert_eq!(got["withdrawn_by"], "fixture");
    store.success(&["graph", "rebuild"]);
    assert_eq!(
        store.success(&["get", "entity:1"])["result"]["active_fact_ids"],
        json!([])
    );
    assert_eq!(
        store.success(&["graph", "query", "SELECT ?s WHERE {?s ?p ?o}"])["result"]["rows"],
        json!([])
    );
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
