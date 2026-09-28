use std::{path::Path, sync::Once};

use anyhow::{Result, ensure};
use bytemuck::cast_slice;
use rusqlite::{Connection, ffi::sqlite3_auto_extension, params};
use serde_json::{Value, json};
use sqlite_vec::sqlite3_vec_init;

static REGISTER: Once = Once::new();
type ExtensionEntry = unsafe extern "C" fn(
    *mut rusqlite::ffi::sqlite3,
    *mut *mut std::ffi::c_char,
    *const rusqlite::ffi::sqlite3_api_routines,
) -> std::ffi::c_int;

pub fn probe(root: &Path, embeddings: Option<&[Vec<f32>]>) -> Result<Value> {
    REGISTER.call_once(|| unsafe {
        assert_eq!(
            sqlite3_auto_extension(Some(std::mem::transmute::<*const (), ExtensionEntry>(
                sqlite3_vec_init as *const (),
            ))),
            0
        );
    });
    let fixture = vec![vec![1.0_f32, 0.0, 0.0, 0.0], vec![0.0, 1.0, 0.0, 0.0]];
    let vectors = embeddings.unwrap_or(&fixture);
    ensure!(vectors.len() >= 2, "need two vectors");
    let dimension = vectors[0].len();
    let path = root.join("sqlite-vector.db");
    ensure!(
        !path.exists(),
        "probe database already exists: {}",
        path.display()
    );
    let mut db = Connection::open(&path)?;
    db.execute_batch(&format!(
        "CREATE TABLE passages(passage_id INTEGER PRIMARY KEY, text TEXT NOT NULL) STRICT;
         CREATE VIRTUAL TABLE passage_fts USING fts5(text);
         CREATE VIRTUAL TABLE passage_vectors USING vec0(
             passage_id INTEGER PRIMARY KEY, embedding FLOAT[{dimension}]);"
    ))?;
    let tx = db.transaction()?;
    for (index, vector) in vectors.iter().take(2).enumerate() {
        let id = i64::try_from(index + 1)?;
        let text = ["alpha release plan", "beta incident report"][index];
        tx.execute("INSERT INTO passages VALUES (?1, ?2)", params![id, text])?;
        tx.execute(
            "INSERT INTO passage_fts(rowid, text) VALUES (?1, ?2)",
            params![id, text],
        )?;
        tx.execute(
            "INSERT INTO passage_vectors VALUES (?1, ?2)",
            params![id, cast_slice(vector)],
        )?;
    }
    tx.commit()?;
    let nearest: i64 = db.query_row(
        "SELECT passage_id FROM passage_vectors WHERE embedding MATCH ?1 ORDER BY distance LIMIT 1",
        [cast_slice(&vectors[0])],
        |row| row.get(0),
    )?;
    ensure!(nearest == 1, "real vector binding failed");
    let lexical: i64 = db.query_row(
        "SELECT rowid FROM passage_fts WHERE passage_fts MATCH 'incident'",
        [],
        |r| r.get(0),
    )?;
    ensure!(lexical == 2, "FTS lookup failed");
    ensure!(
        db.execute(
            "INSERT INTO passage_vectors VALUES (3, ?1)",
            [cast_slice(&[1.0_f32])]
        )
        .is_err(),
        "wrong vector dimensions accepted"
    );
    for commit in [false, true] {
        let tx = db.transaction()?;
        tx.execute_batch(
            "DELETE FROM passage_vectors WHERE passage_id = 2;
            DELETE FROM passage_fts WHERE rowid = 2;
            DELETE FROM passages WHERE passage_id = 2;",
        )?;
        if commit {
            tx.commit()?;
        } else {
            tx.rollback()?;
        }
        for table in ["passages", "passage_fts", "passage_vectors"] {
            let count: i64 =
                db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
            ensure!(
                count == if commit { 1 } else { 2 },
                "transaction parity failed: {table}"
            );
        }
    }
    let versions: (String, String) =
        db.query_row("SELECT sqlite_version(), vec_version()", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
    Ok(
        json!({"status":"pass", "sqlite":versions.0, "sqlite_vec":versions.1,
        "dimensions":dimension, "nearest_passage_id":nearest, "fts_passage_id":lexical,
        "dimension_rejection":true, "transaction_rollback_and_commit":true}),
    )
}
