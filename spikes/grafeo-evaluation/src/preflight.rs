use std::fs::{File, OpenOptions};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Result, ensure};
use grafeo::{Config, GrafeoDB, Role};
use serde_json::{Value, json};

#[path = "serialization.rs"]
mod serialization;

fn build(path: &Path, name: &str) -> Result<()> {
    ensure!(!path.exists(), "refusing to overwrite existing probe graph");
    let db = GrafeoDB::with_config(Config::persistent(path).with_threads(2))?;
    let mut session = db.session();
    session.begin_transaction()?;
    session.execute_cypher(&format!(
        "CREATE (:Entity {{id: 'entity:1', entity_id: 1, name: '{name}'}})"
    ))?;
    session.execute_cypher("CREATE (:Entity {id: 'entity:2', entity_id: 2, name: 'Roadmap'})")?;
    session.execute_cypher(
        "MATCH (a:Entity {entity_id: 1}), (b:Entity {entity_id: 2})
         CREATE (a)-[:RELATIONSHIP {knowledge_item_id: 7, predicate: 'depends_on'}]->(b)",
    )?;
    let mut prepared = session.prepare_commit()?;
    prepared.set_metadata("knowledge_version", "p2-metadata-sentinel-42");
    ensure!(
        prepared
            .metadata()
            .get("knowledge_version")
            .map(String::as_str)
            == Some("p2-metadata-sentinel-42"),
        "prepared metadata missing before commit"
    );
    prepared.commit()?;
    drop(session);
    db.close()?;
    drop(db);
    Ok(())
}

pub fn probe(root: &Path) -> Result<Value> {
    let graph = root.join("graph");
    std::fs::create_dir_all(&graph)?;
    let current = graph.join("current.grafeo");
    let candidate = graph.join("candidate.grafeo");
    let lock_path = graph.join("publication.lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&lock_path)?;
    build(&current, "Atlas")?;
    build(&candidate, "Candidate")?;
    lock.lock_shared()?;
    let competing = OpenOptions::new().read(true).write(true).open(&lock_path)?;
    ensure!(
        competing.try_lock().is_err(),
        "shared publication lock failed to exclude activation"
    );
    let db = GrafeoDB::with_config(
        Config::read_only(&current).with_query_timeout(Duration::from_secs(5)),
    )?;
    let reader = db.session_with_role(Role::ReadOnly);
    let mut rejections = Vec::new();
    for query in [
        "CREATE (:Entity {id:'entity:3'})",
        "MATCH (n:Entity) SET n.name = 'Changed'",
        "MATCH (n:Entity) DETACH DELETE n",
        "MERGE (:Entity {id:'entity:3'})",
    ] {
        let error = reader
            .execute_cypher(query)
            .expect_err("read-only mutation accepted");
        ensure!(
            matches!(&error, grafeo::Error::Query(e) if e.message.starts_with("permission denied:")),
            "mutation failed for non-authorization reason: {error}"
        );
        rejections.push(error.to_string());
    }
    let result = reader.execute_cypher("UNWIND range(1, 10000) AS x RETURN x")?;
    let materialized = result.row_count();
    let consumed = result.iter().take(3).count();
    ensure!(
        materialized == 10000 && consumed == 3,
        "unexpected materialization probe"
    );
    let values = reader.execute_cypher(
        "MATCH p=(a:Entity)-[r:RELATIONSHIP]->(b:Entity)
         RETURN a, r, p, a.id AS canonical_id, 42 AS integer,
                [1, true, null] AS items, {z: 2, a: 'text'} AS object",
    )?;
    let raw_values = format!("{:?}", values.rows());
    let raw_types = format!("{:?}", values.column_types);
    let serialized_fixture = serialization::fixture(&db, &values)?;
    let collision = reader.execute_cypher(
        "MATCH (a:Entity {entity_id:1})
         RETURN a, {_id:0, _labels:['Entity'], entity_id:1, id:'entity:1', name:'Atlas'} AS object",
    )?;
    ensure!(
        collision.rows()[0][0] == collision.rows()[0][1],
        "map collision changed"
    );
    ensure!(
        collision.column_types[0] == collision.column_types[1],
        "types now disambiguate maps"
    );
    let snapshot = db.export_snapshot()?;
    let sentinel = b"p2-metadata-sentinel-42";
    ensure!(
        !snapshot
            .windows(sentinel.len())
            .any(|bytes| bytes == sentinel),
        "prepared metadata unexpectedly persisted"
    );
    drop(collision);
    drop(values);
    drop(result);
    drop(reader);
    db.close()?;
    drop(db);
    lock.unlock()?;
    competing.try_lock()?;
    for path in [&current, &candidate] {
        ensure!(
            !path.with_extension("grafeo.wal").exists(),
            "closed graph still has a WAL sidecar"
        );
    }
    std::fs::rename(&candidate, &current)?;
    File::open(&graph)?.sync_all()?;
    competing.unlock()?;
    lock.lock_shared()?;
    let reopened = GrafeoDB::open_read_only(&current)?;
    let reader = reopened.session_with_role(Role::ReadOnly);
    let name: String = reader
        .execute_cypher("MATCH (n:Entity {entity_id:1}) RETURN n.name")?
        .scalar()?;
    ensure!(name == "Candidate", "candidate did not publish");
    drop(reader);
    reopened.close()?;
    drop(reopened);
    lock.unlock()?;
    let timed = GrafeoDB::with_config(
        Config::in_memory()
            .with_threads(2)
            .with_query_timeout(Duration::from_millis(1)),
    )?;
    let reader = timed.session_with_role(Role::ReadOnly);
    let start = Instant::now();
    let error = reader
        .execute_cypher(
            "UNWIND range(1, 1000000) AS x UNWIND range(1, 1000000) AS y RETURN sum(x*y)",
        )
        .expect_err("deadline query unexpectedly completed");
    let elapsed = start.elapsed();
    let deadline = match &error {
        grafeo::Error::Query(error) => error.kind,
        other => anyhow::bail!("not a native query deadline error: {other}"),
    };
    ensure!(format!("{deadline:?}") == "Timeout", "not timeout: {error}");
    drop(reader);
    timed.close()?;
    Ok(json!({
        "gate":"blocked", "grafeo":grafeo::VERSION, "role_rejections":rejections,
        "publication":{"status":"pass", "current":current, "candidate":candidate,
            "lock":lock_path, "reopened_name":name, "handles_dropped_before_rename":true},
        "deadline":{"error":error.to_string(), "elapsed_ms":elapsed.as_millis()},
        "bounded_results":{"status":"fail", "row_limit":2, "allowed":3,
            "materialized_before_iteration":materialized, "consumed":consumed},
        "metadata":{"status":"blocked", "prepared_metadata_survives_snapshot":false,
            "reason":"no verified application metadata carrier; no reserved node or sidecar added"},
        "serialization_probe":{"status":"blocked", "raw_types":raw_types, "raw_values":raw_values,
            "known_typed_fixture":serialized_fixture, "node_and_plain_map_indistinguishable":true,
            "unsupported_values_rejected":true}
    }))
}
