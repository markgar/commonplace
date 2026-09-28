use std::{
    fs::{self, File, OpenOptions},
    path::Path,
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use oxigraph::sparql::SparqlEvaluator;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::{projection as p, query};

fn lock(root: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("publication.lock"))?)
}
fn sync(root: &Path) -> Result<()> {
    File::open(root)?.sync_all()?;
    Ok(())
}
fn activate(root: &Path) -> Result<()> {
    fs::rename(root.join("current"), root.join("previous"))?;
    fs::rename(root.join("candidate"), root.join("current"))?;
    sync(root)
}
fn restore(root: &Path) -> Result<()> {
    fs::rename(root.join("current"), root.join("rejected"))?;
    fs::rename(root.join("previous"), root.join("current"))?;
    sync(root)?;
    fs::remove_dir_all(root.join("rejected"))?;
    Ok(())
}
fn wait(child: &mut Child, timeout: Duration) -> Result<std::process::ExitStatus> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if start.elapsed() > timeout {
            child.kill()?;
            child.wait()?;
            anyhow::bail!("child exceeded {timeout:?}");
        }
        thread::sleep(Duration::from_millis(5));
    }
}
fn await_file(child: &mut Child, path: &Path) -> Result<()> {
    let start = Instant::now();
    while !path.exists() {
        ensure!(child.try_wait()?.is_none(), "child exited before readiness");
        if start.elapsed() > Duration::from_secs(5) {
            child.kill()?;
            child.wait()?;
            anyhow::bail!("reader readiness deadline");
        }
        thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}
pub fn child(root: &Path, mode: &str) -> Result<()> {
    if mode == "writer-lock" {
        let error = oxigraph::store::Store::open(root.join("write-lock"))
            .err()
            .context("second writer unexpectedly opened")?;
        fs::write(root.join("write-lock-error"), error.to_string())?;
        return Ok(());
    }
    if mode.starts_with("reader") {
        let lease = lock(root)?;
        lease.lock_shared()?;
        let store = p::open_checked(&root.join("current"), 1)?;
        let result = SparqlEvaluator::new()
            .parse_query("SELECT * WHERE { ?s ?p ?o }")?
            .on_store(&store)
            .execute()?;
        fs::write(root.join(format!("{mode}.ready")), b"ready")?;
        let start = Instant::now();
        while !root.join("release").exists() {
            ensure!(
                start.elapsed() < Duration::from_secs(10),
                "reader release deadline"
            );
            thread::sleep(Duration::from_millis(5));
        }
        drop(result);
        drop(store);
        lease.unlock()?;
        return Ok(());
    }
    if mode == "kill-query" {
        let lease = lock(root)?;
        lease.lock_shared()?;
        let store = oxigraph::store::Store::open_read_only(root.join("numbers"))?;
        fs::write(root.join("kill.ready"), b"ready")?;
        let result = SparqlEvaluator::new()
            .parse_query(query::expensive())?
            .on_store(&store)
            .execute()?;
        query::collect(result, 2)?;
        anyhow::bail!("expensive query unexpectedly completed before termination");
    }
    let db = Connection::open(root.join("truth.sqlite"))?;
    db.execute_batch("PRAGMA foreign_keys=ON; BEGIN IMMEDIATE; UPDATE state SET version=2; UPDATE knowledge SET withdrawn=1 WHERE id=4")?;
    p::build(&db, &root.join("candidate"))?;
    let lease = lock(root)?;
    lease.lock()?;
    if mode == "crash-between" {
        fs::rename(root.join("current"), root.join("previous"))?;
        sync(root)?;
    } else if mode == "crash-after" {
        activate(root)?;
    } else {
        ensure!(mode == "crash-before", "unknown crash mode");
    }
    // Deliberately skip destructors; RocksDB candidates were already flushed and dropped.
    std::process::exit(73);
}

pub fn probe(root: &Path) -> Result<Value> {
    let fixture = root.join("publication");
    fs::create_dir(&fixture)?;
    let db = p::fixture(&fixture.join("truth.sqlite"))?;
    db.execute_batch("BEGIN")?;
    p::build(&db, &fixture.join("current"))?;
    db.execute_batch("COMMIT")?;
    let lease = lock(&fixture)?;
    let executable = std::env::current_exe()?;
    let writer = oxigraph::store::Store::open(fixture.join("write-lock"))?;
    let mut contender = Command::new(&executable)
        .arg("child")
        .arg(&fixture)
        .arg("writer-lock")
        .spawn()?;
    ensure!(wait(&mut contender, Duration::from_secs(5))?.success());
    let native_lock_error = fs::read_to_string(fixture.join("write-lock-error"))?;
    drop(writer);
    drop(oxigraph::store::Store::open(fixture.join("write-lock"))?);
    let mut children = Vec::new();
    for mode in ["reader1", "reader2"] {
        let mut child = Command::new(&executable)
            .arg("child")
            .arg(&fixture)
            .arg(mode)
            .spawn()?;
        await_file(&mut child, &fixture.join(format!("{mode}.ready")))?;
        children.push(child);
    }
    ensure!(matches!(
        lease.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    fs::write(fixture.join("release"), b"release")?;
    for child in &mut children {
        ensure!(wait(child, Duration::from_secs(5))?.success());
    }
    lease.try_lock()?;
    lease.unlock()?;

    // Exercise a real deferred SQLite FK error at COMMIT, then restore retained graph.
    db.execute_batch("BEGIN IMMEDIATE; UPDATE state SET version=2; UPDATE knowledge SET withdrawn=1 WHERE id=4; INSERT INTO deferred_child VALUES(999)")?;
    p::build(&db, &fixture.join("candidate"))?;
    lease.lock()?;
    activate(&fixture)?;
    let commit_error = db.execute_batch("COMMIT").unwrap_err();
    db.execute_batch("ROLLBACK")?;
    restore(&fixture)?;
    lease.unlock()?;
    let store = p::open_checked(&fixture.join("current"), 1)?;
    p::verify(&db, &store)?;
    drop(store);
    let mut crashes = Vec::new();
    for phase in ["crash-before", "crash-after", "crash-between"] {
        let mut child = Command::new(&executable)
            .arg("child")
            .arg(&fixture)
            .arg(phase)
            .spawn()?;
        ensure!(wait(&mut child, Duration::from_secs(10))?.code() == Some(73));
        ensure!(p::version(&db)? == 1);
        let opening = p::open_checked(&fixture.join("current"), 1);
        if phase == "crash-before" {
            ensure!(opening.is_ok());
        } else {
            ensure!(opening.is_err(), "divergence/absence not rejected");
        }
        drop(opening);
        crashes.push(json!({"phase":phase,"sqlite_version":1,"read":if phase=="crash-before" {"old_complete"} else {"rejected"}}));
        // Explicit fixture recovery/rebuild, not an automatic production recovery policy.
        lease.lock()?;
        for name in ["current", "previous", "candidate"] {
            let path = fixture.join(name);
            if path.exists() {
                fs::remove_dir_all(path)?;
            }
        }
        db.execute_batch("BEGIN")?;
        p::build(&db, &fixture.join("candidate"))?;
        db.execute_batch("COMMIT")?;
        fs::rename(fixture.join("candidate"), fixture.join("current"))?;
        sync(&fixture)?;
        lease.unlock()?;
        drop(p::open_checked(&fixture.join("current"), 1)?);
    }
    // Successful change publishes the same uncommitted snapshot/version that SQLite commits.
    db.execute_batch(
        "BEGIN IMMEDIATE; UPDATE state SET version=2; UPDATE knowledge SET withdrawn=1 WHERE id=4",
    )?;
    p::build(&db, &fixture.join("candidate"))?;
    lease.lock()?;
    activate(&fixture)?;
    db.execute_batch("COMMIT")?;
    fs::remove_dir_all(fixture.join("previous"))?;
    lease.unlock()?;
    let store = p::open_checked(&fixture.join("current"), 2)?;
    let withdrawn = query::select(&store, "ASK { <urn:commonplace:knowledge:4> ?p ?o }", 1)?;
    ensure!(withdrawn["boolean"] == false);
    drop(store);
    // Stock backup and reader iterator lifetime: dropping Store alone does not close snapshots.
    let store = p::open_checked(&fixture.join("current"), 2)?;
    store.backup(fixture.join("corrupt"))?;
    let mut results = match SparqlEvaluator::new()
        .parse_query("SELECT * WHERE { ?s ?p ?o }")?
        .on_store(&store)
        .execute()?
    {
        oxigraph::sparql::QueryResults::Solutions(rows) => rows,
        _ => anyhow::bail!("expected SELECT"),
    };
    drop(store);
    ensure!(results.next().context("snapshot row")?.is_ok());
    drop(results);
    fs::write(fixture.join("corrupt/CURRENT"), b"")?;
    ensure!(oxigraph::store::Store::open_read_only(fixture.join("corrupt")).is_err());
    drop(p::open_checked(&fixture.join("current"), 2)?);
    let empty = root.join("empty");
    db.execute_batch("BEGIN; UPDATE knowledge SET withdrawn=1; UPDATE state SET version=3")?;
    p::build(&db, &empty)?;
    db.execute_batch("ROLLBACK")?;
    let empty = p::open_checked(&empty, 3)?;
    ensure!(empty.len()? == 1, "empty graph must contain metadata only");
    let default = query::select(&empty, "ASK { ?s ?p ?o }", 1)?;
    ensure!(default["boolean"] == false);
    drop(empty);
    Ok(
        json!({"two_process_readers":"pass","publisher_exclusion":"pass","native_writer_lock_error":native_lock_error,"commit_failure":commit_error.to_string(),
        "restore_previous":"pass","crashes":crashes,"explicit_rebuild":"pass",
        "successful_withdrawal":"pass","corrupt_candidate":"rejected",
        "iterator_outlives_store":"proved; drop iterator before lease release","empty_metadata_only":"pass"}),
    )
}

pub fn kill_probe(root: &Path) -> Result<Value> {
    let mut child = Command::new(std::env::current_exe()?)
        .arg("child")
        .arg(root)
        .arg("kill-query")
        .spawn()?;
    await_file(&mut child, &root.join("kill.ready"))?;
    let lease = lock(root)?;
    ensure!(matches!(
        lease.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    ensure!(
        Command::new("/bin/kill")
            .args(["-INT", &child.id().to_string()])
            .status()?
            .success()
    );
    let status = wait(&mut child, Duration::from_secs(5))?;
    ensure!(!status.success());
    lease.try_lock()?;
    lease.unlock()?;
    Ok(
        json!({"terminated_pid":child.id(),"status":status.to_string(),"publication_lock_released":true}),
    )
}
