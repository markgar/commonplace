mod common;

use commonplace::app::{ingest, search};
use commonplace::domain::documents::DocumentInput;
use commonplace::domain::ids::PassageId;
use commonplace::domain::search::{CANDIDATE_LIMIT, SearchRequest, fuse};
use commonplace::providers::embeddings::{
    EMBEDDING_DIMENSIONS, EMBEDDING_IDENTITY, EmbeddingModel,
};
use commonplace::providers::reranker::{LocalReranker, Reranker};
use commonplace::storage::{database::SqliteDatabase, evidence, search as queries};
use commonplace::{CommonplaceError, Result};
use serde_json::json;

use common::Store;

#[derive(Default)]
struct Embedding {
    calls: usize,
    on_embed: Option<Box<dyn FnMut()>>,
    malformed: bool,
}

fn vector(axis: usize) -> Vec<f32> {
    let mut result = vec![0.0; EMBEDDING_DIMENSIONS];
    result[axis] = 1.0;
    result
}

impl EmbeddingModel for Embedding {
    fn identity(&self) -> &'static str {
        EMBEDDING_IDENTITY
    }
    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        self.calls += 1;
        if let Some(hook) = self.on_embed.as_mut() {
            hook();
        }
        if self.malformed {
            return Ok(vec![vec![1.0]]);
        }
        Ok(texts
            .iter()
            .map(|text| vector(usize::from(text.starts_with("lexical"))))
            .collect())
    }
}

#[derive(Default)]
struct Ranker {
    calls: Vec<usize>,
    scores: Option<Vec<f32>>,
}

impl Reranker for Ranker {
    fn rerank(&mut self, _: &str, texts: &[&str]) -> Result<Vec<f32>> {
        self.calls.push(texts.len());
        Ok(self
            .scores
            .clone()
            .unwrap_or_else(|| vec![0.0; texts.len()]))
    }
}

fn input(key: &str, text: &str, source_type: &str, time: Option<&str>) -> ingest::InputItem {
    ingest::InputItem {
        input: key.into(),
        source_key: Some(key.into()),
        document: Ok(DocumentInput {
            source_key: key.into(),
            text: text.into(),
            title: Some(format!("Title {key}")),
            source_type: source_type.into(),
            temporal_state: if time.is_some() {
                commonplace::domain::documents::TemporalState::Dated
            } else {
                commonplace::domain::documents::TemporalState::Unknown
            },
            occurred_at: time.map(str::to_owned),
            metadata: json!({"nested":{"value":"exact"}})
                .as_object()
                .unwrap()
                .clone(),
        }),
    }
}

fn publish(root: &std::path::Path, inputs: Vec<ingest::InputItem>) {
    let result = ingest::ingest(
        root,
        inputs.into_iter(),
        &mut Embedding::default(),
        &ingest::OperationConfig::default(),
    )
    .unwrap();
    assert_eq!(result.summary.failed, 0, "{result:?}");
}

fn request(query: &str, limit: usize) -> SearchRequest {
    SearchRequest {
        query: query.into(),
        must_contain: None,
        since: None,
        source_types: vec![],
        limit,
    }
}

fn execute(store: &Store, request: &SearchRequest) -> commonplace::domain::search::SearchResult {
    search::search(
        &store.root,
        request,
        &mut Embedding::default(),
        &mut Ranker::default(),
    )
    .unwrap()
}

