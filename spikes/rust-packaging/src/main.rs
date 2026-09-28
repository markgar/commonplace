use std::path::Path;
use std::sync::Once;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use bytemuck::cast_slice;
use fastembed::{
    EmbeddingModel, RerankInitOptions, RerankerModel, TextEmbedding, TextInitOptions, TextRerank,
};
use lbug::{Connection as GraphConnection, Database as GraphDatabase, SystemConfig};
use rusqlite::{Connection, ffi::sqlite3_auto_extension, params};
use serde_json::{Value, json};
use sqlite_vec::sqlite3_vec_init;

static REGISTER_SQLITE_VEC: Once = Once::new();
type SqliteExtensionEntry = unsafe extern "C" fn(
    *mut rusqlite::ffi::sqlite3,
    *mut *mut std::ffi::c_char,
    *const rusqlite::ffi::sqlite3_api_routines,
) -> std::ffi::c_int;

fn main() -> Result<()> {
    let command = std::env::args().nth(1).unwrap_or_else(|| "all".to_string());
    let root = match std::env::var_os("COMMONPLACE_SPIKE_DATA_DIR") {
        Some(path) => path.into(),
        None => std::env::current_exe()?
            .parent()
            .context("release executable has no parent directory")?
            .join("data"),
    };
    std::fs::create_dir_all(&root)?;

    let mut report = serde_json::Map::new();
    report.insert(
        "platform".into(),
        json!({
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "rust_toolchain": "recorded in the spike report"
        }),
    );
    report.insert("lbug_crate".into(), json!(lbug::VERSION));
    report.insert(
        "lbug_library_source".into(),
        json!(lbug::get_library_source()),
    );
    report.insert(
        "lbug_storage_version".into(),
        json!(lbug::get_storage_version()),
    );

    match command.as_str() {
        "sqlite" => {
            report.insert("sqlite".into(), run_sqlite_vector(&root)?);
        }
        "ladybug" => {
            report.insert("ladybug".into(), run_ladybug(&root)?);
        }
        "models" => {
            report.insert("models".into(), run_models(&root)?);
        }
        "all" => {
            report.insert("sqlite".into(), run_sqlite_vector(&root)?);
            report.insert("ladybug".into(), run_ladybug(&root)?);
            report.insert("models".into(), run_models(&root)?);
        }
        other => bail!("unknown spike command: {other}"),
    }

    println!("{}", serde_json::to_string_pretty(&Value::Object(report))?);
    Ok(())
}

