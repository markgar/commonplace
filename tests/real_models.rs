mod common;

use std::path::PathBuf;

use commonplace::providers::embeddings::{
    EMBEDDING_DIMENSIONS, EMBEDDING_IDENTITY, EMBEDDING_REPOSITORY, EMBEDDING_REVISION,
    EmbeddingModel, LocalEmbeddingModel,
};
use commonplace::storage::database::SqliteDatabase;
use serde_json::{Value, json};

use common::Store;

#[test]
#[ignore = "requires prepared pinned COMMONPLACE_MODEL_CACHE; explicitly run offline, never downloads or skips"]
fn real_model_search_offline() {
    let cache = PathBuf::from(
        std::env::var_os("COMMONPLACE_MODEL_CACHE")
            .expect("set COMMONPLACE_MODEL_CACHE to the prepared P2 revision/file cache"),
    );
    let store = Store::new();
    let execute = |args: &[&str]| -> Value {
        let output = store
            .command()
            .env("COMMONPLACE_MODEL_CACHE", &cache)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        serde_json::from_slice(&output.stdout).unwrap()
    };
    assert_eq!(
        execute(&["search", "Lantern"])["result"],
        json!({"items":[],"truncated":false})
    );
    let corpus = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/meeting-notes");
    let notes = corpus.join("Obsidian Notes");
    let recaps = corpus.join("AI Summaries");
    assert_eq!(
        execute(&[
            "ingest",
            notes.to_str().unwrap(),
            "--source-type",
            "note",
            "--occurred-at",
            "2026-09-28T00:00:00.000000001Z"
        ])["result"]["summary"]["added"],
        25
    );
    assert_eq!(
        execute(&["ingest", recaps.to_str().unwrap(), "--source-type", "recap"])["result"]["summary"]
            ["added"],
        11
    );
    assert_eq!(
        execute(&[
            "ingest",
            notes.to_str().unwrap(),
            "--source-type",
            "note",
            "--occurred-at",
            "2026-09-28T00:00:00.000000001Z"
        ])["result"]["summary"]["unchanged"],
        25
    );
    let session = SqliteDatabase::read(&store.root).unwrap();
    assert_eq!(
        session
            .connection()
            .query_row("SELECT count(*) FROM documents", [], |row| row
                .get::<_, usize>(0))
            .unwrap(),
        36
    );
    assert_eq!(session.connection().query_row(
        "SELECT count(*) FROM document_revisions WHERE title IN ('README.md','OVERVIEW.md')", [],
        |row| row.get::<_, usize>(0)).unwrap(), 0);
    drop(session);
    let cases: Value =
        serde_json::from_str(include_str!("../fixtures/search-relevance.json")).unwrap();
    for (index, case) in cases.as_array().unwrap().iter().enumerate() {
        let query = case["query"].as_str().unwrap();
        let limit = case["top"].as_u64().unwrap().to_string();
        let result = execute(&["search", query, "--limit", &limit]);
        assert_eq!(result["operation"], "search");
        assert_eq!(result["contract_version"], "1");
        assert_eq!(result["status"], "complete");
        assert_eq!(result["result"]["truncated"], true);
        let items = result["result"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 5);
        assert!(
            items
                .iter()
                .any(|item| case["evidence"].as_array().unwrap().iter().any(
                    |expected| item["title"] == expected["title"]
                        && item["text"]
                            .as_str()
                            .unwrap()
                            .contains(expected["contains"].as_str().unwrap())
                )),
            "required top-5 source and quote missing: {case}\n{result}"
        );
        let mut ids = std::collections::BTreeSet::new();
        for (rank, item) in items.iter().enumerate() {
            assert_eq!(item["rank"], rank + 1);
            let id = item["passage_id"].as_str().unwrap();
            assert!(ids.insert(id));
            let exact = execute(&["get", id]);
            let mut citation = exact["result"].as_object().unwrap().clone();
            citation.remove("kind");
            citation.insert("rank".into(), json!(rank + 1));
            assert_eq!(item, &Value::Object(citation));
            let revision = execute(&["get", item["revision_id"].as_str().unwrap()]);
            assert_eq!(
                revision["result"]["text"].as_str().unwrap().get(
                    item["start_byte"].as_u64().unwrap() as usize
                        ..item["end_byte"].as_u64().unwrap() as usize
                ),
                item["text"].as_str()
            );
        }
        println!("PASS relevance case {}: query={query:?}; top5={}", index + 1,
            serde_json::to_string(&items.iter().map(|item| json!({"rank":item["rank"],"title":item["title"],"passage_id":item["passage_id"]})).collect::<Vec<_>>()).unwrap());
        if index == 0 {
            assert_eq!(execute(&["search", query, "--limit", &limit]), result);
        }
    }
    let filtered = execute(&[
        "search",
        "Lantern",
        "--source-type",
        "note",
        "--since",
        "2026-09-27T19:00:00.000000001-05:00",
        "--limit",
        "50",
    ]);
    assert!(!filtered["result"]["items"].as_array().unwrap().is_empty());
    for item in filtered["result"]["items"].as_array().unwrap() {
        assert_eq!(item["source_type"], "note");
        assert_eq!(item["occurred_at"], "2026-09-28T00:00:00.000000001Z");
    }
    for args in [
        vec!["search", "Lantern", "--source-type", "absent"],
        vec![
            "search",
            "Lantern",
            "--since",
            "2026-09-28T00:00:00.000000002Z",
        ],
    ] {
        assert_eq!(
            execute(&args)["result"],
            json!({"items":[],"truncated":false})
        );
    }
    assert_eq!(
        execute(&["search", "Lantern", "--limit", "0"])["result"],
        json!({"items":[],"truncated":true})
    );

    let models: Value =
        serde_json::from_str(include_str!("../spikes/rust-packaging/models.json")).unwrap();
    let embedding_only = store.directory.path().join("embedding-only");
    for file in models[0]["files"].as_array().unwrap() {
        let name = file[0].as_str().unwrap();
        let destination = embedding_only.join(EMBEDDING_REVISION).join(name);
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::hard_link(cache.join(EMBEDDING_REVISION).join(name), destination).unwrap();
    }
    for args in [
        vec!["search", "Lantern"],
        vec!["search", "Lantern", "--limit", "0"],
        vec!["search", "Lantern", "--source-type", "absent"],
    ] {
        let output = store
            .command()
            .env("COMMONPLACE_MODEL_CACHE", &embedding_only)
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], "model_unavailable");
        assert!(
            error["error"]["message"]
                .as_str()
                .unwrap()
                .contains(commonplace::providers::reranker::RERANKER_REVISION)
        );
    }
    let hf_home = store.directory.path().join("search-hf-home");
    let stock = hf_hub::Cache::new(hf_home.join("hub"));
    for model in models.as_array().unwrap() {
        let revision = model["revision"].as_str().unwrap();
        let repo = stock.repo(hf_hub::Repo::with_revision(
            model["repository"].as_str().unwrap().into(),
            hf_hub::RepoType::Model,
            revision.into(),
        ));
        repo.create_ref(revision).unwrap();
        for file in model["files"].as_array().unwrap() {
            let name = file[0].as_str().unwrap();
            let destination = repo.pointer_path(revision).join(name);
            std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
            std::fs::hard_link(cache.join(revision).join(name), destination).unwrap();
        }
    }
    let output = store
        .command()
        .env_remove("COMMONPLACE_MODEL_CACHE")
        .env("HF_HOME", &hf_home)
        .args(["search", "Lantern", "--limit", "0"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["result"],
        json!({"items":[],"truncated":true})
    );

    use commonplace::providers::reranker::{LocalReranker, Reranker};
    let mut reranker = LocalReranker::new(Some(cache.clone()));
    let texts = [
        "Support escalation guide",
        "Community volunteer supplies",
        "Lantern pilot schedule",
    ];
    let scores = reranker.rerank("Lantern pilot schedule", &texts).unwrap();
    assert_eq!(
        scores,
        reranker.rerank("Lantern pilot schedule", &texts).unwrap()
    );
    assert_eq!(scores.len(), texts.len());
    assert!(scores.iter().all(|score| score.is_finite()));
    assert!(scores[2] > scores[0] && scores[2] > scores[1]);
    println!(
        "PASS public-binary pinned-model search: 36 source documents, 4/4 top-5 relevance cases, repeat order, exact get citations, filters, zero/empty output, required reranker failure, verified stock cache hit and retained reranker session; cache={}",
        cache.display()
    );
}