#[test]
fn both_real_candidate_paths_contribute_deduplicate_and_rerank_with_bounds() {
    let store = Store::new();
    let mut inputs: Vec<_> = (0..64)
        .map(|i| input(&format!("semantic-{i}"), "related concept", "note", None))
        .collect();
    inputs.push(input("lexical", "lexical needle", "note", None));
    publish(&store.root, inputs);
    let request = request("needle", 50);
    let session = SqliteDatabase::read(&store.root).unwrap();
    let filters = request.validate().unwrap();
    let lexical =
        queries::lexical(session.connection(), &request.lexical_query(), &filters).unwrap();
    let dense = queries::vector(session.connection(), &vector(0), &filters).unwrap();
    assert_eq!(lexical.ids, vec![PassageId::new(65).unwrap()]);
    assert!(!dense.ids.contains(&lexical.ids[0]));
    assert_eq!(dense.ids.len(), CANDIDATE_LIMIT);
    assert!(dense.truncated);
    let mut ranker = Ranker::default();
    let result = search::search(
        &store.root,
        &request,
        &mut Embedding::default(),
        &mut ranker,
    )
    .unwrap();
    assert_eq!(ranker.calls, [64]);
    assert!(result.truncated);
    assert_eq!(result.items.len(), 50);
    assert!(
        result
            .items
            .iter()
            .any(|item| item.evidence.source_key == "lexical")
    );
    assert!(
        result
            .items
            .iter()
            .any(|item| item.evidence.source_key.starts_with("semantic-"))
    );
    let ids: std::collections::BTreeSet<_> = result
        .items
        .iter()
        .map(|item| item.evidence.passage_id)
        .collect();
    assert_eq!(ids.len(), result.items.len());
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        serde_json::to_value(execute(&store, &request)).unwrap()
    );
    let mut reversed = Ranker {
        scores: Some((0..64).map(|i| i as f32).collect()),
        ..Default::default()
    };
    let reranked = search::search(
        &store.root,
        &request,
        &mut Embedding::default(),
        &mut reversed,
    )
    .unwrap();
    assert_ne!(
        reranked.items[0].evidence.passage_id,
        result.items[0].evidence.passage_id
    );
}

#[test]
fn candidate_cutoff_ties_and_result_edges_have_exact_truncation() {
    let store = Store::new();
    let request = request("needle", 50);
    assert!(!execute(&store, &request).truncated);
    publish(
        &store.root,
        (1..=50)
            .map(|i| input(&i.to_string(), "needle", "note", None))
            .collect(),
    );
    assert!(!execute(&store, &request).truncated);
    for limit in [0, 1, 49, 50] {
        let result = execute(&store, &self::request("needle", limit));
        assert_eq!(result.items.len(), limit);
        assert_eq!(result.truncated, limit < 50);
        assert!(result.items.iter().enumerate().all(
            |(i, item)| item.rank == i + 1 && item.evidence.passage_id.value() == i as i64 + 1
        ));
    }
    for count in [63, 64, 65] {
        let start = if count == 63 { 51 } else { count };
        publish(
            &store.root,
            (start..=count)
                .map(|i| input(&i.to_string(), "needle", "note", None))
                .collect(),
        );
        let session = SqliteDatabase::read(&store.root).unwrap();
        let filters = request.validate().unwrap();
        for candidates in [
            queries::lexical(session.connection(), &request.lexical_query(), &filters).unwrap(),
            queries::vector(session.connection(), &vector(0), &filters).unwrap(),
        ] {
            assert_eq!(candidates.ids.len(), count.min(64));
            assert_eq!(candidates.truncated, count == 65);
            assert_eq!(
                candidates
                    .ids
                    .iter()
                    .map(|id| id.value())
                    .collect::<Vec<_>>(),
                (1..=count.min(64) as i64).collect::<Vec<_>>()
            );
        }
    }
    let mut too_large = request;
    too_large.limit = 51;
    assert_eq!(too_large.validate().unwrap_err().code(), "limit_exceeded");
    too_large.limit = usize::MAX;
    assert_eq!(too_large.validate().unwrap_err().code(), "limit_exceeded");
}

