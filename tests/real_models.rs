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
    println!(
        "PASS pinned local model; multi-file public binary, unchanged/content/metadata revisions, exact UTF-8 get, current FTS/vector parity, strict override and default HF cache hits; cache={}",
        cache.display()
    );
}
