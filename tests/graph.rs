mod common;

use std::fs::{self, File};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::Store;
use commonplace::graph::{GraphRuntime, QueryConfig};
use oxigraph::model::{GraphName, Literal, NamedNode, Quad, vocab::xsd};
use oxigraph::sparql::SparqlEvaluator;
use serde_json::{Value, json};

const META_QUERY: &str = "SELECT ?v WHERE { GRAPH <urn:commonplace:metadata> { <urn:commonplace:store> <urn:commonplace:property:knowledge_version> ?v } }";
const ALL: &str = "SELECT ?s ?p ?o WHERE { ?s ?p ?o } ORDER BY ?s ?p ?o";
const QUOTE: &str = "café\r\n🦀 e\u{301}\0";

fn literal(value: impl ToString, datatype: &str) -> Value {
    json!({"type":"literal","value":value.to_string(),"datatype":format!("http://www.w3.org/2001/XMLSchema#{datatype}"),"language":null})
}

fn query(store: &Store, text: &str) -> Value {
    store.success(&["graph", "query", text])["result"].clone()
}

fn fixture(store: &Store) {
    store.apply(
        &json!({"entity_types":[{"name":"person"},{"name":"unused"}]}),
        false,
    );
    let db = store.database();
    db.execute_batch(
        "PRAGMA foreign_keys=ON;
         BEGIN IMMEDIATE;
         INSERT INTO entities VALUES (10,'Ada','2026-01-01T00:00:00Z',NULL),
                                     (11,'Uncited','2026-01-01T00:00:00Z',NULL),
                                     (12,'Bare','2026-01-01T00:00:00Z',NULL);
         INSERT INTO knowledge_items VALUES
             (100,'type_membership',1,'2026-01-01T00:00:00Z',NULL,NULL,NULL),
             (101,'type_membership',1,'2026-01-01T00:00:00Z',NULL,NULL,NULL),
             (102,'type_membership',1,'2026-01-01T00:00:00Z',NULL,NULL,NULL),
             (103,'type_membership',1,'2026-01-01T00:00:00Z',NULL,'2026-01-02T00:00:00Z',NULL);
         INSERT INTO entity_type_memberships VALUES (100,10,1),(101,10,1),(102,11,1),(103,12,2);
         INSERT INTO documents VALUES (10,'file:///original.txt','2026-01-01T00:00:00Z','2026-01-02T00:00:00Z'),
                                      (11,'file:///nullable.txt','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z'),
                                      (12,'file:///uncited.txt','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
         INSERT INTO document_revisions VALUES
             (1001,10,2,printf('%064d',2),'latest text','latest title','file',NULL,'{}','2026-01-02T00:00:00Z'),
             (1002,11,1,printf('%064d',3),'second',NULL,'file',NULL,'{}','2026-01-01T00:00:00Z'),
             (1003,12,1,printf('%064d',4),'uncited',NULL,'file',NULL,'{}','2026-01-01T00:00:00Z');"
    ).unwrap();
    let text = format!("prefix:{QUOTE}:suffix");
    db.execute("INSERT INTO document_revisions VALUES (1000,10,1,?1,?2,'old title','file','2026-01-01T00:00:00Z',?3,'2026-01-01T00:00:00Z')",
        rusqlite::params!["a".repeat(64),text,r#"{"nested":{"a":1},"z":"old"}"#]).unwrap();
    db.execute(
        "INSERT INTO passages VALUES (2000,1000,0,7,?1,?2)",
        rusqlite::params![7 + QUOTE.len(), QUOTE],
    )
    .unwrap();
    db.execute_batch(
        "INSERT INTO passages VALUES (2001,1002,0,0,6,'second'),(2002,1001,0,0,11,'latest text'),(2003,1003,0,0,7,'uncited');
         INSERT INTO knowledge_item_evidence VALUES (100,2000),(100,2001),(101,2000),(103,2003);
         UPDATE store_state SET knowledge_version=7;
         COMMIT;"
    ).unwrap();
}

#[test]
fn init_schema_query_rebuild_reopen_and_unused_vocabulary() {
    let store = Store::new();
    assert!(store.root.join("graph/current").is_dir());
    assert!(store.root.join("graph/publication.lock").is_file());
    assert!(!store.root.join("graph/current.grafeo").exists());
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(store.root.join("config.json")).unwrap())
            .unwrap(),
        json!({"format":"commonplace-config/2","database":"commonplace.sqlite3","graph":"graph/current"})
    );
    assert_eq!(
        query(&store, "SELECT ?knowledge WHERE {?knowledge ?p ?o}"),
        json!({"kind":"select","columns":["knowledge"],"rows":[],"truncated":false})
    );
    assert_eq!(
        query(&store, META_QUERY)["rows"],
        json!([[literal(0, "integer")]])
    );
    store.apply(&Store::vocabulary(), false);
    assert_eq!(query(&store, ALL)["rows"], json!([]));
    assert_eq!(
        store.success(&["graph", "rebuild"])["result"],
        json!({"knowledge_version":0})
    );
    assert_eq!(query(&store, ALL)["rows"], json!([]));
    assert_eq!(
        query(&store, META_QUERY)["rows"],
        json!([[literal(0, "integer")]])
    );
    let absent = store.directory.path().join("absent");
    let output = Command::new(env!("CARGO_BIN_EXE_commonplace"))
        .arg("--store")
        .arg(&absent)
        .args(["graph", "schema", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!absent.exists());
    let schema: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(schema["operation"], "graph.schema");
    assert_eq!(schema["result"]["rdf"], "1.1");
    assert_eq!(
        schema["result"]["projection"]["supported_knowledge_kinds"],
        json!(["type_membership", "fact"])
    );
    for example in schema["result"]["examples"].as_array().unwrap() {
        query(&store, example.as_str().unwrap());
    }
}

#[test]
fn membership_identity_evidence_bytes_and_complete_mapping_survive_rebuild() {
    let store = Store::new();
    fixture(&store);
    store.failure(&["graph", "query", ALL], "graph_unavailable", 1);
    assert_eq!(
        store.success(&["graph", "rebuild"])["result"],
        json!({"knowledge_version":7})
    );
    let before = query(&store, ALL);
    let triples = before["rows"].as_array().unwrap();
    let has = |s: &str, p: &str, object: Value| {
        triples.contains(&json!([
            {"type":"uri","value":format!("urn:commonplace:{s}")},
            {"type":"uri","value":format!("urn:commonplace:property:{p}")}, object
        ]))
    };
    assert!(has(
        "knowledge:100",
        "id",
        literal("knowledge:100", "string")
    ));
    assert!(has(
        "knowledge:101",
        "id",
        literal("knowledge:101", "string")
    ));
    assert!(has(
        "knowledge:102",
        "id",
        literal("knowledge:102", "string")
    ));
    assert!(has(
        "knowledge:100",
        "kind",
        literal("type_membership", "string")
    ));
    assert!(has(
        "knowledge:100",
        "schema_version",
        literal(1, "integer")
    ));
    assert!(has("type:1", "id", literal("entity-type:1", "string")));
    assert!(has("type:1", "name", literal("person", "string")));
    assert!(has("entity:10", "name", literal("Ada", "string")));
    assert!(has("passage:2000", "text", literal(QUOTE, "string")));
    assert!(has("passage:2000", "ordinal", literal(0, "integer")));
    assert!(has("passage:2000", "start_byte", literal(7, "integer")));
    assert!(has(
        "passage:2000",
        "end_byte",
        literal(7 + QUOTE.len(), "integer")
    ));
    assert!(has(
        "revision:1000",
        "revision_number",
        literal(1, "integer")
    ));
    assert!(has(
        "revision:1000",
        "revision_digest",
        literal("a".repeat(64), "string")
    ));
    assert!(has(
        "revision:1000",
        "source_type",
        literal("file", "string")
    ));
    assert!(has(
        "revision:1000",
        "metadata_json",
        literal(r#"{"nested":{"a":1},"z":"old"}"#, "string")
    ));
    assert!(has(
        "revision:1000",
        "title",
        literal("old title", "string")
    ));
    assert!(has(
        "revision:1000",
        "occurred_at",
        literal("2026-01-01T00:00:00Z", "dateTime")
    ));
    assert!(has(
        "doc:10",
        "source_key",
        literal("file:///original.txt", "string")
    ));
    let citations = query(&store, "PREFIX c: <urn:commonplace:property:>
        SELECT ?k ?p ?r ?d ?text WHERE {?k c:evidence ?p . ?p c:revision ?r;c:text ?text . ?r c:document ?d} ORDER BY ?k ?p");
    assert_eq!(citations["rows"].as_array().unwrap().len(), 3);
    assert_eq!(citations["rows"][0][4], literal(QUOTE, "string"));
    assert_eq!(
        query(
            &store,
            "PREFIX c: <urn:commonplace:property:> SELECT ?k ?e WHERE {?k c:kind 'type_membership' OPTIONAL {?k c:evidence ?e}} ORDER BY ?k ?e"
        )["rows"][3][1],
        Value::Null
    );
    for absent in [
        "knowledge:103",
        "entity:12",
        "type:2",
        "revision:1001",
        "doc:12",
        "passage:2003",
    ] {
        assert!(
            !triples
                .iter()
                .any(|row| row[0]["value"] == format!("urn:commonplace:{absent}"))
        );
    }
    assert_eq!(
        query(
            &store,
            "PREFIX c: <urn:commonplace:property:> SELECT ?title ?time WHERE { OPTIONAL {<urn:commonplace:revision:1002> c:title ?title} OPTIONAL {<urn:commonplace:revision:1002> c:occurred_at ?time} }"
        )["rows"],
        json!([[null, null]])
    );
    store.success(&["graph", "rebuild"]);
    assert_eq!(query(&store, ALL), before);
    assert_eq!(
        query(&store, META_QUERY)["rows"],
        json!([[literal(7, "integer")]])
    );
    assert_eq!(
        store
            .database()
            .query_row("SELECT knowledge_version FROM store_state", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        7
    );
}

#[test]
fn native_terms_unbound_columns_local_service_and_query_forms() {
    let store = Store::new();
    let before = store.graph_files();
    let result = query(
        &store,
        "SELECT ?uri ?blank ?lo ?hi ?boolean ?text ?time ?missing ?error WHERE {
        BIND(<urn:commonplace:entity:10> AS ?uri) BIND(BNODE('local') AS ?blank)
        BIND('-9223372036854775808'^^<http://www.w3.org/2001/XMLSchema#integer> AS ?lo)
        BIND(9223372036854775807 AS ?hi) BIND(true AS ?boolean) BIND('café'@fr AS ?text)
        BIND('2026-01-01T00:00:00Z'^^<http://www.w3.org/2001/XMLSchema#dateTime> AS ?time)
        BIND(1/0 AS ?error) }",
    );
    assert_eq!(
        result["columns"],
        json!([
            "uri", "blank", "lo", "hi", "boolean", "text", "time", "missing", "error"
        ])
    );
    let row = &result["rows"][0];
    assert_eq!(
        row[0],
        json!({"type":"uri","value":"urn:commonplace:entity:10"})
    );
    assert_eq!(row[1]["type"], "bnode");
    assert!(row[1]["value"].is_string());
    assert_eq!(row[1].as_object().unwrap().len(), 2);
    assert_eq!(row[2], literal(i64::MIN, "integer"));
    assert_eq!(row[3], literal(i64::MAX, "integer"));
    assert_eq!(row[4], literal("true", "boolean"));
    assert_eq!(
        row[5],
        json!({"type":"literal","value":"café","datatype":"http://www.w3.org/1999/02/22-rdf-syntax-ns#langString","language":"fr"})
    );
    assert_eq!(row[6], literal("2026-01-01T00:00:00Z", "dateTime"));
    assert!(row[7].is_null() && row[8].is_null());
    assert_eq!(
        query(&store, "SELECT (-9223372036854775808 AS ?native) WHERE {}")["rows"],
        json!([[null]])
    );
    for text in [
        "ASK {}",
        "CONSTRUCT {?s ?p ?o} WHERE {?s ?p ?o}",
        "DESCRIBE ?s WHERE {?s ?p ?o}",
        "INSERT DATA {<urn:a> <urn:b> <urn:c>}",
        "CLEAR ALL",
        "not sparql",
    ] {
        store.failure(&["graph", "query", text], "invalid_input", 2);
    }
    store.failure(
        &[
            "graph",
            "query",
            "SELECT * WHERE {SERVICE <http://127.0.0.1:9/sparql> {?s ?p ?o}}",
        ],
        "graph_unavailable",
        1,
    );
    assert_eq!(
        query(
            &store,
            "SELECT ?s WHERE {SERVICE SILENT <http://127.0.0.1:9/sparql> {?s ?p ?o}}"
        )["rows"],
        json!([[null]])
    );
    assert_eq!(
        query(
            &store,
            "SELECT ?s FROM <http://127.0.0.1:9/graph> WHERE {?s ?p ?o}"
        )["rows"],
        json!([])
    );
    assert_eq!(store.graph_files(), before);
}

#[test]
fn bounded_rows_preserve_native_order_limit_offset_distinct_and_aggregates() {
    let store = Store::new();
    for (text, expected) in [
        (
            "SELECT ?x WHERE {VALUES ?x {4 1 2 3}} ORDER BY ?x",
            vec![1, 2, 3, 4],
        ),
        (
            "SELECT DISTINCT ?x WHERE {VALUES ?x {3 1 1 2}} ORDER BY ?x",
            vec![1, 2, 3],
        ),
        (
            "SELECT ?x WHERE {VALUES ?x {1 2 3 4}} ORDER BY ?x OFFSET 1 LIMIT 2",
            vec![2, 3],
        ),
        ("SELECT (SUM(?v) AS ?x) WHERE {VALUES ?v {1 2 3}}", vec![6]),
        (
            "SELECT ?x WHERE {{VALUES ?x {1 2}} UNION {VALUES ?x {3 4}}} ORDER BY ?x",
            vec![1, 2, 3, 4],
        ),
    ] {
        for limit in [0, 1, 2, 3, 4, 5] {
            let result = store.success(&[
                "graph",
                "query",
                text,
                "--row-limit",
                &limit.to_string(),
            ])["result"]
                .clone();
            assert_eq!(
                result["rows"],
                Value::Array(
                    expected
                        .iter()
                        .take(limit)
                        .map(|v| json!([literal(v, "integer")]))
                        .collect()
                )
            );
            assert_eq!(result["truncated"], expected.len() > limit);
        }
    }
    assert_eq!(
        store.success(&[
            "graph",
            "query",
            "SELECT ?s WHERE {?s ?p ?o}",
            "--row-limit",
            "0"
        ])["result"]["truncated"],
        false
    );
    store.failure(
        &[
            "graph",
            "query",
            "SELECT * WHERE {}",
            "--row-limit",
            &usize::MAX.to_string(),
        ],
        "invalid_input",
        2,
    );
    store.failure(
        &["graph", "query", "SELECT * WHERE {}", "--timeout-ms", "0"],
        "invalid_input",
        2,
    );
    let huge = (usize::MAX - 1).to_string();
    assert_eq!(
        store.success(&[
            "graph",
            "query",
            "SELECT (1 AS ?x) WHERE {}",
            "--row-limit",
            &huge
        ])["result"]["rows"],
        json!([[literal(1, "integer")]])
    );
}

#[test]
fn native_read_only_rejects_mutation() {
    let store = Store::new();
    let lease = File::open(store.root.join("graph/publication.lock")).unwrap();
    lease.lock_shared().unwrap();
    let native = oxigraph::store::Store::open_read_only(store.root.join("graph/current")).unwrap();
    let quad = Quad::new(
        NamedNode::new("urn:a").unwrap(),
        NamedNode::new("urn:b").unwrap(),
        Literal::from(1),
        GraphName::DefaultGraph,
    );
    assert!(native.insert(&quad).is_err());
    assert!(native.clear().is_err());
    assert!(
        SparqlEvaluator::new()
            .parse_update("INSERT DATA {<urn:a> <urn:b> <urn:c>}")
            .unwrap()
            .on_store(&native)
            .execute()
            .is_err()
    );
    drop(native);
    drop(lease);
    assert_eq!(query(&store, ALL)["rows"], json!([]));
}

#[test]
fn old_unknown_and_relabeled_layouts_are_never_mutated() {
    for damage in [
        "v1-config",
        "v1-marker",
        "old-file",
        "unknown-config",
        "relabeled-ddl",
    ] {
        let store = Store::new();
        match damage {
            "v1-config" => fs::write(store.root.join("config.json"),br#"{"format":"commonplace-config/1","database":"commonplace.sqlite3","graph":"graph/current.grafeo"}"#).unwrap(),
            "v1-marker" => store.database().execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE store_state SET format='commonplace-store/1'").unwrap(),
            "old-file" => fs::write(store.root.join("graph/current.grafeo"),b"historical").unwrap(),
            "unknown-config" => fs::write(store.root.join("config.json"),br#"{"format":"future","database":"commonplace.sqlite3","graph":"graph/current"}"#).unwrap(),
            "relabeled-ddl" => store.database().execute_batch("PRAGMA writable_schema=ON; UPDATE sqlite_schema SET sql=replace(sql,'commonplace-store/2','commonplace-store/1') WHERE name='store_state'; PRAGMA writable_schema=OFF").unwrap(),
            _ => unreachable!(),
        }
        let before = store.files();
        for args in [
            vec!["init"],
            vec!["graph", "rebuild"],
            vec!["graph", "query", ALL],
            vec!["schema", "show"],
        ] {
            store.failure(&args, "conflict", 3);
            store.assert_files(&before);
        }
    }
}

#[test]
fn unavailable_graphs_fail_closed_and_explicit_rebuild_recovers_known_scratch_only() {
    for damage in [
        "missing",
        "corrupt",
        "version",
        "missing-metadata",
        "multiple",
        "wrong-type",
        "negative",
        "malformed",
        "out-of-range",
        "missing-lock",
    ] {
        let store = Store::new();
        let current = store.root.join("graph/current");
        match damage {
            "missing" => fs::rename(&current, store.root.join("graph/previous")).unwrap(),
            "corrupt" => fs::write(current.join("CURRENT"), b"corrupt\n").unwrap(),
            "version" => {
                store
                    .database()
                    .execute("UPDATE store_state SET knowledge_version=8", [])
                    .unwrap();
            }
            "missing-lock" => fs::remove_file(store.root.join("graph/publication.lock")).unwrap(),
            _ => {
                let native = oxigraph::store::Store::open(&current).unwrap();
                native.clear().unwrap();
                let value = match damage {
                    "wrong-type" => Literal::new_simple_literal("0"),
                    "negative" => Literal::from(-1),
                    "malformed" => Literal::new_typed_literal("no", xsd::INTEGER),
                    "out-of-range" => {
                        Literal::new_typed_literal("9223372036854775808", xsd::INTEGER)
                    }
                    _ => Literal::from(0),
                };
                let metadata = |value| {
                    Quad::new(
                        NamedNode::new("urn:commonplace:store").unwrap(),
                        NamedNode::new("urn:commonplace:property:knowledge_version").unwrap(),
                        value,
                        NamedNode::new("urn:commonplace:metadata").unwrap(),
                    )
                };
                if damage != "missing-metadata" {
                    native.insert(&metadata(value)).unwrap();
                }
                if damage == "multiple" {
                    native.insert(&metadata(Literal::from(1))).unwrap();
                }
                native.flush().unwrap();
            }
        }
        let before = store.files();
        store.failure(&["graph", "query", ALL], "graph_unavailable", 1);
        store.failure(&["init"], "graph_unavailable", 1);
        store.assert_files(&before);
        // SQLite-only operations must not depend on graph health or create graph files.
        store.success(&["schema", "show"]);
        store.apply(&json!({}), true);
        store.assert_files(&before);
        fs::write(store.root.join("graph/unrelated"), b"leave me").unwrap();
        fs::create_dir_all(store.root.join("graph/candidate")).unwrap();
        fs::write(store.root.join("graph/candidate/partial"), b"leftover").unwrap();
        store.success(&["graph", "rebuild"]);
        assert!(!store.root.join("graph/candidate").exists());
        assert!(!store.root.join("graph/previous").exists());
        assert_eq!(
            fs::read(store.root.join("graph/unrelated")).unwrap(),
            b"leave me"
        );
        assert_eq!(query(&store, ALL)["rows"], json!([]));
        assert_eq!(
            query(&store, META_QUERY)["rows"][0][0],
            literal(if damage == "version" { 8 } else { 0 }, "integer")
        );
    }
}

#[test]
fn invalid_subtypes_or_fact_endpoints_preserve_current() {
    for damage in [
        "no-subtype",
        "both",
        "wrong-kind",
        "fact-endpoints",
        "bad-slice",
        "negative-id",
    ] {
        let store = Store::new();
        fixture(&store);
        store.success(&["graph", "rebuild"]);
        let before = store.graph_files();
        let db = store.database();
        match damage {
            "no-subtype" => {
                db.execute(
                    "DELETE FROM entity_type_memberships WHERE knowledge_item_id=100",
                    [],
                )
                .unwrap();
            }
            "both" | "fact-endpoints" => {
                db.execute(
                    "INSERT INTO predicates VALUES (1,'knows','entity',NULL,1)",
                    [],
                )
                .unwrap();
                db.execute("INSERT INTO facts VALUES (100,10,1,11,NULL)", [])
                    .unwrap();
                if damage == "fact-endpoints" {
                    db.execute_batch("DELETE FROM entity_type_memberships WHERE knowledge_item_id=100; UPDATE knowledge_items SET kind='fact' WHERE knowledge_item_id=100").unwrap();
                }
            }
            "wrong-kind" => {
                db.execute(
                    "UPDATE knowledge_items SET kind='fact' WHERE knowledge_item_id=100",
                    [],
                )
                .unwrap();
            }
            "bad-slice" => {
                db.execute("UPDATE passages SET start_byte=8 WHERE passage_id=2000", [])
                    .unwrap();
            }
            "negative-id" => {
                db.execute_batch("PRAGMA foreign_keys=OFF; UPDATE knowledge_items SET knowledge_item_id=-1 WHERE knowledge_item_id=100; UPDATE entity_type_memberships SET knowledge_item_id=-1 WHERE knowledge_item_id=100; UPDATE knowledge_item_evidence SET knowledge_item_id=-1 WHERE knowledge_item_id=100;").unwrap();
            }
            _ => unreachable!(),
        }

        drop(db);
        store.failure(
            &["graph", "rebuild"],
            if damage == "bad-slice" {
                "internal_error"
            } else {
                "graph_unavailable"
            },
            1,
        );
        let after = store.graph_files();
        for (path, bytes) in before
            .iter()
            .filter(|(path, _)| path.starts_with(store.root.join("graph/current")))
        {
            assert_eq!(after.get(path), Some(bytes));
        }
    }
}

#[test]
fn two_reader_processes_exclude_publication_and_termination_releases_leases() {
    let store = Store::new();
    let first = common::LockHolder::start_named(&store, "graph_reader", "reader-one");
    let second = common::LockHolder::start_named(&store, "graph_reader", "reader-two");
    let before = store.graph_files();
    let error = commonplace::graph::rebuild(&store.root, Duration::from_millis(40)).unwrap_err();
    assert_eq!(error.code(), "conflict");
    assert_eq!(store.graph_files(), before);
    query(&store, META_QUERY);
    drop(first);
    assert_eq!(
        commonplace::graph::rebuild(&store.root, Duration::ZERO)
            .unwrap_err()
            .code(),
        "conflict"
    );
    drop(second);
    store.success(&["graph", "rebuild"]);
}

#[test]
#[ignore = "child-process reader helper"]
fn graph_reader() {
    let root = std::path::PathBuf::from(std::env::var_os("COMMONPLACE_TEST_STORE").unwrap());
    let graph = GraphRuntime::open(&root).unwrap();
    assert!(
        !graph
            .query(META_QUERY, QueryConfig::default())
            .unwrap()
            .rows
            .is_empty()
    );
    fs::write(
        std::env::var_os("COMMONPLACE_TEST_READY").unwrap(),
        b"ready",
    )
    .unwrap();
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[test]
fn native_writer_lock_is_separate_from_publication_lock() {
    let store = Store::new();
    let native = oxigraph::store::Store::open(store.root.join("graph/current")).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "native_writer_contender"])
        .env("COMMONPLACE_TEST_STORE", &store.root)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    assert!(common::wait_for_exit(&mut child, Duration::from_secs(10)).success());
    drop(native);
    query(&store, META_QUERY);
}

#[test]
#[ignore = "native writer contention helper"]
fn native_writer_contender() {
    let root = std::path::PathBuf::from(std::env::var_os("COMMONPLACE_TEST_STORE").unwrap());
    assert!(oxigraph::store::Store::open(root.join("graph/current")).is_err());
}

fn expensive_store(store: &Store) {
    let native = oxigraph::store::Store::open(store.root.join("graph/current")).unwrap();
    native
        .extend((0..600).map(|id| {
            Quad::new(
                NamedNode::new(format!("urn:probe:{id}")).unwrap(),
                NamedNode::new("urn:probe:n").unwrap(),
                Literal::from(id),
                GraphName::DefaultGraph,
            )
        }))
        .unwrap();
    native.flush().unwrap();
}

fn expensive(aggregate: bool) -> String {
    format!(
        "SELECT {} WHERE {{?a <urn:probe:n> ?x . ?b <urn:probe:n> ?y . ?c <urn:probe:n> ?z . FILTER(?x+?y+?z<0)}}",
        if aggregate {
            "(COUNT(*) AS ?count)"
        } else {
            "?x"
        }
    )
}

#[test]
fn cancellation_covers_execution_and_iteration_without_partial_success() {
    let store = Store::new();
    expensive_store(&store);
    for aggregate in [true, false] {
        let mut child = store
            .command()
            .args([
                "graph",
                "query",
                &expensive(aggregate),
                "--timeout-ms",
                "20",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        assert_eq!(
            common::wait_for_exit(&mut child, Duration::from_secs(10)).code(),
            Some(2)
        );
        let output = child.wait_with_output().unwrap();
        assert!(output.stdout.is_empty());
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], "limit_exceeded");
    }
    // Completion, truncation, evaluation failure, and syntax rejection do not wait for 30s.
    for text in [
        "SELECT (1 AS ?x) WHERE {}",
        "SELECT ?x WHERE {VALUES ?x {1 2 3}}",
        "SELECT ?x WHERE {SERVICE <http://127.0.0.1:9/sparql> {?x ?p ?o}}",
        "ASK {}",
    ] {
        let start = Instant::now();
        let mut child = store
            .command()
            .args([
                "graph",
                "query",
                text,
                "--timeout-ms",
                "30000",
                "--row-limit",
                "1",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        common::wait_for_exit(&mut child, Duration::from_secs(5));
        assert!(start.elapsed() < Duration::from_secs(5));
    }
    store.success(&["graph", "rebuild"]);
}

#[cfg(unix)]
#[test]
fn sigint_terminates_query_and_releases_publication_lease() {
    use std::os::unix::process::ExitStatusExt;
    let store = Store::new();
    expensive_store(&store);
    let mut child = store
        .command()
        .args(["graph", "query", &expensive(true), "--timeout-ms", "30000"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(store.root.join("graph/publication.lock"))
        .unwrap();
    loop {
        match lock.try_lock() {
            Err(fs::TryLockError::WouldBlock) => break,
            Ok(()) => lock.unlock().unwrap(),
            Err(error) => panic!("{error}"),
        }
        if start.elapsed() > Duration::from_secs(5) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("query did not take lease");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        common::wait_for_exit(&mut child, Duration::from_secs(5)).signal(),
        Some(2)
    );
    drop(lock);
    store.success(&["graph", "rebuild"]);
}

#[test]
fn snapshot_crosses_page_and_batch_boundaries_without_losing_items() {
    let store = Store::new();
    store.apply(&json!({"entity_types":[{"name":"person"}]}), false);
    let db = store.database();
    db.execute_batch("BEGIN; INSERT INTO entities VALUES (1,'one','2026-01-01T00:00:00Z',NULL);
                WITH RECURSIVE ids(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM ids WHERE n<513)
                INSERT INTO knowledge_items SELECT n,'type_membership',1,'2026-01-01T00:00:00Z',NULL,NULL,NULL FROM ids;
                INSERT INTO entity_type_memberships SELECT knowledge_item_id,1,1 FROM knowledge_items;
                INSERT INTO documents VALUES (1,'file:///batch.txt','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
                INSERT INTO document_revisions VALUES (1,1,1,printf('%064d',0),replace(printf('%01026d',0),'0','a'),NULL,'file',NULL,'{}','2026-01-01T00:00:00Z');
                INSERT INTO passages SELECT knowledge_item_id,1,knowledge_item_id-1,(knowledge_item_id-1)*2,knowledge_item_id*2,'aa' FROM knowledge_items;
                INSERT INTO knowledge_item_evidence SELECT 1,passage_id FROM passages;
                INSERT INTO knowledge_item_evidence SELECT knowledge_item_id,knowledge_item_id FROM knowledge_items WHERE knowledge_item_id>1;
                UPDATE store_state SET knowledge_version=1; COMMIT;").unwrap();
    drop(db);
    store.success(&["graph", "rebuild"]);
    assert_eq!(
        query(
            &store,
            "PREFIX c: <urn:commonplace:property:> SELECT (COUNT(?k) AS ?n) WHERE {?k c:kind 'type_membership'}"
        )["rows"],
        json!([[literal(513, "integer")]])
    );
    assert_eq!(query(&store,"PREFIX c: <urn:commonplace:property:> SELECT ?id WHERE {?k c:kind 'type_membership';c:id ?id}")["rows"].as_array().unwrap().len(),513);
    assert_eq!(
        query(
            &store,
            "PREFIX c: <urn:commonplace:property:> SELECT (COUNT(*) AS ?n) WHERE {?k c:evidence ?p}"
        )["rows"],
        json!([[literal(1025, "integer")]])
    );
    assert_eq!(
        query(
            &store,
            "PREFIX c: <urn:commonplace:property:> SELECT (COUNT(*) AS ?n) WHERE {?p c:text ?text}"
        )["rows"],
        json!([[literal(513, "integer")]])
    );
}

#[test]
fn default_row_budget_has_exact_boundary_behavior() {
    let store = Store::new();
    for count in [999, 1000, 1001] {
        let values = (0..count)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let result = query(
            &store,
            &format!("SELECT ?x WHERE {{VALUES ?x {{{values}}}}}"),
        );
        assert_eq!(result["rows"].as_array().unwrap().len(), count.min(1000));
        assert_eq!(result["truncated"], count > 1000);
    }
}