#[test]
fn filters_precede_both_cutoffs_and_preserve_nanoseconds_offsets_and_nulls() {
    let store = Store::new();
    let mut inputs: Vec<_> = (0..70)
        .map(|i| {
            input(
                &format!("excluded-{i}"),
                "needle",
                "other",
                Some("2026-09-21T00:00:00Z"),
            )
        })
        .collect();
    for (key, kind, time) in [
        ("whole", "note", Some("2026-09-21T00:00:00Z")),
        ("nano", "note", Some("2026-09-20T19:00:00.000000001-05:00")),
        ("fraction", "recap", Some("2026-09-21T00:00:00.1000Z")),
        ("unknown", "note", None),
        ("case", "Note", Some("2026-09-21T00:00:00.2Z")),
    ] {
        inputs.push(input(key, "needle", kind, time));
    }
    publish(&store.root, inputs);
    let mut request = request("needle", 50);
    request.since = Some("2026-09-21T00:00:00.000000001Z".into());
    request.source_types = vec!["note".into(), "recap".into(), "note".into()];
    let session = SqliteDatabase::read(&store.root).unwrap();
    let assert_paths = |request: &SearchRequest, expected: &[i64]| {
        let filters = request.validate().unwrap();
        let lexical =
            queries::lexical(session.connection(), &request.lexical_query(), &filters).unwrap();
        let dense = queries::vector(session.connection(), &vector(0), &filters).unwrap();
        for candidates in [lexical, dense] {
            assert_eq!(
                candidates
                    .ids
                    .iter()
                    .map(|id| id.value())
                    .collect::<Vec<_>>(),
                expected
            );
            assert!(!candidates.truncated);
        }
    };
    assert_paths(&request, &[72, 73]);
    let result = execute(&store, &request);
    assert_eq!(
        result.items[0].evidence.occurred_at.as_deref(),
        Some("2026-09-21T00:00:00.000000001Z")
    );
    assert_eq!(
        result.items[1].evidence.occurred_at.as_deref(),
        Some("2026-09-21T00:00:00.1Z")
    );
    request.since = Some("2026-09-21T00:00:00.100000000Z".into());
    assert_paths(&request, &[73]);
    request.since = Some("2026-09-21T00:00:00.100000001Z".into());
    assert_paths(&request, &[]);
    request.since = None;
    assert_paths(&request, &[71, 72, 73, 74]);
}

#[test]
fn required_phrase_precedes_both_candidate_limits_and_preserves_exact_citations() {
    let store = Store::new();
    let mut inputs: Vec<_> = (0..70)
        .map(|i| input(&format!("irrelevant-{i}"), "needle", "note", None))
        .collect();
    inputs.push(input("account", "lexical needle ACME Corp", "note", None));
    publish(&store.root, inputs);
    let mut request = request("needle", 50);
    request.must_contain = Some("acme CORP".into());
    let session = SqliteDatabase::read(&store.root).unwrap();
    let filters = request.validate().unwrap();
    for candidates in [
        queries::lexical(session.connection(), &request.lexical_query(), &filters).unwrap(),
        queries::vector(session.connection(), &vector(0), &filters).unwrap(),
    ] {
        assert_eq!(candidates.ids, [PassageId::new(71).unwrap()]);
        assert!(!candidates.truncated);
    }
    let result = execute(&store, &request);
    assert_eq!(result.items.len(), 1);
    assert!(!result.truncated);
    assert_eq!(result.items[0].evidence.text, "lexical needle ACME Corp");
    assert_eq!(
        serde_json::to_value(&result.items[0].evidence).unwrap(),
        serde_json::to_value(
            evidence::passage(session.connection(), result.items[0].evidence.passage_id).unwrap()
        )
        .unwrap()
    );
    request.limit = 0;
    assert!(execute(&store, &request).truncated);
    request.must_contain = Some("no evidence of this phrase".into());
    let empty = execute(&store, &request);
    assert!(empty.items.is_empty());
    assert!(!empty.truncated);
    request.must_contain = None;
    request.limit = 50;
    assert_eq!(execute(&store, &request).items.len(), 50);
    drop(session);
    publish(
        &store.root,
        (0..64)
            .map(|i| input(&format!("matching-{i}"), "needle ACME Corp", "note", None))
            .collect(),
    );
    request.must_contain = Some("acme corp".into());
    let session = SqliteDatabase::read(&store.root).unwrap();
    let filters = request.validate().unwrap();
    for candidates in [
        queries::lexical(session.connection(), &request.lexical_query(), &filters).unwrap(),
        queries::vector(session.connection(), &vector(0), &filters).unwrap(),
    ] {
        assert_eq!(candidates.ids.len(), CANDIDATE_LIMIT);
        assert!(candidates.truncated);
        assert!(candidates.ids.iter().all(|id| id.value() >= 71));
    }
    let result = execute(&store, &request);
    assert_eq!(result.items.len(), 50);
    assert!(result.truncated);
}

