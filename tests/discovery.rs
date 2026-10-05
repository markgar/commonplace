mod common;

use common::Store;
use commonplace::Result;
use commonplace::app::{ingest, search};
use commonplace::domain::documents::{DocumentInput, TemporalState};
use commonplace::domain::search::{DocumentScope, MAX_SCOPE_IDS, SearchRequest};
use commonplace::providers::embeddings::{
    EMBEDDING_DIMENSIONS, EMBEDDING_IDENTITY, EmbeddingModel,
};
use commonplace::providers::reranker::Reranker;
use commonplace::storage::{database::SqliteDatabase, evidence, search as queries};
use serde_json::{Value, json};

struct Model;

fn vector(axis: usize) -> Vec<f32> {
    let mut values = vec![0.0; EMBEDDING_DIMENSIONS];
    values[axis] = 1.0;
    values
}

impl EmbeddingModel for Model {
    fn identity(&self) -> &'static str {
        EMBEDDING_IDENTITY
    }
    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|text| vector(usize::from(text.starts_with("selected"))))
            .collect())
    }
}

impl Reranker for Model {
    fn rerank(&mut self, _: &str, texts: &[&str]) -> Result<Vec<f32>> {
        Ok(texts
            .iter()
            .map(|text| if text.contains("action") { 2.0 } else { 1.0 })
            .collect())
    }
}

fn request() -> SearchRequest {
    SearchRequest {
        query: "modernization risk".into(),
        must_contain: None,
        since: None,
        until: None,
        source_types: vec![],
        scope: None,
        limit: 50,
    }
}

fn input(
    key: &str,
    text: &str,
    kind: &str,
    state: TemporalState,
    time: Option<&str>,
) -> DocumentInput {
    DocumentInput {
        source_key: key.into(),
        text: text.into(),
        title: Some(key.into()),
        source_type: kind.into(),
        temporal_state: state,
        occurred_at: time.map(str::to_owned),
        metadata: Default::default(),
    }
}

fn publish(store: &Store, inputs: Vec<DocumentInput>) -> Value {
    let result = ingest::ingest(
        &store.root,
        inputs.into_iter().map(|document| ingest::InputItem {
            input: document.source_key.clone(),
            source_key: Some(document.source_key.clone()),
            document: Ok(document),
        }),
        &mut Model,
        &ingest::OperationConfig::default(),
    )
    .unwrap();
    assert_eq!(result.summary.failed, 0);
    serde_json::to_value(result).unwrap()
}

fn execute(store: &Store, request: &SearchRequest) -> commonplace::domain::search::SearchResult {
    search::search(&store.root, request, &mut Model, &mut Model).unwrap()
}

fn record(store: &Store, value: Value) -> Value {
    let path = store.input(&value);
    store.success(&["record", path.to_str().unwrap()])["result"].clone()
}