fn run_sqlite_vector(root: &Path) -> Result<Value> {
    REGISTER_SQLITE_VEC.call_once(|| unsafe {
        sqlite3_auto_extension(Some(
            std::mem::transmute::<*const (), SqliteExtensionEntry>(sqlite3_vec_init as *const ()),
        ));
    });

    let database_path = root.join("sqlite-vector.db");
    if database_path.exists() {
        std::fs::remove_file(&database_path)?;
    }
    let mut db = Connection::open(&database_path)?;
    db.execute_batch(
        "
        PRAGMA foreign_keys = ON;
        CREATE TABLE passages (
            passage_id INTEGER PRIMARY KEY,
            text TEXT NOT NULL
        ) STRICT;
        CREATE VIRTUAL TABLE passage_fts USING fts5(
            text,
            content = 'passages',
            content_rowid = 'passage_id',
            tokenize = 'unicode61'
        );
        CREATE VIRTUAL TABLE passage_vectors USING vec0(
            passage_id INTEGER PRIMARY KEY,
            embedding FLOAT[4]
        );
        ",
    )?;

    let vectors = [
        (1_i64, "alpha release plan", [1.0_f32, 0.0, 0.0, 0.0]),
        (2_i64, "beta incident report", [0.0_f32, 1.0, 0.0, 0.0]),
    ];
    let tx = db.transaction()?;
    for (id, text, embedding) in vectors {
        tx.execute(
            "INSERT INTO passages(passage_id, text) VALUES (?1, ?2)",
            params![id, text],
        )?;
        tx.execute(
            "INSERT INTO passage_fts(rowid, text) VALUES (?1, ?2)",
            params![id, text],
        )?;
        tx.execute(
            "INSERT INTO passage_vectors(passage_id, embedding) VALUES (?1, ?2)",
            params![id, cast_slice(&embedding)],
        )?;
    }
    tx.commit()?;

    let query = [0.9_f32, 0.1, 0.0, 0.0];
    let nearest: (i64, f64) = db.query_row(
        "
        SELECT passage_id, distance
        FROM passage_vectors
        WHERE embedding MATCH ?1
        ORDER BY distance
        LIMIT 1
        ",
        [cast_slice(&query)],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if nearest.0 != 1 {
        bail!("vector lookup returned passage {}, expected 1", nearest.0);
    }

    let lexical_id: i64 = db.query_row(
        "
        SELECT rowid
        FROM passage_fts
        WHERE passage_fts MATCH 'incident'
        ",
        [],
        |row| row.get(0),
    )?;
    if lexical_id != 2 {
        bail!("FTS lookup returned passage {lexical_id}, expected 2");
    }

    let tx = db.transaction()?;
    tx.execute("DELETE FROM passage_vectors WHERE passage_id = 2", [])?;
    tx.execute("DELETE FROM passage_fts WHERE rowid = 2", [])?;
    tx.execute("DELETE FROM passages WHERE passage_id = 2", [])?;
    tx.commit()?;

    let remaining: i64 =
        db.query_row("SELECT count(*) FROM passage_vectors", [], |row| row.get(0))?;
    if remaining != 1 {
        bail!("transactional vector deletion left {remaining} rows");
    }

    let versions: (String, String) =
        db.query_row("SELECT sqlite_version(), vec_version()", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;

    Ok(json!({
        "status": "pass",
        "sqlite_version": versions.0,
        "sqlite_vec_version": versions.1,
        "ddl": "vec0(passage_id INTEGER PRIMARY KEY, embedding FLOAT[4])",
        "nearest_passage_id": nearest.0,
        "nearest_distance": nearest.1,
        "fts_passage_id": lexical_id,
        "transactional_delete": true,
        "database": database_path
    }))
}

fn run_ladybug(root: &Path) -> Result<Value> {
    let graph_path = root.join("ladybug");
    if graph_path.exists() {
        if graph_path.is_dir() {
            std::fs::remove_dir_all(&graph_path)?;
        } else {
            std::fs::remove_file(&graph_path)?;
        }
    }

    {
        let db = GraphDatabase::new(
            &graph_path,
            SystemConfig::default()
                .buffer_pool_size(256 * 1024 * 1024)
                .max_num_threads(2),
        )?;
        let conn = GraphConnection::new(&db)?;
        conn.query(
            "CREATE NODE TABLE Entity(entity_id INT64, name STRING, PRIMARY KEY(entity_id));",
        )?;
        conn.query(
            "CREATE REL TABLE RELATIONSHIP(
                FROM Entity TO Entity,
                knowledge_item_id INT64,
                predicate STRING
            );",
        )?;
        conn.query("CREATE (:Entity {entity_id: 1, name: 'Atlas'});")?;
        conn.query("CREATE (:Entity {entity_id: 2, name: 'Roadmap'});")?;
        conn.query(
            "MATCH (a:Entity), (b:Entity)
             WHERE a.entity_id = 1 AND b.entity_id = 2
             CREATE (a)-[:RELATIONSHIP {
                 knowledge_item_id: 7,
                 predicate: 'depends_on'
             }]->(b);",
        )?;
        let result = conn.query(
            "MATCH (a:Entity)-[r:RELATIONSHIP]->(b:Entity)
             RETURN a.entity_id, r.knowledge_item_id, r.predicate, b.entity_id;",
        )?;
        let rendered = result.to_string();
        if !rendered.contains("depends_on") || !rendered.contains('7') {
            bail!("Ladybug query did not return the inserted relationship");
        }
        let mutation =
            conn.prepare("MATCH (a:Entity) WHERE a.entity_id = 1 SET a.name = 'Changed';")?;
        if mutation.is_read_only() {
            bail!("Ladybug classified a mutation as read-only");
        }
    }

    let read_only_db = GraphDatabase::new(
        &graph_path,
        SystemConfig::default()
            .buffer_pool_size(256 * 1024 * 1024)
            .max_num_threads(2)
            .read_only(true),
    )?;
    let read_only = GraphConnection::new(&read_only_db)?;
    eprintln!("ladybug: testing native read-only enforcement");
    let mutation_rejected = read_only
        .query("MATCH (a:Entity) WHERE a.entity_id = 1 SET a.name = 'Changed';")
        .is_err();
    if !mutation_rejected {
        bail!("read-only Ladybug database accepted a mutation");
    }

    eprintln!("ladybug: testing explicit interrupt");
    let interrupted_connection = GraphConnection::new(&read_only_db)?;
    interrupted_connection.set_query_timeout(5_000);
    let interrupt_started = Instant::now();
    let interrupt_result = std::thread::scope(|scope| {
        let handle = scope.spawn(|| {
            interrupted_connection
                .query(
                    "UNWIND range(1, 100000) AS x
                     UNWIND range(1, 100000) AS y
                    RETURN sum((x % 1000) * (y % 1000));",
                )
                .map(|result| result.to_string())
        });
        std::thread::sleep(Duration::from_millis(100));
        interrupted_connection.interrupt()?;
        Ok::<_, lbug::Error>(handle.join().expect("query thread panicked"))
    })?;
    let interrupt_elapsed = interrupt_started.elapsed();
    let interrupted = interrupt_result.is_err();
    if !interrupted || interrupt_elapsed >= Duration::from_secs(2) {
        bail!(
            "Ladybug interrupt did not cancel promptly: error={interrupted}, elapsed={interrupt_elapsed:?}"
        );
    }

    eprintln!("ladybug: testing native query timeout");
    let timeout_connection = GraphConnection::new(&read_only_db)?;
    timeout_connection.set_query_timeout(50);
    let timeout_started = Instant::now();
    let timed_out = timeout_connection
        .query(
            "UNWIND range(1, 100000) AS x
             UNWIND range(1, 100000) AS y
             RETURN sum((x % 1000) * (y % 1000));",
        )
        .is_err();
    let timeout_elapsed = timeout_started.elapsed();
    if !timed_out || timeout_elapsed >= Duration::from_secs(2) {
        bail!(
            "Ladybug timeout did not cancel promptly: error={timed_out}, elapsed={timeout_elapsed:?}"
        );
    }

    Ok(json!({
        "status": "pass",
        "crate_version": lbug::VERSION,
        "library_source": lbug::get_library_source(),
        "storage_version": lbug::get_storage_version(),
        "graph_path": graph_path,
        "read_only_mutation_rejected": mutation_rejected,
        "interrupt_cancelled_query": interrupted,
        "interrupt_elapsed_ms": interrupt_elapsed.as_millis(),
        "query_timeout_cancelled_query": timed_out,
        "query_timeout_elapsed_ms": timeout_elapsed.as_millis()
    }))
}

fn run_models(root: &Path) -> Result<Value> {
    let cache = root.join("fastembed-cache-jina");
    std::fs::create_dir_all(&cache)?;

    let mut embedding = TextEmbedding::try_new(
        TextInitOptions::new(EmbeddingModel::AllMiniLML6V2)
            .with_cache_dir(cache.clone())
            .with_show_download_progress(true),
    )
    .context("initialize local embedding model")?;
    let embeddings = embedding.embed(
        vec![
            "release planning notes",
            "incident response timeline",
            "project dependency",
        ],
        Some(2),
    )?;
    if embeddings.len() != 3 || embeddings.iter().any(|vector| vector.len() != 384) {
        bail!("embedding output shape did not match 3 x 384");
    }
    let embedding_norms: Vec<f32> = embeddings
        .iter()
        .map(|vector| vector.iter().map(|value| value * value).sum::<f32>().sqrt())
        .collect();
    if embedding_norms
        .iter()
        .any(|norm| (norm - 1.0).abs() > 0.001)
    {
        bail!("embedding output was not normalized: {embedding_norms:?}");
    }

    let mut reranker = TextRerank::try_new(
        RerankInitOptions::new(RerankerModel::JINARerankerV1TurboEn)
            .with_cache_dir(cache.clone())
            .with_show_download_progress(true),
    )
    .context("initialize local reranker")?;
    let reranked = reranker.rerank(
        "what depends on the project?",
        vec![
            "The release depends on the Atlas project.",
            "Lunch is scheduled for noon.",
        ],
        true,
        Some(2),
    )?;
    if reranked.len() != 2 || reranked[0].index != 0 {
        bail!("reranker did not rank the relevant passage first");
    }

    Ok(json!({
        "status": "pass",
        "crate_version": "7.1.0",
        "embedding_model": "sentence-transformers/all-MiniLM-L6-v2",
        "embedding_dimensions": embeddings[0].len(),
        "embedding_normalized": true,
        "embedding_batch_size": 2,
        "reranker_model": "jinaai/jina-reranker-v1-turbo-en",
        "reranker_top_index": reranked[0].index,
        "reranker_top_score": reranked[0].score,
        "cache": cache
    }))
}
