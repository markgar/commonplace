use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
    process::Command,
    time::Instant,
};

use anyhow::{Context, Result, ensure};
use grafeo::{GrafeoDB, Role};
use rusqlite::Connection;
use serde_json::json;
use sha2::{Digest, Sha256};

const MAGIC: &[u8; 8] = b"P2ENV001";
const KEY: &str = "knowledge_version";

fn lock(path: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?)
}

fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}

fn envelope(db: &GrafeoDB, version: u64) -> Result<Vec<u8>> {
    let snapshot = db.export_snapshot()?;
    let mut bytes = MAGIC.to_vec();
    bytes.extend(version.to_le_bytes());
    bytes.extend((snapshot.len() as u64).to_le_bytes());
    bytes.extend(snapshot);
    let digest = Sha256::digest(&bytes);
    bytes.extend(digest);
    Ok(bytes)
}

fn open_envelope(path: &Path) -> Result<(GrafeoDB, u64)> {
    let bytes = fs::read(path)?;
    ensure!(
        bytes.len() >= 56 && &bytes[..8] == MAGIC,
        "incompatible envelope"
    );
    let end = bytes.len() - 32;
    ensure!(
        Sha256::digest(&bytes[..end])[..] == bytes[end..],
        "envelope checksum"
    );
    let version = u64::from_le_bytes(bytes[8..16].try_into()?);
    let len = usize::try_from(u64::from_le_bytes(bytes[16..24].try_into()?))?;
    ensure!(len == end - 24, "envelope length");
    Ok((GrafeoDB::import_snapshot(&bytes[24..end])?, version))
}

fn build(path: &Path, version: u64, nodes: usize, native: bool) -> Result<()> {
    let db = GrafeoDB::new_in_memory();
    for index in 0..nodes {
        db.execute_cypher(&format!("CREATE (:Entity {{id:'entity:{index}'}})"))?;
    }
    if native {
        db.set_application_metadata(KEY.into(), version.to_string())?;
        db.save(path)?;
    } else {
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(&envelope(&db, version)?)?;
        file.sync_all()?;
    }
    db.close()?;
    drop(db);
    File::open(path)?.sync_all()?;
    sync_directory(path.parent().context("candidate parent")?)?;
    Ok(())
}

fn open(path: &Path, native: bool) -> Result<(GrafeoDB, u64)> {
    if native {
        let db = GrafeoDB::open_read_only(path)?;
        let version = db
            .application_metadata(KEY)
            .context("missing durable version")?
            .parse()?;
        Ok((db, version))
    } else {
        open_envelope(path)
    }
}

fn verify(path: &Path, version: u64, nodes: usize, native: bool) -> Result<()> {
    let (db, actual) = open(path, native)?;
    ensure!(actual == version, "knowledge version mismatch");
    let reader = db.session_with_role(Role::ReadOnly);
    let result = reader.execute_cypher_bounded("MATCH (n) RETURN count(n)", HashMap::new(), 1)?;
    ensure!(
        result.scalar::<i64>()? == i64::try_from(nodes)?,
        "projection/version mismatch"
    );
    ensure!(
        reader
            .execute_cypher_bounded("CREATE (:Oops)", HashMap::new(), 1)
            .is_err()
    );
    drop(result);
    drop(reader);
    if native {
        ensure!(
            db.set_application_metadata(KEY.into(), "999".into())
                .is_err()
        );
        let exported = GrafeoDB::import_snapshot(&db.export_snapshot()?)?;
        ensure!(exported.application_metadata(KEY) == Some(version.to_string()));
        exported.close()?;
    }
    db.close()?;
    drop(db);
    Ok(())
}

// Child process intentionally exits without destructors to leave SQLite's transaction uncommitted.
pub fn crash_child(root: &Path, native: bool, activate: bool) -> Result<()> {
    let publication = lock(&root.join("publication.lock"))?;
    publication.lock()?;
    let conn = Connection::open(root.join("truth.sqlite"))?;
    conn.execute_batch("BEGIN IMMEDIATE; UPDATE knowledge SET version=2")?;
    verify(&root.join("candidate.grafeo"), 2, 3, native)?;
    if activate {
        fs::rename(root.join("candidate.grafeo"), root.join("current.grafeo"))?;
        sync_directory(root)?;
    }
    std::process::exit(73);
}

pub fn measure(path: &Path, native: bool) -> Result<serde_json::Value> {
    let publication = lock(
        &path
            .parent()
            .context("fixture parent")?
            .join("publication.lock"),
    )?;
    publication.lock_shared()?;
    let start = Instant::now();
    let (db, version) = open(path, native)?;
    let open_us = start.elapsed().as_micros();
    ensure!(version == 7);
    let snapshot_bytes = db.export_snapshot()?.len();
    db.close()?;
    drop(db);
    publication.unlock()?;
    Ok(json!({"open_us":open_us,"snapshot_bytes":snapshot_bytes}))
}