#[test]
fn graph_selection_prefilters_both_branches_then_fuses_reranks_and_keeps_history() {
    let store = Store::new();
    store.apply(&json!({"entity_types":[{"name":"organization"}]}), false);
    let mut inputs = (0..70)
        .map(|index| {
            input(
                &format!("outside-{index}"),
                "modernization risk",
                "note",
                TemporalState::Dated,
                Some("2026-10-01T00:00:00Z"),
            )
        })
        .collect::<Vec<_>>();
    inputs.push(input(
        "selected-a",
        "selected modernization risk context",
        "note",
        TemporalState::Dated,
        Some("2026-10-01T00:00:00.000000001Z"),
    ));
    inputs.push(input(
        "selected-b",
        "selected modernization risk action",
        "recap",
        TemporalState::Timeless,
        None,
    ));
    let added = publish(&store, inputs);
    let old = added["items"][70]["passage_ids"][0]
        .as_str()
        .unwrap()
        .to_owned();
    record(
        &store,
        json!({"items":[
            {"kind":"entity","ref":"a","name":"Synthetic Organization"},
            {"kind":"type_membership","entity":{"ref":"a"},"entity_type":"organization",
             "support":[{"passage_id":old},{"passage_id":added["items"][71]["passage_ids"][0]}]}
        ]}),
    );
    let query = "PREFIX c:<urn:commonplace:property:> SELECT DISTINCT ?document WHERE {
        ?entity c:name \"Synthetic Organization\" .
        ?membership c:subject ?entity; c:evidence ?passage .
        ?passage c:revision ?revision . ?revision c:document ?document
    } ORDER BY ?document";
    let graph = store.success(&["graph", "query", query, "--document-scope", "document"]);
    assert_eq!(
        graph["result"],
        json!({"document_ids":["doc:71","doc:72"],"truncated":false})
    );
    let mut request = request();
    let db = SqliteDatabase::read(&store.root).unwrap();
    let filters = request.validate().unwrap();
    for candidates in [
        queries::lexical(db.connection(), &request.lexical_query(), &filters).unwrap(),
        queries::vector(db.connection(), &vector(0), &filters).unwrap(),
    ] {
        assert!(candidates.truncated);
        assert!(candidates.ids.iter().all(|id| id.value() <= 70));
    }
    drop(db);
    request.scope = Some(serde_json::from_value(graph["result"].clone()).unwrap());
    let db = SqliteDatabase::read(&store.root).unwrap();
    let filters = request.validate().unwrap();
    for candidates in [
        queries::lexical(db.connection(), &request.lexical_query(), &filters).unwrap(),
        queries::vector(db.connection(), &vector(0), &filters).unwrap(),
    ] {
        assert_eq!(
            candidates
                .ids
                .iter()
                .map(|id| id.value())
                .collect::<Vec<_>>(),
            [71, 72]
        );
        assert!(!candidates.truncated);
    }
    drop(db);
    let result = execute(&store, &request);
    assert_eq!(result.items.len(), 2);
    assert_eq!(result.items[0].evidence.source_key, "selected-b");
    assert!(!result.truncated);
    assert_eq!(result.scope.unwrap().eligible_passages, 2);
    request.must_contain = Some("context".into());
    request.source_types = vec!["note".into()];
    request.since = Some("2026-09-30T19:00:00.000000001-05:00".into());
    request.until = Some("2026-10-01T00:00:00.000000001Z".into());
    let result = execute(&store, &request);
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].evidence.passage_id.to_string(), old);
    assert_eq!(result.scope.unwrap().excluded_source_type, 1);
    assert_eq!(result.temporal_filter.unwrap().coverage.eligible_dated, 1);
    let revised = publish(
        &store,
        vec![input(
            "selected-a",
            "selected modernization risk context replacement",
            "note",
            TemporalState::Dated,
            Some("2026-10-01T00:00:00.000000001Z"),
        )],
    );
    let current = revised["items"][0]["passage_ids"][0].as_str().unwrap();
    assert_eq!(
        execute(&store, &request).items[0]
            .evidence
            .passage_id
            .to_string(),
        current
    );
    assert_eq!(
        store.success(&["get", &old])["result"]["text"],
        "selected modernization risk context"
    );
    assert_eq!(
        store.success(&["graph", "query", query, "--document-scope", "document"])["result"],
        graph["result"]
    );
    store.success(&["remove", "--source-key", "selected-a"]);
    let result = execute(&store, &request);
    assert!(result.items.is_empty());
    assert_eq!(
        result.scope.unwrap().missing_document_ids[0].to_string(),
        "doc:71"
    );
}

