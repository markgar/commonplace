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