#[test]
fn required_phrase_is_passage_only_not_title_metadata_or_other_passages() {
    let store = Store::new();
    let mut title_only = input("title", "needle unrelated", "note", None);
    let document = title_only.document.as_mut().unwrap();
    document.title = Some("ACME Corp".into());
    document
        .metadata
        .insert("account".into(), json!("ACME Corp"));
    let split_source = format!("{}\n\nneedle ACME Corp", "needle ".repeat(146));
    publish(
        &store.root,
        vec![title_only, input("split", &split_source, "note", None)],
    );
    let mut request = request("needle", 50);
    request.must_contain = Some("ACME Corp".into());
    let session = SqliteDatabase::read(&store.root).unwrap();
    let filters = request.validate().unwrap();
    for candidates in [
        queries::lexical(session.connection(), &request.lexical_query(), &filters).unwrap(),
        queries::vector(session.connection(), &vector(0), &filters).unwrap(),
    ] {
        assert_eq!(candidates.ids.len(), 1);
        let passage = evidence::passage(session.connection(), candidates.ids[0]).unwrap();
        assert_eq!(passage.source_key, "split");
        assert_eq!(passage.text, "needle ACME Corp");
        assert_eq!(passage.start_byte, 1024);
    }
}

#[test]
fn required_phrase_uses_literal_contiguous_unicode_lowercase_without_normalization() {
    let store = Store::new();
    publish(
        &store.root,
        vec![
            input("unicode", "needle ÉCRAN Straße ΟΣ", "note", None),
            input("decomposed", "needle e\u{301}cran STRASSE σ", "note", None),
            input("literal", "needle \"%_.* OR\" ACME  Corp", "note", None),
            input("ordinary", "needle ACME Corp", "note", None),
            input("nul", "needle A\0BC", "note", None),
        ],
    );
    let session = SqliteDatabase::read(&store.root).unwrap();
    for (phrase, expected) in [
        ("écran", vec!["unicode"]),
        ("e\u{301}cran", vec!["decomposed"]),
        ("straße", vec!["unicode"]),
        ("STRASSE", vec!["decomposed"]),
        ("Σ", vec!["decomposed"]),
        ("ς", vec!["unicode"]),
        ("\"%_.* OR\"", vec!["literal"]),
        ("ACME  Corp", vec!["literal"]),
        ("ACME Corp", vec!["ordinary"]),
        (" ACME Corp ", vec![]),
        ("[a-z]+", vec![]),
        ("CRAN", vec!["decomposed", "unicode"]),
        ("bc", vec!["nul"]),
    ] {
        let mut request = request("needle", 50);
        request.must_contain = Some(phrase.into());
        let filters = request.validate().unwrap();
        for candidates in [
            queries::lexical(session.connection(), &request.lexical_query(), &filters).unwrap(),
            queries::vector(session.connection(), &vector(0), &filters).unwrap(),
        ] {
            let mut keys: Vec<_> = candidates
                .ids
                .into_iter()
                .map(|id| {
                    evidence::passage(session.connection(), id)
                        .unwrap()
                        .source_key
                })
                .collect();
            keys.sort();
            assert_eq!(keys, expected, "{phrase}");
        }
    }
}