#[test]
#[ignore = "requires prepared pinned COMMONPLACE_MODEL_CACHE; explicitly run offline, never downloads or skips"]
fn real_model_stream_ingest_get_offline() {
    let cache = PathBuf::from(
        std::env::var_os("COMMONPLACE_MODEL_CACHE")
            .expect("set COMMONPLACE_MODEL_CACHE to the prepared pinned cache"),
    );
    let store = Store::new();
    let graph_before = store.graph_files();
    let run = |args: &[&str], bytes: &[u8]| -> (i32, Value) {
        let mut command = store.command();
        command.args(args).env("COMMONPLACE_MODEL_CACHE", &cache);
        let output = common::with_stdin(command, bytes);
        assert!(output.stderr.is_empty(), "{output:?}");
        (
            output.status.code().unwrap(),
            serde_json::from_slice(&output.stdout).unwrap(),
        )
    };
    let text = "\u{feff}Riley approved the draft.\r\n\r\nExact \0 e\u{301} \u{1f980}\n";
    let (exit, added) = run(
        &["ingest", "--stdin", "--source-key", "notes/42"],
        text.as_bytes(),
    );
    assert_eq!(exit, 0);
    let first = &added["result"]["items"][0];
    assert!(!first["passage_ids"].as_array().unwrap().is_empty());
    let original = store.success(&["get", first["revision_id"].as_str().unwrap()]);
    assert_eq!(original["result"]["text"], text);
    let mut record = json!({"source_key":"notes/42","text":text});
    let (exit, same) = run(
        &["ingest", "--jsonl", "-"],
        &serde_json::to_vec(&record).unwrap(),
    );
    assert_eq!(exit, 0);
    assert_eq!(same["result"]["summary"]["unchanged"], 1);
    assert_eq!(
        same["result"]["items"][0]["revision_id"],
        first["revision_id"]
    );
    record["metadata"] = json!({"project":"riley"});
    let (exit, changed) = run(
        &["ingest", "--jsonl", "-"],
        &serde_json::to_vec(&record).unwrap(),
    );
    assert_eq!(exit, 0);
    assert_eq!(changed["result"]["summary"]["updated"], 1);
    assert_eq!(
        changed["result"]["items"][0]["document_id"],
        first["document_id"]
    );
    assert_ne!(
        changed["result"]["items"][0]["revision_id"],
        first["revision_id"]
    );
    let lines = format!(
        "{}\n{{bad}}\n{}\n{}\n",
        record,
        record,
        json!({"source_key":"notes/43","text":"Review the draft on Friday."})
    );
    let (exit, partial) = run(&["ingest", "--jsonl", "-"], lines.as_bytes());
    assert_eq!(exit, 2);
    assert_eq!(
        partial["result"]["summary"],
        json!({"added":1,"updated":0,"unchanged":1,"failed":2})
    );
    assert_eq!(
        store.success(&["get", first["revision_id"].as_str().unwrap()]),
        original
    );
    let session = SqliteDatabase::read(&store.root).unwrap();
    let ids = |sql: &str| {
        session
            .connection()
            .prepare(sql)
            .unwrap()
            .query_map([], |row| row.get::<_, i64>(0))
            .unwrap()
            .map(std::result::Result::unwrap)
            .collect::<Vec<_>>()
    };
    let current = ids("SELECT passage_id FROM passages p JOIN document_revisions r USING(revision_id)
        WHERE r.revision_number = (SELECT max(revision_number) FROM document_revisions WHERE document_id = r.document_id)
        ORDER BY passage_id");
    assert_eq!(ids("SELECT rowid FROM passage_fts ORDER BY rowid"), current);
    assert_eq!(
        ids("SELECT passage_id FROM passage_vectors ORDER BY passage_id"),
        current
    );
    assert_eq!(store.graph_files(), graph_before);
}

#[test]
#[ignore = "requires prepared pinned COMMONPLACE_MODEL_CACHE; explicitly run offline, never downloads or skips"]
fn real_model_ingest_get_offline() {
    let cache = PathBuf::from(
        std::env::var_os("COMMONPLACE_MODEL_CACHE")
            .expect("set COMMONPLACE_MODEL_CACHE to the prepared P2 revision/file cache"),
    );
    let mut model = LocalEmbeddingModel::new(Some(cache.clone()), 2);
    assert_eq!(model.identity(), EMBEDDING_IDENTITY);
    let vectors = model
        .embed(&["release planning notes", "incident response timeline"])
        .unwrap();
    assert_eq!(vectors.len(), 2);
    for vector in &vectors {
        assert_eq!(vector.len(), EMBEDDING_DIMENSIONS);
        assert!((vector.iter().map(|v| v * v).sum::<f32>().sqrt() - 1.0).abs() < 0.001);
    }
    let repeated = model.embed(&["release planning notes"]).unwrap();
    assert!(
        repeated[0]
            .iter()
            .zip(&vectors[0])
            .all(|(a, b)| (a - b).abs() < 0.00001)
    );

    let store = Store::new();
    assert_eq!(
        store.success(&["init"])["result"]["format"],
        "commonplace-store/2"
    );
    let graph_before_ingest = store.graph_files();
    let notes = store.directory.path().join("notes");
    std::fs::create_dir_all(notes.join("nested")).unwrap();
    let a = notes.join("a.md");
    let b = notes.join("nested/b.txt");
    let text = format!(
        "\u{feff}Release planning\r\n\r\nNUL\0e\u{301}🦀\n\n{}",
        "Long paragraph 🦀 ".repeat(180)
    );
    std::fs::write(&a, &text).unwrap();
    std::fs::write(&b, "incident response timeline").unwrap();
    std::fs::write(notes.join("empty.md"), "").unwrap();
    std::fs::write(notes.join("ignored.json"), "not a scanned source").unwrap();
    let execute = |args: &[&str]| -> Value {
        let output = store
            .command()
            .env("COMMONPLACE_MODEL_CACHE", &cache)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    };
    let added = execute(&[
        "ingest",
        notes.to_str().unwrap(),
        "--recursive",
        "--embedding-batch-size",
        "2",
    ]);
    assert_eq!(added["result"]["summary"]["added"], 3);
    let doc = added["result"]["items"][0]["document_id"].as_str().unwrap();
    let old_revision = added["result"]["items"][0]["revision_id"].as_str().unwrap();
    let streamed_text = "The local release decision is ready for review.";
    let stream = || {
        let mut command = store.command();
        command.env("COMMONPLACE_MODEL_CACHE", &cache).args([
            "ingest",
            "--stdin",
            "--source-key",
            "release/decision",
        ]);
        let output = common::with_stdin(command, streamed_text.as_bytes());
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    let streamed = stream();
    assert_eq!(streamed["result"]["summary"]["added"], 1);
    let stream_passage = streamed["result"]["items"][0]["passage_ids"][0].clone();
    assert_eq!(
        execute(&["get", stream_passage.as_str().unwrap()])["result"]["text"],
        streamed_text
    );
    let same_stream = stream();
    assert_eq!(same_stream["result"]["summary"]["unchanged"], 1);
    assert_eq!(
        same_stream["result"]["items"][0]["revision_id"],
        streamed["result"]["items"][0]["revision_id"]
    );
    let unchanged = execute(&["ingest", a.to_str().unwrap(), b.to_str().unwrap()]);
    assert_eq!(unchanged["result"]["summary"]["unchanged"], 2);
    assert_eq!(unchanged["result"]["items"][0]["document_id"], doc);
    let metadata_revision = execute(&[
        "ingest",
        a.to_str().unwrap(),
        "--metadata",
        "{\"z\":true,\"a\":{\"n\":2}}",
    ]);
    assert_eq!(metadata_revision["result"]["summary"]["updated"], 1);
    std::fs::write(&a, "revised release planning").unwrap();
    let updated = execute(&["ingest", a.to_str().unwrap()]);
    assert_eq!(updated["result"]["summary"]["updated"], 1);
    let record = execute(&["get", doc]);
    assert_eq!(
        record["result"]["revision_ids"].as_array().unwrap().len(),
        3
    );
    assert_eq!(
        record["result"]["current_revision_id"],
        updated["result"]["items"][0]["revision_id"]
    );
    for (id, expected_text) in [
        (old_revision, text.as_str()),
        (
            metadata_revision["result"]["items"][0]["revision_id"]
                .as_str()
                .unwrap(),
            text.as_str(),
        ),
        (
            updated["result"]["items"][0]["revision_id"]
                .as_str()
                .unwrap(),
            "revised release planning",
        ),
    ] {
        let revision = execute(&["get", id]);
        assert_eq!(revision["result"]["text"], expected_text);
        let mut offset = 0;
        for passage_id in revision["result"]["passage_ids"].as_array().unwrap() {
            let passage = execute(&["get", passage_id.as_str().unwrap()]);
            let evidence = &passage["result"];
            let start = evidence["start_byte"].as_u64().unwrap() as usize;
            let end = evidence["end_byte"].as_u64().unwrap() as usize;
            assert_eq!(start, offset);
            assert_eq!(evidence["text"], &expected_text[start..end]);
            assert_eq!(evidence["revision_id"], id);
            assert_eq!(evidence["document_id"], doc);
            assert_eq!(evidence["source_type"], "file");
            offset = end;
        }
        assert_eq!(offset, expected_text.len());
    }
    let session = SqliteDatabase::read(&store.root).unwrap();
    let ids = |sql| {
        session
            .connection()
            .prepare(sql)
            .unwrap()
            .query_map([], |row| row.get::<_, i64>(0))
            .unwrap()
            .map(std::result::Result::unwrap)
            .collect::<Vec<_>>()
    };
    let current = ids("SELECT passage_id FROM passages p JOIN document_revisions r USING(revision_id)
        WHERE revision_number=(SELECT max(revision_number) FROM document_revisions WHERE document_id=r.document_id)
        ORDER BY passage_id");
    assert_eq!(ids("SELECT rowid FROM passage_fts ORDER BY rowid"), current);
    assert_eq!(
        ids("SELECT passage_id FROM passage_vectors ORDER BY passage_id"),
        current
    );
    let query = model.embed(&["incident response timeline"]).unwrap();
    let bytes: Vec<u8> = query[0]
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    let nearest: i64 = session
        .connection()
        .query_row(
            "SELECT passage_id FROM passage_vectors WHERE embedding MATCH ?1 AND k=1",
            [bytes],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        json!(format!("passage:{nearest}")),
        added["result"]["items"][2]["passage_ids"][0]
    );
    let hf_home = store.directory.path().join("hf-home");
    let hub = hf_hub::Cache::new(hf_home.join("hub")).repo(hf_hub::Repo::with_revision(
        EMBEDDING_REPOSITORY.into(),
        hf_hub::RepoType::Model,
        EMBEDDING_REVISION.into(),
    ));
    hub.create_ref(EMBEDDING_REVISION).unwrap();
    let snapshot_path = hub.pointer_path(EMBEDDING_REVISION);
    std::fs::create_dir_all(&snapshot_path).unwrap();
    for name in [
        "model.onnx",
        "tokenizer.json",
        "config.json",
        "special_tokens_map.json",
        "tokenizer_config.json",
    ] {
        std::fs::copy(
            cache.join(EMBEDDING_REVISION).join(name),
            snapshot_path.join(name),
        )
        .unwrap();
    }
    let default_cache = store
        .command()
        .env_remove("COMMONPLACE_MODEL_CACHE")
        .env("HF_HOME", &hf_home)
        .args([
            "ingest",
            b.to_str().unwrap(),
            "--title",
            "Default cache hit",
        ])
        .output()
        .unwrap();
    assert!(default_cache.status.success(), "{default_cache:?}");
    assert!(default_cache.stderr.is_empty());
    let default_result: Value = serde_json::from_slice(&default_cache.stdout).unwrap();
    assert_eq!(default_result["result"]["summary"]["updated"], 1);
    drop(session);
    assert_eq!(store.graph_files(), graph_before_ingest);
    assert_eq!(
        execute(&["graph", "rebuild"])["result"],
        json!({"knowledge_version":0})
    );
    assert_eq!(
        execute(&["graph", "query", "SELECT ?s WHERE { ?s ?p ?o }"])["result"]["rows"],
        json!([])
    );

    let searched = execute(&["search", "release planning"]);
    let items = searched["result"]["items"].as_array().unwrap();
    assert!(!items.is_empty());
    for (rank, item) in items.iter().enumerate() {
        assert_eq!(item["rank"], rank + 1);
        let exact = execute(&["get", item["passage_id"].as_str().unwrap()]);
        let mut expected = exact["result"].as_object().unwrap().clone();
        expected.remove("kind");
        expected.insert("rank".into(), json!(rank + 1));
        assert_eq!(item, &Value::Object(expected));
    }
    assert_eq!(execute(&["search", "release planning"]), searched);
    store.apply(
        &json!({"entity_types":[{"name":"person"},{"name":"lead"}],"predicates":[
            {"name":"reports_to","object_kind":"entity","subject_types":["person"],"object_types":["lead"]},
            {"name":"decision","object_kind":"string","subject_types":["person"]}
        ]}),
        false,
    );
    let passages = [
        added["result"]["items"][0]["passage_ids"][0]
            .as_str()
            .unwrap(),
        updated["result"]["items"][0]["passage_ids"][0]
            .as_str()
            .unwrap(),
    ];
    let input = store.input(&json!({"created_by":"offline-acceptance","items":[
        {"kind":"entity","ref":"ada","name":"Ada"},
        {"kind":"type_membership","entity":{"ref":"ada"},"entity_type":"person",
            "support":passages.iter().map(|id| json!({"passage_id":id})).collect::<Vec<_>>()},
        {"kind":"type_membership","entity":{"ref":"ada"},"entity_type":"lead","support":[]},
        {"kind":"entity","ref":"lead","name":"Grace"},
        {"kind":"type_membership","entity":{"ref":"lead"},"entity_type":"lead"},
        {"kind":"fact","subject":{"ref":"ada"},"predicate":"reports_to","object":{"entity":{"ref":"lead"}},
            "support":passages.iter().map(|id| json!({"passage_id":id})).collect::<Vec<_>>()},
        {"kind":"fact","subject":{"ref":"ada"},"predicate":"decision","object":{"literal":"approved"},
            "support":passages.iter().map(|id| json!({"passage_id":id})).collect::<Vec<_>>()}
    ]}));
    let recorded = execute(&["record", input.to_str().unwrap()]);
    assert_eq!(recorded["result"]["knowledge_version"], 1);
    assert_eq!(recorded["result"]["summary"]["memberships_created"], 3);
    assert_eq!(recorded["result"]["summary"]["facts_created"], 2);
    assert_eq!(
        execute(&["get", "knowledge:4"])["result"]["object"],
        json!({"entity_id":"entity:2"})
    );
    assert_eq!(
        execute(&["get", "knowledge:5"])["result"]["object"],
        json!({
            "literal_kind":"string","literal":"approved","literal_json":"\"approved\""
        })
    );
    assert_eq!(
        execute(&["get", "entity:1"])["result"]["active_fact_ids"],
        json!(["knowledge:4", "knowledge:5"])
    );
    assert_eq!(
        execute(&["get", "entity:1"])["result"]["active_type_membership_ids"],
        json!(["knowledge:1", "knowledge:2"])
    );
    assert_eq!(
        execute(&["get", "knowledge:1"])["result"]["support"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        execute(&["get", "knowledge:2"])["result"]["support"],
        json!([])
    );
    let citation_query = "PREFIX c: <urn:commonplace:property:>
        SELECT ?p ?r ?d ?text ?start ?end WHERE {
            <urn:commonplace:knowledge:1> c:evidence ?p .
            ?p c:revision ?r; c:text ?text; c:start_byte ?start; c:end_byte ?end .
            ?r c:document ?d
        } ORDER BY ?p";
    let published = execute(&["graph", "query", citation_query])["result"].clone();
    let fact_query = "PREFIX c:<urn:commonplace:property:> SELECT ?k ?subject ?predicate ?object ?p ?text WHERE {
        ?k c:kind \"fact\"; c:subject ?subject; c:predicate ?predicate; c:object ?object; c:evidence ?p.
        ?p c:text ?text } ORDER BY ?k ?p";
    let fact_graph = execute(&["graph", "query", fact_query])["result"].clone();
    assert_eq!(fact_graph["rows"].as_array().unwrap().len(), 4);
    assert_eq!(
        fact_graph["rows"][0][0]["value"],
        "urn:commonplace:knowledge:4"
    );
    assert_eq!(
        fact_graph["rows"][0][3],
        json!({"type":"uri","value":"urn:commonplace:entity:2"})
    );
    assert_eq!(
        fact_graph["rows"][2][3],
        json!({"type":"literal","value":"approved",
        "datatype":"http://www.w3.org/2001/XMLSchema#string","language":null})
    );
    assert_eq!(
        execute(&["graph", "rebuild"])["result"],
        json!({"knowledge_version":1})
    );
    let citations = execute(&["graph", "query", citation_query])["result"].clone();
    assert_eq!(citations, published);
    assert_eq!(citations["rows"].as_array().unwrap().len(), 2);
    for passage in passages {
        let exact = execute(&["get", passage]);
        let exact = &exact["result"];
        let uri = format!("urn:commonplace:{passage}");
        let row = citations["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row[0]["value"] == uri)
            .unwrap();
        assert_eq!(
            row[1]["value"],
            format!("urn:commonplace:{}", exact["revision_id"].as_str().unwrap())
        );
        assert_eq!(row[2]["value"], format!("urn:commonplace:{doc}"));
        assert_eq!(row[3]["value"], exact["text"]);
        assert_eq!(row[4]["value"], exact["start_byte"].to_string());
        assert_eq!(row[5]["value"], exact["end_byte"].to_string());
        for id in ["knowledge:4", "knowledge:5"] {
            let got = execute(&["get", id]);
            let support = got["result"]["support"]
                .as_array()
                .unwrap()
                .iter()
                .find(|support| support["passage_id"] == passage)
                .unwrap();
            for field in [
                "text",
                "start_byte",
                "end_byte",
                "document_id",
                "revision_id",
                "metadata",
            ] {
                assert_eq!(support[field], exact[field]);
            }
            let row = fact_graph["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| {
                    row[0]["value"] == format!("urn:commonplace:{id}") && row[4]["value"] == uri
                })
                .unwrap();
            assert_eq!(row[5]["value"], exact["text"]);
        }
    }
    execute(&["graph", "rebuild"]);
    assert_eq!(
        execute(&["graph", "query", citation_query])["result"],
        citations
    );
    assert_eq!(execute(&["init"])["status"], "unchanged");
    assert_eq!(
        execute(&["graph", "query", fact_query])["result"],
        fact_graph
    );
    let removed_document = execute(&["get", doc])["result"].clone();
    let source_key = removed_document["source_key"].as_str().unwrap();
    assert!(source_key.starts_with("file://"));
    let mut removed_passages = Vec::new();
    for revision in removed_document["revision_ids"].as_array().unwrap() {
        let revision = execute(&["get", revision.as_str().unwrap()]);
        removed_passages.extend(
            revision["result"]["passage_ids"]
                .as_array()
                .unwrap()
                .iter()
                .cloned(),
        );
    }
    let unrelated_doc = added["result"]["items"][2]["document_id"].as_str().unwrap();
    let unrelated_before = execute(&["get", unrelated_doc]);
    let unrelated_revision = unrelated_before["result"]["current_revision_id"]
        .as_str()
        .unwrap();
    let unrelated_passage =
        execute(&["get", unrelated_revision])["result"]["passage_ids"][0].clone();
    let input = store.input(&json!({"items":[
        {"kind":"type_membership","entity":{"id":"entity:1"},"entity_type":"person",
            "support":[{"passage_id":passages[0]},{"passage_id":unrelated_passage}]},
        {"kind":"type_membership","entity":{"id":"entity:1"},"entity_type":"lead",
            "support":[{"passage_id":unrelated_passage}]},
        {"kind":"fact","subject":{"id":"entity:1"},"predicate":"reports_to","object":{"entity":{"id":"entity:2"}},
            "support":[{"passage_id":passages[0]},{"passage_id":unrelated_passage}]},
        {"kind":"fact","subject":{"id":"entity:1"},"predicate":"decision","object":{"literal":"approved"},
            "support":[{"passage_id":passages[0]},{"passage_id":unrelated_passage}]},
        {"kind":"fact","subject":{"id":"entity:1"},"predicate":"reports_to","object":{"entity":{"id":"entity:2"}},
            "support":[{"passage_id":unrelated_passage}]},
        {"kind":"fact","subject":{"id":"entity:1"},"predicate":"decision","object":{"literal":"approved"},
            "support":[{"passage_id":unrelated_passage}]}
    ]}));
    let extra = execute(&["record", input.to_str().unwrap()]);
    let mixed_id = extra["result"]["items"][0]["knowledge_id"]
        .as_str()
        .unwrap();
    let unrelated_id = extra["result"]["items"][1]["knowledge_id"]
        .as_str()
        .unwrap();
    let untouched_knowledge = execute(&["get", unrelated_id]);
    let fact_ids = ["knowledge:4", "knowledge:5"]
        .into_iter()
        .chain(
            extra["result"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .skip(2)
                .map(|item| item["knowledge_id"].as_str().unwrap()),
        )
        .collect::<Vec<_>>();
    let facts_before = fact_ids
        .iter()
        .map(|id| execute(&["get", id])["result"].clone())
        .collect::<Vec<_>>();
    let input = store.input(&json!({"items":[{
        "kind":"fact","subject":{"id":"entity:1"},"predicate":"decision",
        "object":{"literal":"reviewed before removal"},"support":[{"passage_id":stream_passage}]
    }]}));
    let pre_removal = execute(&["record", input.to_str().unwrap()]);
    let pre_removal_id = pre_removal["result"]["items"][0]["knowledge_id"]
        .as_str()
        .unwrap();
    let mut pre_removal_history = execute(&["get", pre_removal_id])["result"].clone();
    let input = store.input(&json!({"knowledge_ids":[pre_removal_id]}));
    let withdrawal = execute(&["withdraw", input.to_str().unwrap()]);
    pre_removal_history["withdrawn_at"] = withdrawal["result"]["items"][0]["withdrawn_at"].clone();
    assert_eq!(
        execute(&["get", pre_removal_id])["result"],
        pre_removal_history
    );
    let query = format!("SELECT ?p ?o WHERE {{ <urn:commonplace:{pre_removal_id}> ?p ?o }}");
    assert_eq!(
        execute(&["graph", "query", &query])["result"]["rows"],
        json!([])
    );
    let expected_version = withdrawal["result"]["knowledge_version"].as_i64().unwrap() + 1;
    let removed = execute(&["remove", "--source-key", source_key, "--json"]);
    assert_eq!(
        removed["result"],
        json!({
            "source_key":source_key,"document_id":doc,"deleted_revisions":3,
            "deleted_passages":removed_passages.len(),"detached_evidence":9,
            "affected_knowledge_ids":["knowledge:1","knowledge:4","knowledge:5",mixed_id,fact_ids[2],fact_ids[3]],"knowledge_version":expected_version
        })
    );
    for id in std::iter::once(json!(doc))
        .chain(
            removed_document["revision_ids"]
                .as_array()
                .unwrap()
                .iter()
                .cloned(),
        )
        .chain(removed_passages.iter().cloned())
    {
        store.failure(&["get", id.as_str().unwrap()], "not_found", 2);
    }
    assert!(a.is_file(), "remove must not delete the original file");
    let sqlite = SqliteDatabase::read(&store.root).unwrap();
    let connection = sqlite.connection();
    let deleted_document_id = doc.strip_prefix("doc:").unwrap().parse::<i64>().unwrap();
    for sql in [
        "SELECT count(*) FROM documents WHERE document_id=?1",
        "SELECT count(*) FROM document_revisions WHERE document_id=?1",
    ] {
        assert_eq!(
            connection
                .query_row(sql, [deleted_document_id], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    for passage in &removed_passages {
        let id = passage
            .as_str()
            .unwrap()
            .strip_prefix("passage:")
            .unwrap()
            .parse::<i64>()
            .unwrap();
        for (table, key) in [
            ("passages", "passage_id"),
            ("passage_fts", "rowid"),
            ("passage_vectors", "passage_id"),
            ("knowledge_item_evidence", "passage_id"),
        ] {
            assert_eq!(
                connection
                    .query_row(
                        &format!("SELECT count(*) FROM {table} WHERE {key}=?1"),
                        [id],
                        |r| r.get::<_, i64>(0)
                    )
                    .unwrap(),
                0,
                "{table}: {id}"
            );
        }
    }
    let current_ids: Vec<i64> = connection.prepare(
        "SELECT passage_id FROM passages p JOIN document_revisions r USING(revision_id)
        WHERE revision_number=(SELECT max(revision_number) FROM document_revisions WHERE document_id=r.document_id)
        ORDER BY passage_id"
    ).unwrap().query_map([], |r|r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
    for sql in [
        "SELECT rowid FROM passage_fts ORDER BY rowid",
        "SELECT passage_id FROM passage_vectors ORDER BY passage_id",
    ] {
        let indexed: Vec<i64> = connection
            .prepare(sql)
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(indexed, current_ids);
    }
    drop(sqlite);
    let retained = execute(&["get", "knowledge:1"]);
    assert_eq!(retained["result"]["support"], json!([]));
    assert_eq!(retained["result"]["withdrawn_at"], Value::Null);
    let mixed = execute(&["get", mixed_id]);
    assert_eq!(mixed["result"]["support"].as_array().unwrap().len(), 1);
    assert_eq!(
        mixed["result"]["support"][0]["passage_id"],
        unrelated_passage
    );
    assert_eq!(execute(&["get", unrelated_id]), untouched_knowledge);
    assert_eq!(execute(&["get", unrelated_doc]), unrelated_before);
    for (index, (id, before)) in fact_ids.iter().zip(&facts_before).enumerate() {
        let mut expected = before.clone();
        expected["support"]
            .as_array_mut()
            .unwrap()
            .retain(|support| support["source_key"] != source_key);
        let got = execute(&["get", id])["result"].clone();
        assert_eq!(got, expected);
        assert_eq!(got["withdrawn_at"], Value::Null);
        assert_eq!(
            got["support"].as_array().unwrap().len(),
            usize::from(index >= 2)
        );
        if index >= 2 {
            assert_eq!(got["support"][0]["passage_id"], unrelated_passage);
        }
    }
    let retained_fact_query = "PREFIX c:<urn:commonplace:property:>
        SELECT ?k ?s ?predicate ?object ?json ?p ?text WHERE {
            ?k c:kind \"fact\"; c:subject ?s; c:predicate ?predicate; c:object ?object .
            OPTIONAL {?k c:literal_json ?json} OPTIONAL {?k c:evidence ?p . ?p c:text ?text}
        } ORDER BY ?k";
    let retained_facts = execute(&["graph", "query", retained_fact_query])["result"].clone();
    assert_eq!(retained_facts["rows"].as_array().unwrap().len(), 6);
    let unrelated_exact = execute(&["get", unrelated_passage.as_str().unwrap()])["result"].clone();
    for (index, id) in fact_ids.iter().enumerate() {
        let row = retained_facts["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row[0]["value"] == format!("urn:commonplace:{id}"))
            .unwrap();
        assert_eq!(row[1]["value"], "urn:commonplace:entity:1");
        if index % 2 == 0 {
            assert_eq!(
                row[3],
                json!({"type":"uri","value":"urn:commonplace:entity:2"})
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
            assert_eq!(row[6], Value::Null);
        } else {
            assert_eq!(
                row[5]["value"],
                format!("urn:commonplace:{}", unrelated_passage.as_str().unwrap())
            );
            assert_eq!(row[6]["value"], unrelated_exact["text"]);
        }
    }
    assert_eq!(
        execute(&["graph", "query", citation_query])["result"]["rows"],
        json!([])
    );
    let optional_query = "PREFIX c:<urn:commonplace:property:>
        SELECT ?kind ?p WHERE {<urn:commonplace:knowledge:1> c:kind ?kind
            OPTIONAL {<urn:commonplace:knowledge:1> c:evidence ?p}}";
    let retained_graph = execute(&["graph", "query", optional_query])["result"].clone();
    assert_eq!(retained_graph["rows"].as_array().unwrap().len(), 1);
    assert_eq!(retained_graph["rows"][0][0]["value"], "type_membership");
    assert_eq!(retained_graph["rows"][0][1], Value::Null);
    let mixed_query = format!(
        "PREFIX c:<urn:commonplace:property:>
        SELECT ?p WHERE {{<urn:commonplace:{mixed_id}> c:evidence ?p}}"
    );
    let mixed_graph = execute(&["graph", "query", &mixed_query])["result"].clone();
    assert_eq!(mixed_graph["rows"].as_array().unwrap().len(), 1);
    assert_eq!(
        mixed_graph["rows"][0][0]["value"],
        format!("urn:commonplace:{}", unrelated_passage.as_str().unwrap())
    );
    let searched = execute(&["search", "release planning"]);
    assert!(!searched["result"]["items"].as_array().unwrap().is_empty());
    assert!(
        searched["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["document_id"] != doc
                && !removed_passages.contains(&item["passage_id"]))
    );
    store.failure(&["remove", "--source-key", source_key], "not_found", 2);
    let all_query = "SELECT ?s ?p ?o WHERE {?s ?p ?o} ORDER BY ?s ?p ?o";
    let canonical_graph = execute(&["graph", "query", all_query])["result"].clone();
    assert_eq!(canonical_graph["truncated"], false);
    assert_eq!(
        execute(&["get", "entity:1"])["result"]["active_fact_ids"],
        json!(fact_ids)
    );
    assert_eq!(execute(&["init"])["status"], "unchanged");
    assert_eq!(
        execute(&["graph", "rebuild"])["result"]["knowledge_version"],
        expected_version
    );
    assert_eq!(
        execute(&["graph", "query", optional_query])["result"],
        retained_graph
    );
    assert_eq!(
        execute(&["graph", "query", &mixed_query])["result"],
        mixed_graph
    );
    assert_eq!(
        execute(&["graph", "query", all_query])["result"],
        canonical_graph
    );
    assert_eq!(
        execute(&["graph", "query", retained_fact_query])["result"],
        retained_facts
    );
    let again = execute(&["ingest", a.to_str().unwrap()]);
    assert_eq!(again["result"]["summary"]["added"], 1);
    assert_ne!(again["result"]["items"][0]["document_id"], doc);
    assert_eq!(again["result"]["items"][0]["source_key"], source_key);
    assert_eq!(execute(&["get", "knowledge:1"]), retained);
    assert_eq!(
        execute(&["graph", "query", all_query])["result"],
        canonical_graph
    );
    assert_eq!(
        execute(&["graph", "query", optional_query])["result"],
        retained_graph
    );
    assert_eq!(
        execute(&["graph", "query", retained_fact_query])["result"],
        retained_facts
    );
    for (id, before) in fact_ids.iter().zip(&facts_before) {
        let mut expected = before.clone();
        expected["support"]
            .as_array_mut()
            .unwrap()
            .retain(|support| support["source_key"] != source_key);
        assert_eq!(execute(&["get", id])["result"], expected);
    }
    let fresh_passage = again["result"]["items"][0]["passage_ids"][0].clone();
    let input = store.input(&json!({"created_by":"author","items":[
        {"kind":"fact","subject":{"id":"entity:1"},"predicate":"reports_to",
         "object":{"entity":{"id":"entity:2"}},"support":[{"passage_id":fresh_passage}]},
        {"kind":"fact","subject":{"id":"entity:1"},"predicate":"decision",
         "object":{"literal":"withdraw this decision"},"support":[{"passage_id":fresh_passage}]}
    ]}));
    let recorded = execute(&["record", input.to_str().unwrap()]);
    let cited_ids: Vec<_> = recorded["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["knowledge_id"].as_str().unwrap())
        .collect();
    // Include empty-support facts retained by the earlier source removal.
    let withdrawal_ids = [cited_ids[0], cited_ids[1], "knowledge:4", "knowledge:5"];
    let prior: Vec<_> = withdrawal_ids
        .iter()
        .map(|id| execute(&["get", id])["result"].clone())
        .collect();
    let sources_before = execute(&[
        "get",
        again["result"]["items"][0]["document_id"].as_str().unwrap(),
    ]);
    let search_before = execute(&["search", "release planning"]);
    let input = store.input(&json!({"withdrawn_by":"reviewer","knowledge_ids":withdrawal_ids}));
    let withdrawn = execute(&["withdraw", input.to_str().unwrap(), "--json"]);
    assert_eq!(
        withdrawn["result"]["summary"],
        json!({"items":4,"withdrawn":4})
    );
    assert_eq!(
        withdrawn["result"]["knowledge_version"].as_i64().unwrap(),
        recorded["result"]["knowledge_version"].as_i64().unwrap() + 1
    );
    let timestamp = withdrawn["result"]["items"][0]["withdrawn_at"].clone();
    assert!(timestamp.is_string());
    for (index, (id, before)) in withdrawal_ids.iter().zip(&prior).enumerate() {
        let mut expected = before.clone();
        expected["withdrawn_at"] = timestamp.clone();
        expected["withdrawn_by"] = json!("reviewer");
        assert_eq!(execute(&["get", id])["result"], expected);
        expected.as_object_mut().unwrap().remove("kind");
        expected["index"] = json!(index);
        assert_eq!(withdrawn["result"]["items"][index], expected);
        assert_eq!(
            expected["support"].as_array().unwrap().len(),
            usize::from(index < 2)
        );
    }
    let after_withdrawal = execute(&["graph", "query", all_query])["result"].clone();
    for id in withdrawal_ids {
        assert!(
            !after_withdrawal
                .to_string()
                .contains(&format!("urn:commonplace:{id}\""))
        );
    }
    assert_eq!(execute(&["get", unrelated_id]), untouched_knowledge);
    assert_eq!(
        execute(&[
            "get",
            sources_before["result"]["document_id"].as_str().unwrap()
        ]),
        sources_before
    );
    assert_eq!(execute(&["search", "release planning"]), search_before);
    assert_eq!(
        execute(&["get", "entity:1"])["result"]["active_fact_ids"],
        json!(&fact_ids[2..])
    );
    execute(&["init"]);
    execute(&["graph", "rebuild"]);
    assert_eq!(
        execute(&["graph", "query", all_query])["result"],
        after_withdrawal
    );
    let removed_again = execute(&["remove", "--source-key", source_key]);
    assert_eq!(
        removed_again["result"]["affected_knowledge_ids"],
        json!(cited_ids)
    );
    for id in cited_ids {
        let history = execute(&["get", id])["result"].clone();
        assert_eq!(history["support"], json!([]));
        assert_eq!(history["withdrawn_at"], timestamp);
        assert_eq!(history["withdrawn_by"], "reviewer");
    }
    execute(&["graph", "rebuild"]);
    assert_eq!(
        execute(&["graph", "query", all_query])["result"],
        after_withdrawal
    );
    let final_ingest = execute(&["ingest", a.to_str().unwrap()]);
    assert_eq!(final_ingest["result"]["summary"]["added"], 1);
    assert_ne!(
        final_ingest["result"]["items"][0]["document_id"],
        again["result"]["items"][0]["document_id"]
    );
    let final_search = execute(&["search", "release planning"]);
    assert!(
        !final_search["result"]["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(execute(&["init"])["status"], "unchanged");
    assert_eq!(execute(&["search", "release planning"]), final_search);
    assert_eq!(
        execute(&["graph", "query", all_query])["result"],
        after_withdrawal
    );
    assert_eq!(
        execute(&["get", pre_removal_id])["result"],
        pre_removal_history
    );
    println!(
        "PASS pinned local model; v2 directory/direct/streamed public ingestion, unchanged/content/metadata revisions, exact UTF-8 get, ordered hybrid citations before authoring, current FTS/vector parity, strict override and default HF cache hits, unchanged graph bytes during ingest, public multiply-typed entity + relationship + literal decision record/get/SPARQL/rebuild/reopen and exact old/new citation parity; cited withdrawal before removal with retained history; explicit file-key removal of all revisions/index/evidence, retained active membership/relationship/literal facts with empty/mixed support and unrelated citations, public search exclusion, reopen/rebuild, and new reingest identity; atomic cited and empty-support withdrawal with exact history, active IDs/RDF absence, unchanged sources/search/unrelated knowledge, reopen/rebuild and later removal lifecycle parity; final reingest/reopen search and graph parity; cache={}",
        cache.display()
    );
}
