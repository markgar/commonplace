use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use grafeo::{Config, GrafeoDB, Role};
use serde_json::json;

fn main() -> Result<()> {
    let root = match std::env::var_os("COMMONPLACE_GRAFEO_DATA_DIR") {
        Some(path) => path.into(),
        None => std::env::current_exe()?
            .parent()
            .context("release executable has no parent directory")?
            .join("data"),
    };
    std::fs::create_dir_all(&root)?;

    let graph_path = root.join("commonplace.grafeo");
    remove_graph(&graph_path)?;

    let db = GrafeoDB::with_config(
        Config::persistent(&graph_path)
            .with_memory_limit(256 * 1024 * 1024)
            .with_threads(2)
            .with_query_timeout(Duration::from_millis(50)),
    )?;
    let session = db.session();

    session.execute_cypher(
        "CREATE (:Entity {
            entity_id: 1,
            canonical_id: 'entity:atlas',
            name: 'Atlas'
        })",
    )?;
    session.execute_cypher(
        "CREATE (:Entity {
            entity_id: 2,
            canonical_id: 'entity:roadmap',
            name: 'Roadmap'
        })",
    )?;
    session.execute_cypher(
        "MATCH (a:Entity {entity_id: 1}), (b:Entity {entity_id: 2})
         CREATE (a)-[:RELATIONSHIP {
             knowledge_item_id: 7,
             predicate: 'depends_on',
             evidence_passage_id: 41
         }]->(b)",
    )?;

    let result = session.execute_cypher(
        "MATCH (a:Entity)-[r:RELATIONSHIP]->(b:Entity)
         RETURN a.canonical_id, r.knowledge_item_id, r.predicate,
                r.evidence_passage_id, b.canonical_id",
    )?;
    let rows: Vec<_> = result.iter().collect();
    if rows.len() != 1
        || rows[0][0].as_str() != Some("entity:atlas")
        || rows[0][1].as_int64() != Some(7)
        || rows[0][2].as_str() != Some("depends_on")
        || rows[0][3].as_int64() != Some(41)
        || rows[0][4].as_str() != Some("entity:roadmap")
    {
        bail!("Cypher query did not preserve canonical IDs and evidence properties");
    }

    let role_reader = db.session_with_role(Role::ReadOnly);
    let role_mutation_rejected = role_reader
        .execute_cypher("MATCH (n:Entity {entity_id: 1}) SET n.name = 'Changed'")
        .is_err();
    if !role_mutation_rejected {
        bail!("read-only role accepted a Cypher mutation");
    }

    db.close()?;

    let reopened = GrafeoDB::with_config(
        Config::read_only(&graph_path).with_query_timeout(Duration::from_millis(50)),
    )?;
    let read_only = reopened.session();
    let persisted_count: i64 = read_only
        .execute_cypher("MATCH (n:Entity) RETURN count(n)")?
        .scalar()?;
    if persisted_count != 2 {
        bail!("persistent reopen returned {persisted_count} entities, expected 2");
    }

    let database_mutation = read_only
        .execute_cypher("MATCH (n:Entity {entity_id: 1}) SET n.name = 'Changed'")
        .err()
        .map(|error| error.to_string());
    let database_mutation_rejected = database_mutation.is_some();

    reopened.close()?;

    let timeout_path = root.join("timeout.grafeo");
    remove_graph(&timeout_path)?;
    let timeout_db = GrafeoDB::with_config(
        Config::persistent(&timeout_path)
            .with_threads(2)
            .with_query_timeout(Duration::from_millis(1)),
    )?;
    let timeout_session = timeout_db.session();
    let timeout_started = Instant::now();
    let timeout_result = timeout_session.execute_cypher(
        "UNWIND range(1, 1000000) AS x
             UNWIND range(1, 1000000) AS y
             RETURN sum((x % 1000) * (y % 1000))",
    );
    let timeout_elapsed = timeout_started.elapsed();
    let timeout_message = timeout_result.err().map(|error| error.to_string());
    let timeout_passed = timeout_message
        .as_deref()
        .is_some_and(|message| message.to_ascii_lowercase().contains("timeout"))
        && timeout_elapsed < Duration::from_secs(2);
    timeout_db.close()?;

    let passed = role_mutation_rejected && database_mutation_rejected && timeout_passed;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "status": if passed { "partial" } else { "fail" },
            "grafeo_version": grafeo::VERSION,
            "graph_path": graph_path,
            "cypher_projection": "pass",
            "canonical_id_and_evidence_parity": "pass",
            "persistent_reopen": "pass",
            "role_read_only_mutation_rejected": role_mutation_rejected,
            "database_read_only": {
                "mutation_rejected": database_mutation_rejected,
                "error": database_mutation
            },
            "query_timeout": {
                "status": if timeout_passed { "pass" } else { "fail" },
                "elapsed_ms": timeout_elapsed.as_millis(),
                "error": timeout_message
            },
            "explicit_external_cancellation": {
                "status": "fail",
                "reason": "Grafeo 0.5.43 exposes query deadlines but no public interrupt or cancellation handle"
            }
        }))?
    );

    Ok(())
}

fn remove_graph(path: &Path) -> Result<()> {
    if path.is_file() {
        std::fs::remove_file(path)?;
    } else if path.is_dir() {
        std::fs::remove_dir_all(path)?;
    }

    let wal_path = path.with_extension("grafeo.wal");
    if wal_path.is_dir() {
        std::fs::remove_dir_all(wal_path)?;
    } else if wal_path.is_file() {
        std::fs::remove_file(wal_path)?;
    }
    Ok(())
}