#[test]
fn overlapping_boundary_evidence_keeps_account_and_decision_and_distinct_results() {
    let store = Store::new();
    let phrase = "ACME Corp approved budget";
    let mut text = format!("{} {phrase}. ", "x".repeat(535));
    text.push_str(&"y".repeat(1100 - text.len()));
    assert!(!text[..550].contains(phrase));
    assert!(!text[550..].contains(phrase));
    publish(
        &store.root,
        vec![input("boundary-account", &text, "note", None)],
    );
    let mut request = request("approved budget", 10);
    request.must_contain = Some("ACME Corp".into());
    let result = execute(&store, &request);
    assert_eq!(result.items.len(), 2);
    assert!(!result.truncated);
    let session = SqliteDatabase::read(&store.root).unwrap();
    let filters = request.validate().unwrap();
    for candidates in [
        queries::lexical(session.connection(), &request.lexical_query(), &filters).unwrap(),
        queries::vector(session.connection(), &vector(0), &filters).unwrap(),
    ] {
        assert_eq!(candidates.ids.len(), 2);
        assert_ne!(candidates.ids[0], candidates.ids[1]);
        assert!(!candidates.truncated);
    }
    for item in &result.items {
        let passage = &item.evidence;
        assert!(passage.text.contains(phrase));
        assert_eq!(passage.text, &text[passage.start_byte..passage.end_byte]);
        assert!(passage.text.len() <= 1024);
    }
    assert_eq!(result.items[0].rank, 1);
    assert_eq!(result.items[1].rank, 2);
    assert_ne!(
        result.items[0].evidence.passage_id,
        result.items[1].evidence.passage_id
    );
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        serde_json::to_value(execute(&store, &request)).unwrap()
    );
    request.limit = 1;
    let limited = execute(&store, &request);
    assert_eq!(limited.items.len(), 1);
    assert!(limited.truncated);
    assert_eq!(
        limited.items[0].evidence.passage_id,
        result.items[0].evidence.passage_id
    );
}

#[test]
fn exact_hydration_and_one_snapshot_survive_current_revision_replacement() {
    let store = Store::new();
    let original = "needle\r\n\r\nExact \0e\u{301}🦀";
    publish(
        &store.root,
        vec![input(
            "source",
            original,
            "note",
            Some("2026-01-01T00:00:00Z"),
        )],
    );
    let root = store.root.clone();
    let mut model = Embedding {
        on_embed: Some(Box::new(move || {
            publish(&root, vec![input("source", "replacement", "recap", None)])
        })),
        ..Default::default()
    };
    let mut historical_request = request("needle", 10);
    historical_request.since = Some("2026-01-01T00:00:00Z".into());
    let result = search::search(
        &store.root,
        &historical_request,
        &mut model,
        &mut Ranker::default(),
    )
    .unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(
        result
            .temporal_filter
            .as_ref()
            .unwrap()
            .coverage
            .eligible_dated,
        1
    );
    assert_eq!(
        result
            .temporal_filter
            .as_ref()
            .unwrap()
            .coverage
            .excluded_unknown,
        0
    );
    let citation = &result.items[0].evidence;
    assert_eq!(citation.text, original);
    let session = SqliteDatabase::read(&store.root).unwrap();
    assert_eq!(
        serde_json::to_value(citation).unwrap(),
        serde_json::to_value(evidence::passage(session.connection(), citation.passage_id).unwrap())
            .unwrap()
    );
    let revision = evidence::revision(session.connection(), citation.revision_id).unwrap();
    assert_eq!(
        revision.text.get(citation.start_byte..citation.end_byte),
        Some(citation.text.as_str())
    );
    queries::validate_representation(session.connection()).unwrap();
    let current = execute(&store, &request("needle", 10));
    assert_eq!(current.items.len(), 1);
    assert_eq!(current.items[0].evidence.text, "replacement");
    assert_ne!(current.items[0].evidence.revision_id, citation.revision_id);
    assert_eq!(current.items[0].evidence.source_type, "recap");
    assert_eq!(
        current.items[0].evidence.metadata,
        json!({"nested":{"value":"exact"}})
            .as_object()
            .unwrap()
            .clone()
    );
}

#[test]
fn empty_zero_and_literal_queries_still_run_both_providers() {
    let store = Store::new();
    for query in ["hello", "\"", "OR NOT ( ) * --", "🦀"] {
        let mut embedding = Embedding::default();
        let mut ranker = Ranker::default();
        let result =
            search::search(&store.root, &request(query, 0), &mut embedding, &mut ranker).unwrap();
        assert!(result.items.is_empty());
        assert!(!result.truncated);
        assert_eq!(embedding.calls, 1);
        assert_eq!(ranker.calls, [0]);
    }
    publish(&store.root, vec![input("a", "needle", "note", None)]);
    let mut no_matches = request("needle", 0);
    no_matches.source_types.push("missing".into());
    assert!(!execute(&store, &no_matches).truncated);
    no_matches.source_types.clear();
    assert!(execute(&store, &no_matches).truncated);
    assert!(!execute(&store, &request("\" OR * --", 10)).items.is_empty());
}

