use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde_json::json;

#[path = "../../grafeo-evaluation/src/preflight.rs"]
mod graph;
mod models;
mod sqlite;

fn main() -> Result<()> {
    let command = std::env::args().nth(1).unwrap_or_else(|| "all".into());
    let root = std::env::var_os("COMMONPLACE_SPIKE_DATA_DIR")
        .map(PathBuf::from)
        .context("set COMMONPLACE_SPIKE_DATA_DIR to a disposable probe directory")?;
    std::fs::create_dir_all(&root)?;
    let report = match command.as_str() {
        "sqlite" => sqlite::probe(&root, None)?,
        "grafeo" => graph::probe(&root)?,
        "models" => models::probe(&root)?,
        "all" => json!({
            "grafeo": graph::probe(&root)?,
            "inference": models::probe(&root)?,
        }),
        other => bail!("unknown spike command: {other}"),
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