pub fn probe() -> Result<serde_json::Value> {
    let mut cases = Vec::new();
    for native in [false, true] {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let current = root.join("current.grafeo");
        let candidate = root.join("candidate.grafeo");
        let conn = Connection::open(root.join("truth.sqlite"))?;
        conn.execute_batch(
            "CREATE TABLE knowledge(version INTEGER NOT NULL); INSERT INTO knowledge VALUES(1)",
        )?;
        build(&current, 1, 0, native)?;
        verify(&current, 1, 0, native)?;

        // Both readers keep the publication lease through graph/result/handle close.
        let reader1 = lock(&root.join("publication.lock"))?;
        reader1.lock_shared()?;
        let reader2 = lock(&root.join("publication.lock"))?;
        reader2.lock_shared()?;
        let (db1, _) = open(&current, native)?;
        let (db2, _) = open(&current, native)?;
        let writer = lock(&root.join("publication.lock"))?;
        ensure!(
            matches!(writer.try_lock(), Err(std::fs::TryLockError::WouldBlock)),
            "writer entered while readers active"
        );
        db1.close()?;
        db2.close()?;
        drop((db1, db2));
        reader1.unlock()?;
        reader2.unlock()?;
        writer.try_lock()?;
        writer.unlock()?;

        build(&candidate, 2, 3, native)?;
        for activate in [false, true] {
            let status = Command::new(std::env::current_exe()?)
                .args([
                    "crash",
                    root.to_str().context("fixture path")?,
                    if native { "native" } else { "envelope" },
                    if activate { "after" } else { "before" },
                ])
                .status()?;
            ensure!(status.code() == Some(73), "crash child failed: {status}");
            let sql_version: u64 =
                conn.query_row("SELECT version FROM knowledge", [], |r| r.get(0))?;
            ensure!(sql_version == 1, "uncommitted SQLite version survived");
            let (_, graph_version) = open(&current, native)?;
            ensure!(graph_version == if activate { 2 } else { 1 });
            if !activate {
                verify(&current, 1, 0, native)?;
            } else {
                ensure!(
                    graph_version != sql_version,
                    "crash divergence not detectable"
                );
            }
        }
        // Explicit rebuild from authoritative SQLite; no implicit repair on open.
        build(&candidate, 1, 0, native)?;
        writer.lock()?;
        fs::rename(&candidate, &current)?;
        sync_directory(root)?;
        writer.unlock()?;
        verify(&current, 1, 0, native)?;

        build(&candidate, 2, 3, native)?;
        writer.lock()?;
        conn.execute_batch("BEGIN IMMEDIATE; UPDATE knowledge SET version=2")?;
        verify(&candidate, 2, 3, native)?;
        fs::rename(&candidate, &current)?;
        sync_directory(root)?;
        conn.execute_batch("COMMIT")?;
        writer.unlock()?;
        verify(&current, 2, 3, native)?;

        // Corrupt candidate is never activated; the current projection remains intact.
        let mut corrupt = fs::read(&current)?;
        corrupt.truncate(16);
        fs::write(&candidate, corrupt)?;
        ensure!(
            open(&candidate, native).is_err(),
            "truncated candidate accepted"
        );
        verify(&current, 2, 3, native)?;
        fs::remove_file(&candidate)?;

        let start = Instant::now();
        build(&candidate, 7, 2000, native)?;
        let build_ms = start.elapsed().as_secs_f64() * 1000.0;
        let bytes = fs::metadata(&candidate)?.len();
        let start = Instant::now();
        for _ in 0..5 {
            verify(&candidate, 7, 2000, native)?;
        }
        let open_verify_ms = start.elapsed().as_secs_f64() * 1000.0 / 5.0;
        let measured = Command::new("/usr/bin/time")
            .args(["-l"])
            .arg(std::env::current_exe()?)
            .arg("measure")
            .arg(&candidate)
            .arg(if native { "native" } else { "envelope" })
            .output()?;
        ensure!(
            measured.status.success(),
            "measurement process: {}",
            String::from_utf8_lossy(&measured.stderr)
        );
        let measurement: serde_json::Value = serde_json::from_slice(&measured.stdout)?;
        let timing = String::from_utf8(measured.stderr)?;
        let max_rss_bytes: u64 = timing
            .lines()
            .find(|line| line.contains("maximum resident set size"))
            .context("macOS time RSS metric missing")?
            .split_whitespace()
            .next()
            .context("RSS value")?
            .parse()?;
        cases.push(
            json!({"carrier":if native {"native_catalog_v2"} else {"snapshot_envelope_v1"},
            "empty_graph":"pass","reopen":"pass","concurrent_readers":"pass",
            "blocked_writer":"pass","crash_before_activation":"old_complete",
            "crash_after_activation":"detectable_sqlite_divergence","explicit_rebuild":"pass",
            "successful_commit":"pass","truncated_candidate":"rejected",
            "nodes":2000,"file_bytes":bytes,"build_ms":build_ms,"open_verify_ms":open_verify_ms,
            "fresh_process":measurement,"max_rss_bytes":max_rss_bytes}),
        );
    }
    // Unsupported input must not acquire a default version.
    let missing = GrafeoDB::new_in_memory();
    ensure!(missing.application_metadata(KEY).is_none());
    let mut old = missing.export_snapshot()?;
    old[0] = 4;
    ensure!(
        GrafeoDB::import_snapshot(&old).is_err(),
        "old snapshot version silently imported"
    );
    Ok(json!(cases))
}