#[test]
fn scope_missing_empty_duplicates_bounds_and_cli_bridge_failures_are_explicit() {
    let store = Store::new();
    publish(
        &store,
        vec![input(
            "one",
            "selected modernization risk",
            "note",
            TemporalState::Unknown,
            None,
        )],
    );
    let mut request = request();
    request.scope = Some(DocumentScope {
        document_ids: vec!["doc:1".into(), "doc:1".into(), "doc:999".into()],
        truncated: true,
    });
    let result = execute(&store, &request);
    let scope = result.scope.unwrap();
    assert_eq!(
        (
            scope.supplied_ids,
            scope.duplicate_ids,
            scope.selected_sources,
            scope.existing_sources
        ),
        (3, 1, 2, 1)
    );
    assert_eq!(scope.missing_document_ids[0].to_string(), "doc:999");
    assert!(scope.selection_truncated);
    assert!(
        scope
            .diagnostics
            .iter()
            .any(|d| d.code == "selection_truncated")
    );
    request.limit = 0;
    let result = execute(&store, &request);
    assert!(result.truncated && result.scope.unwrap().result_truncated);
    request.scope = Some(DocumentScope {
        document_ids: vec![],
        truncated: false,
    });
    let result = execute(&store, &request);
    assert!(result.items.is_empty() && !result.truncated);
    assert!(
        result
            .scope
            .unwrap()
            .diagnostics
            .iter()
            .any(|d| d.code == "empty_scope")
    );
    request.scope = None;
    request.limit = 50;
    assert_eq!(execute(&store, &request).items.len(), 1);
    for id in [
        "doc:0",
        "doc:-1",
        "doc:+1",
        "doc:01",
        "doc:9223372036854775808",
        "passage:1",
        "unknown:1",
        "doc:1 ",
    ] {
        request.scope = Some(DocumentScope {
            document_ids: vec![id.into()],
            truncated: false,
        });
        assert_eq!(
            request.validate().unwrap_err().code(),
            "invalid_input",
            "{id}"
        );
    }
    request.scope = Some(DocumentScope {
        document_ids: vec!["doc:1".into(); MAX_SCOPE_IDS + 1],
        truncated: false,
    });
    assert_eq!(request.validate().unwrap_err().code(), "limit_exceeded");
    request.scope.as_mut().unwrap().document_ids.pop();
    assert_eq!(request.validate().unwrap().document_ids.unwrap().len(), 1);
    let describe = store.success(&["search", "--describe-scope"]);
    assert!(jsonschema::is_valid(
        &describe["result"]["input_schema"],
        &json!({"document_ids":[],"truncated":false})
    ));
    assert!(!jsonschema::is_valid(
        &describe["result"]["input_schema"],
        &json!({"document_ids":["doc:01"],"truncated":false})
    ));
    for value in [
        "{\"document_ids\":[],\"truncated\":false,\"extra\":1}",
        "{\"document_ids\":[],\"document_ids\":[\"doc:1\"],\"truncated\":false}",
        "[]",
        "{\"document_ids\":[null],\"truncated\":false}",
        "{\"document_ids\":[]}",
        "{\"document_ids\":[\"doc:01\"],\"truncated\":false}",
    ] {
        let mut command = store.command();
        command.args(["search", "query", "--scope", "-"]);
        let output = common::with_stdin(command, value.as_bytes());
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"],
            "invalid_input"
        );
    }
    let path = store.directory.path().join("scope.json");
    std::fs::write(&path, r#"{"document_ids":[],"truncated":false}"#).unwrap();
    store.failure(
        &[
            "--model-cache",
            store
                .directory
                .path()
                .join("missing-models")
                .to_str()
                .unwrap(),
            "search",
            "query",
            "--scope",
            path.to_str().unwrap(),
        ],
        "model_unavailable",
        1,
    );
    let base = r#"{"document_ids":[],"truncated":false}"#;
    for (size, code, exit) in [
        (65536, "model_unavailable", 1),
        (65537, "limit_exceeded", 2),
    ] {
        std::fs::write(&path, format!("{base}{}", " ".repeat(size - base.len()))).unwrap();
        store.failure(
            &[
                "--model-cache",
                store
                    .directory
                    .path()
                    .join("missing-models")
                    .to_str()
                    .unwrap(),
                "search",
                "q",
                "--scope",
                path.to_str().unwrap(),
            ],
            code,
            exit,
        );
    }
    for query in [
        "SELECT ?d WHERE {VALUES ?d {<urn:commonplace:revision:1>}}",
        "SELECT ?d WHERE {VALUES ?d {<urn:commonplace:doc:01>}}",
        "SELECT ?d WHERE {VALUES ?d {\"doc:1\"}}",
        "SELECT ?d WHERE {VALUES ?d {UNDEF}}",
    ] {
        store.failure(
            &["graph", "query", query, "--document-scope", "d"],
            "invalid_input",
            2,
        );
    }
    store.failure(
        &[
            "graph",
            "query",
            "SELECT ?d WHERE {}",
            "--document-scope",
            "absent",
        ],
        "invalid_input",
        2,
    );
    let zero = store.success(&[
        "graph",
        "query",
        "SELECT ?d WHERE {VALUES ?d {<urn:commonplace:doc:1>}}",
        "--document-scope",
        "d",
        "--row-limit",
        "0",
    ]);
    assert_eq!(zero["result"], json!({"document_ids":[],"truncated":true}));
}

