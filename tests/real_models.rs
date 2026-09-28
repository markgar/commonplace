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

    store.apply(
        &json!({"entity_types":[{"name":"person"},{"name":"lead"}]}),
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
        {"kind":"type_membership","entity":{"ref":"ada"},"entity_type":"lead","support":[]}
    ]}));
    let recorded = execute(&["record", input.to_str().unwrap()]);
    assert_eq!(recorded["result"]["knowledge_version"], 1);
    assert_eq!(recorded["result"]["summary"]["memberships_created"], 2);
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
    }
    execute(&["graph", "rebuild"]);
    assert_eq!(
        execute(&["graph", "query", citation_query])["result"],
        citations
    );
    assert_eq!(execute(&["init"])["status"], "unchanged");
    println!(
        "PASS pinned local model; v2 multi-file public binary, unchanged/content/metadata revisions, exact UTF-8 get, current FTS/vector parity, strict override and default HF cache hits, unchanged graph bytes during ingest, public multiply-typed record/get/SPARQL/rebuild/reopen and exact old/new citation parity; cache={}",
        cache.display()
    );
}