#[test]
fn missing_corrupt_or_malformed_required_models_fail_instead_of_lexical_only() {
    let store = Store::new();
    publish(&store.root, vec![input("a", "needle", "note", None)]);
    let cache = tempfile::tempdir().unwrap();
    for limit in [0, 10] {
        let result = search::search(
            &store.root,
            &request("needle", limit),
            &mut Embedding::default(),
            &mut LocalReranker::new(Some(cache.path().into())),
        );
        assert_eq!(result.unwrap_err().code(), "model_unavailable");
    }
    let mut wrong_vectors = Embedding {
        malformed: true,
        ..Default::default()
    };
    assert_eq!(
        search::search(
            &store.root,
            &request("needle", 10),
            &mut wrong_vectors,
            &mut Ranker::default()
        )
        .unwrap_err()
        .code(),
        "model_unavailable"
    );
    for scores in [vec![], vec![1.0, 2.0], vec![f32::NAN], vec![f32::INFINITY]] {
        let mut ranker = Ranker {
            scores: Some(scores),
            ..Default::default()
        };
        assert_eq!(
            search::search(
                &store.root,
                &request("needle", 10),
                &mut Embedding::default(),
                &mut ranker
            )
            .unwrap_err()
            .code(),
            "model_unavailable"
        );
    }
    let mut empty = request("needle", 10);
    empty.source_types.push("not-found".into());
    assert_eq!(
        search::search(
            &store.root,
            &empty,
            &mut Embedding::default(),
            &mut LocalReranker::new(Some(cache.path().into()))
        )
        .unwrap_err()
        .code(),
        "model_unavailable"
    );
    let revision = commonplace::providers::reranker::RERANKER_REVISION;
    std::fs::create_dir(cache.path().join(revision)).unwrap();
    std::fs::write(cache.path().join(revision).join("tokenizer.json"), "{}").unwrap();
    let error = search::search(
        &store.root,
        &request("needle", 10),
        &mut Embedding::default(),
        &mut LocalReranker::new(Some(cache.path().into())),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("incompatible pinned model artifact")
    );
}

#[test]
fn missing_extra_and_wrong_dimension_indexes_fail_even_when_empty_or_filtered_out() {
    for corrupt in [
        "DELETE FROM passage_vectors",
        "DELETE FROM passage_fts",
        "DELETE FROM passage_vectors; DELETE FROM passage_fts",
        "INSERT INTO passage_vectors(passage_id, embedding) SELECT 999, embedding FROM passage_vectors WHERE passage_id = 1; DELETE FROM passage_vectors WHERE passage_id = 1",
        "DROP TABLE passage_vectors; CREATE VIRTUAL TABLE passage_vectors USING vec0(passage_id INTEGER PRIMARY KEY, embedding FLOAT[3])",
    ] {
        let store = Store::new();
        publish(&store.root, vec![input("a", "needle", "note", None)]);
        let mut writer =
            SqliteDatabase::write(&store.root, std::time::Duration::from_secs(2)).unwrap();
        let tx = writer.transaction().unwrap();
        tx.execute_batch(corrupt).unwrap();
        tx.commit().unwrap();
        drop(writer);
        let mut request = request("needle", 0);
        request.source_types.push("not-found".into());
        assert_eq!(
            search::search(
                &store.root,
                &request,
                &mut Embedding::default(),
                &mut Ranker::default()
            )
            .expect_err(corrupt)
            .code(),
            "conflict",
            "{corrupt}"
        );
    }
}

#[test]
fn fusion_ties_and_duplicate_elimination_are_deterministic() {
    let a = PassageId::new(1).unwrap();
    let b = PassageId::new(2).unwrap();
    let c = PassageId::new(3).unwrap();
    assert_eq!(fuse(&[b, a], &[a, b]), [a, b]);
    assert_eq!(fuse(&[c, a], &[a, b]), [a, c, b]);
    assert_eq!(fuse(&[], &[]), []);
}