#[test]
fn until_windows_prefilter_candidates_preserve_precision_states_and_scoped_coverage() {
    let store = Store::new();
    let boundary = "2026-10-01T00:00:00.000000001Z";
    let mut inputs = (0..70)
        .map(|index| {
            input(
                &format!("future-{index}"),
                "modernization risk",
                "note",
                TemporalState::Dated,
                Some("2026-10-01T00:00:00.000000002Z"),
            )
        })
        .collect::<Vec<_>>();
    inputs.extend([
        input(
            "earlier",
            "selected modernization risk",
            "note",
            TemporalState::Dated,
            Some("2026-10-01T00:00:00Z"),
        ),
        input(
            "exact",
            "selected modernization risk",
            "note",
            TemporalState::Dated,
            Some(boundary),
        ),
        input(
            "timeless",
            "selected modernization risk",
            "note",
            TemporalState::Timeless,
            None,
        ),
        input(
            "unknown",
            "selected modernization risk",
            "note",
            TemporalState::Unknown,
            None,
        ),
        input("empty", "", "note", TemporalState::Timeless, None),
    ]);
    publish(&store, inputs);
    let mut request = request();
    request.until = Some("2026-09-30T19:00:00.000000001-05:00".into());
    let db = SqliteDatabase::read(&store.root).unwrap();
    let filters = request.validate().unwrap();
    for candidates in [
        queries::lexical(db.connection(), &request.lexical_query(), &filters).unwrap(),
        queries::vector(db.connection(), &vector(0), &filters).unwrap(),
    ] {
        assert_eq!(
            candidates
                .ids
                .iter()
                .map(|id| id.value())
                .collect::<Vec<_>>(),
            [71, 72, 73]
        );
        assert!(!candidates.truncated);
    }
    drop(db);
    let result = execute(&store, &request);
    assert_eq!(result.items.len(), 3);
    let temporal = result.temporal_filter.unwrap();
    assert_eq!(temporal.until.as_deref(), Some(boundary));
    assert_eq!(
        serde_json::to_value(temporal.coverage).unwrap(),
        json!({"eligible_dated":2,"timeless":2,"older_dated":0,"newer_dated":70,"excluded_unknown":1})
    );
    request.since = Some(boundary.into());
    let result = execute(&store, &request);
    assert_eq!(result.items.len(), 2);
    assert_eq!(result.temporal_filter.unwrap().coverage.older_dated, 1);
    request.scope = Some(DocumentScope {
        document_ids: vec!["doc:72".into(), "doc:74".into(), "doc:75".into()],
        truncated: false,
    });
    request.must_contain = Some("absent".into());
    let result = execute(&store, &request);
    assert!(result.items.is_empty());
    assert_eq!(result.temporal_filter.unwrap().coverage.excluded_unknown, 1);
    let scope = result.scope.unwrap();
    assert_eq!((scope.eligible_sources, scope.eligible_passages), (2, 0));
    assert!(
        scope
            .diagnostics
            .iter()
            .any(|d| d.code == "no_eligible_passages")
    );
    request.since = Some("2026-10-01T00:00:00.000000002Z".into());
    assert_eq!(request.validate().unwrap_err().code(), "invalid_input");
    store.failure(
        &[
            "search",
            "q",
            "--since",
            "2026-10-01T00:00:00.000000002Z",
            "--until",
            boundary,
        ],
        "invalid_input",
        2,
    );
    store.failure(&["search", "q", "--until", "tomorrow"], "invalid_input", 2);
}

