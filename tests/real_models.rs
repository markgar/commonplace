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

    // P5b owns authoring commands; cite real ingested passages via a committed fixture.
    store.apply(&json!({"entity_types":[{"name":"person"}]}), false);
    let passages = [
        added["result"]["items"][0]["passage_ids"][0]
            .as_str()
            .unwrap(),
        updated["result"]["items"][0]["passage_ids"][0]
            .as_str()
            .unwrap(),
    ];
    let mut db = store.database();
    db.execute_batch("PRAGMA foreign_keys=ON").unwrap();
    let tx = db.transaction().unwrap();
    tx.execute_batch(
        "INSERT INTO entities VALUES (1,'Ada','2026-01-01T00:00:00Z',NULL);
         INSERT INTO knowledge_items VALUES
             (1,'type_membership',1,'2026-01-01T00:00:00Z',NULL,NULL,NULL);
         INSERT INTO entity_type_memberships VALUES (1,1,1);
         UPDATE store_state SET knowledge_version=1;",
    )
    .unwrap();
    for passage in passages {
        tx.execute(
            "INSERT INTO knowledge_item_evidence VALUES (1,?1)",
            [passage
                .strip_prefix("passage:")
                .unwrap()
                .parse::<i64>()
                .unwrap()],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    drop(db);
    store.failure(
        &["graph", "query", "SELECT ?s WHERE { ?s ?p ?o }"],
        "graph_unavailable",
        1,
    );
    let citation_query = "PREFIX c: <urn:commonplace:property:>
        SELECT ?p ?r ?d ?text ?start ?end WHERE {
            <urn:commonplace:knowledge:1> c:evidence ?p .
            ?p c:revision ?r; c:text ?text; c:start_byte ?start; c:end_byte ?end .
            ?r c:document ?d
        } ORDER BY ?p";
    assert_eq!(
        execute(&["graph", "rebuild"])["result"],
        json!({"knowledge_version":1})
    );
    let citations = execute(&["graph", "query", citation_query])["result"].clone();
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
    }
    execute(&["graph", "rebuild"]);
    assert_eq!(
        execute(&["graph", "query", citation_query])["result"],
        citations
    );
    assert_eq!(execute(&["init"])["status"], "unchanged");
    println!(
        "PASS pinned local model; v2 multi-file public binary, unchanged/content/metadata revisions, exact UTF-8 get, current FTS/vector parity, strict override and default HF cache hits, unchanged graph bytes during ingest, empty/membership rebuild and exact old/new citation parity; cache={}",
        cache.display()
    );
}