#[test]
fn malformed_inputs_and_numerical_boundaries_are_explicit_before_loading() {
    let store = Store::new();
    for (query, code) in [
        ("".to_owned(), "invalid_input"),
        (" \t\n".to_owned(), "invalid_input"),
        ("a\0b".to_owned(), "invalid_input"),
        ("x".repeat(4097), "limit_exceeded"),
        ("x ".repeat(65), "limit_exceeded"),
    ] {
        let mut embedding = Embedding::default();
        assert_eq!(
            search::search(
                &store.root,
                &request(&query, 10),
                &mut embedding,
                &mut Ranker::default()
            )
            .unwrap_err()
            .code(),
            code
        );
        assert_eq!(embedding.calls, 0);
    }
    assert!(request(&"x".repeat(4096), 50).validate().is_ok());
    assert!(request(&"x ".repeat(64), 50).validate().is_ok());
    for (phrase, code) in [
        ("".to_owned(), "invalid_input"),
        (" \t\n".to_owned(), "invalid_input"),
        ("a\0b".to_owned(), "invalid_input"),
        ("🦀".repeat(1025), "limit_exceeded"),
        ("x ".repeat(65), "limit_exceeded"),
    ] {
        let mut request = request("query", 10);
        request.must_contain = Some(phrase);
        let mut embedding = Embedding::default();
        assert_eq!(
            search::search(
                &store.root,
                &request,
                &mut embedding,
                &mut Ranker::default()
            )
            .unwrap_err()
            .code(),
            code
        );
        assert_eq!(embedding.calls, 0);
    }
    for phrase in ["🦀".repeat(1024), "x ".repeat(64)] {
        let mut request = request("query", 10);
        request.must_contain = Some(phrase);
        assert!(request.validate().is_ok());
    }
    for (types, code) in [
        (vec!["".into()], "invalid_input"),
        (vec!["a\0b".into()], "invalid_input"),
        (vec!["x".into(); 33], "limit_exceeded"),
        (vec!["x".repeat(4097)], "limit_exceeded"),
    ] {
        let mut request = request("query", 10);
        request.source_types = types;
        assert_eq!(request.validate().unwrap_err().code(), code);
    }
    let mut request = request("query", 10);
    request.source_types = vec!["x".repeat(128); 32];
    assert!(request.validate().is_ok());
    request.since = Some("yesterday".into());
    assert!(
        request
            .validate()
            .unwrap_err()
            .to_string()
            .contains("--since")
    );
    for args in [
        vec!["search", " "],
        vec!["search", "query", "--since", "bad"],
        vec!["search", "query", "--source-type", ""],
        vec!["search", "query", "--must-contain", " \t"],
    ] {
        let error = store.failure(&args, "invalid_input", 2);
        assert_eq!(error["operation"], "search");
        assert_eq!(error["contract_version"], "1");
    }
    store.failure(&["search", "query", "--limit", "51"], "limit_exceeded", 2);
    store.failure(
        &["search", "query", "--must-contain", &"x".repeat(4097)],
        "limit_exceeded",
        2,
    );
    assert_eq!(
        store
            .run(&["search", "query", "--limit", "-1"])
            .status
            .code(),
        Some(2)
    );
    let output = store
        .command()
        .env(
            "COMMONPLACE_MODEL_CACHE",
            store.directory.path().join("missing-models"),
        )
        .args([
            "search",
            "needle",
            "--limit",
            "0",
            "--must-contain",
            "ACME Corp",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "model_unavailable");
    assert!(store.run(&["search", "--help"]).status.success());
    assert!(
        String::from_utf8(store.run(&["search", "--help"]).stdout)
            .unwrap()
            .contains("--must-contain")
    );
}

struct WrongIdentity;
impl EmbeddingModel for WrongIdentity {
    fn identity(&self) -> &'static str {
        "wrong"
    }
    fn embed(&mut self, _: &[&str]) -> Result<Vec<Vec<f32>>> {
        Err(CommonplaceError::ModelUnavailable(
            "must reject identity first".into(),
        ))
    }
}

#[test]
fn incompatible_embedding_identity_fails_before_inference() {
    let store = Store::new();
    let error = search::search(
        &store.root,
        &request("needle", 10),
        &mut WrongIdentity,
        &mut Ranker::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("identity"));
}