#[test]
fn entity_discovery_resolution_active_types_paging_and_missing_identifiers_are_read_only() {
    let store = Store::new();
    store.apply(
        &json!({"entity_types":[{"name":"organization"},{"name":"unused"}],
        "identifier_schemes":[{"name":"account"},{"name":"other"}]}),
        false,
    );
    let authored = record(
        &store,
        json!({"items":[
            {"kind":"entity","ref":"a","name":"Alpha","aliases":["A","Shared"],
             "identifiers":[{"scheme":"account","value":"Opaque:001/A"}]},
            {"kind":"entity","ref":"b","name":"Beta","aliases":["Shared"]},
            {"kind":"entity","ref":"c","name":"Untyped"},
            {"kind":"type_membership","entity":{"ref":"a"},"entity_type":"organization"},
            {"kind":"type_membership","entity":{"ref":"b"},"entity_type":"organization"}
        ]}),
    );
    let before = store.files();
    let listed = store.success(&["entity", "list"]);
    assert_eq!(listed["result"]["items"].as_array().unwrap().len(), 3);
    let alpha = &listed["result"]["items"][0];
    let mut exact = store.success(&["get", "entity:1"])["result"].clone();
    exact.as_object_mut().unwrap().remove("kind");
    assert_eq!(alpha, &exact);
    for args in [
        vec!["entity", "resolve", "--name", "Alpha"],
        vec!["entity", "resolve", "--name", "A"],
        vec![
            "entity",
            "resolve",
            "--scheme",
            "account",
            "--value",
            "Opaque:001/A",
        ],
        vec!["entity", "resolve", "--id", "entity:1"],
    ] {
        assert_eq!(store.success(&args)["result"], exact);
    }
    store.failure(&["entity", "resolve", "--name", "Shared"], "conflict", 3);
    store.failure(&["entity", "resolve", "--name", "alpha"], "not_found", 2);
    store.failure(
        &[
            "entity",
            "resolve",
            "--scheme",
            "account",
            "--value",
            "opaque:001/a",
        ],
        "not_found",
        2,
    );
    store.failure(
        &["entity", "resolve", "--scheme", "absent", "--value", "x"],
        "invalid_input",
        2,
    );
    store.failure(&["entity", "resolve", "--name", ""], "invalid_input", 2);
    store.failure(&["entity", "list", "--type", "absent"], "invalid_input", 2);
    store.failure(
        &["entity", "list", "--missing-identifier", "absent"],
        "invalid_input",
        2,
    );
    store.failure(&["entity", "list", "--limit", "1001"], "limit_exceeded", 2);
    let missing = store.success(&[
        "entity",
        "list",
        "--type",
        "organization",
        "--missing-identifier",
        "account",
    ]);
    assert_eq!(missing["result"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(missing["result"]["items"][0]["entity_id"], "entity:2");
    let first = store.success(&["entity", "list", "--limit", "1"]);
    assert_eq!(first["result"]["next_after"], "entity:1");
    let next = store.success(&["entity", "list", "--limit", "2", "--after", "entity:1"]);
    assert_eq!(next["result"]["items"].as_array().unwrap().len(), 2);
    assert_eq!(next["result"]["truncated"], false);
    let zero = store.success(&["entity", "list", "--limit", "0"]);
    assert_eq!(
        zero["result"],
        json!({"items":[],"truncated":true,"next_after":null})
    );
    assert_eq!(
        store.success(&["entity", "list", "--type", "unused"])["result"]["items"],
        json!([])
    );
    store.assert_files(&before);
    let path = store.input(&json!({"knowledge_ids":[authored["items"][4]["knowledge_id"]]}));
    store.success(&["withdraw", path.to_str().unwrap()]);
    let active = store.success(&["entity", "list", "--type", "organization"]);
    assert_eq!(active["result"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        store.success(&["entity", "resolve", "--id", "entity:2"])["result"]["canonical_name"],
        "Beta"
    );
}

#[test]
fn compact_groups_keep_full_exact_passages_spans_and_global_ranks() {
    let store = Store::new();
    let text = format!(
        "selected modernization risk {}\n\n{}",
        "é".repeat(900),
        "action plan ".repeat(500)
    );
    publish(
        &store,
        vec![
            input("long", &text, "note", TemporalState::Timeless, None),
            input(
                "short",
                "selected modernization risk action",
                "note",
                TemporalState::Unknown,
                None,
            ),
        ],
    );
    let mut request = request();
    request.limit = 50;
    let result = execute(&store, &request);
    let expected_ids = result
        .items
        .iter()
        .map(|item| item.evidence.passage_id)
        .collect::<Vec<_>>();
    let grouped = result.grouped();
    assert_eq!(grouped.groups.len(), 2);
    assert!(
        grouped
            .groups
            .windows(2)
            .all(|groups| { groups[0].passages[0].rank < groups[1].passages[0].rank })
    );
    let mut found = vec![];
    let db = SqliteDatabase::read(&store.root).unwrap();
    for group in &grouped.groups {
        for excerpt in &group.passages {
            let exact = evidence::passage(db.connection(), excerpt.passage_id).unwrap();
            assert_eq!(
                (exact.document_id, exact.revision_id),
                (group.document_id, group.revision_id)
            );
            assert_eq!(
                (excerpt.start_byte, excerpt.end_byte),
                (exact.start_byte, exact.end_byte)
            );
            assert_eq!(excerpt.text, exact.text);
            assert!(excerpt.text.len() <= commonplace::domain::passages::PASSAGE_TARGET_BYTES);
            assert_eq!(excerpt.text.len(), excerpt.end_byte - excerpt.start_byte);
            found.push((excerpt.rank, excerpt.passage_id));
        }
    }
    found.sort_by_key(|(rank, _)| *rank);
    assert_eq!(
        found.iter().map(|(_, id)| *id).collect::<Vec<_>>(),
        expected_ids
    );
    assert!(
        grouped
            .groups
            .iter()
            .flat_map(|group| &group.passages)
            .any(|passage| passage.text.chars().count() > 600)
    );
    request.limit = 2;
    assert_eq!(
        execute(&store, &request)
            .grouped()
            .groups
            .iter()
            .map(|group| group.passages.len())
            .sum::<usize>(),
        2
    );
    request.limit = 0;
    assert!(execute(&store, &request).grouped().groups.is_empty());
}
