use anyhow::{Context, Result, ensure};
use oxigraph::{
    model::{GraphName, Literal, NamedNode, Quad},
    store::Store,
};
use serde_json::json;
use std::path::PathBuf;

#[path = "../../rust-packaging/src/models.rs"]
mod models;
mod projection;
mod publication;
mod query;
#[path = "../../rust-packaging/src/sqlite.rs"]
mod sqlite;

fn graph(root: &std::path::Path) -> Result<serde_json::Value> {
    let db = projection::fixture(&root.join("canonical.sqlite"))?;
    db.execute_batch("BEGIN IMMEDIATE; UPDATE state SET version=2")?;
    projection::build(&db, &root.join("graph"))?;
    let committed = rusqlite::Connection::open(root.join("canonical.sqlite"))?;
    ensure!(projection::version(&committed)? == 1);
    db.execute_batch("COMMIT")?;
    let store = projection::open_checked(&root.join("graph"), 2)?;
    let projection = projection::verify(&db, &store)?;
    drop(store);
    let numbers = Store::open(root.join("numbers"))?;
    for start in (0..2000).step_by(128) {
        let quads = (start..(start + 128).min(2000))
            .map(|n| -> Result<_> {
                Ok(Quad::new(
                    NamedNode::new(format!("urn:probe:{n}"))?,
                    NamedNode::new("urn:probe:n")?,
                    Literal::from(i64::from(n)),
                    GraphName::DefaultGraph,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        numbers.extend(quads)?;
    }
    numbers.flush()?;
    drop(numbers);
    let numbers = Store::open_read_only(root.join("numbers"))?;
    let query = query::probe(&numbers)?;
    drop(numbers);
    Ok(
        json!({"projection":projection,"queries":query,"publication":publication::probe(root)?,
        "process_termination":publication::kill_probe(root)?}),
    )
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("child") {
        ensure!(args.len() == 4, "child requires root and mode");
        return publication::child(std::path::Path::new(&args[2]), &args[3]);
    }
    let mode = args.get(1).map(String::as_str).unwrap_or("graph");
    let root = std::env::var_os("COMMONPLACE_SPIKE_DATA_DIR")
        .map(PathBuf::from)
        .context("set disposable COMMONPLACE_SPIKE_DATA_DIR")?;
    ensure!(!root.exists(), "refuse existing probe directory");
    std::fs::create_dir_all(&root)?;
    let result = match mode {
        "graph" => graph(&root)?,
        "all" => json!({"oxigraph":graph(&root)?,"inference":models::probe(&root)?}),
        "sqlite" => sqlite::probe(&root, None)?,
        _ => anyhow::bail!("unknown spike mode"),
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
