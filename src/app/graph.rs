use std::path::Path;
use std::time::Duration;

use serde::Serialize;

use crate::graph::{GraphRuntime, QueryConfig, SelectResult};
use crate::{Result, graph};

#[derive(Debug, Serialize)]
pub struct RebuildResult {
    pub knowledge_version: i64,
}

pub fn query(root: &Path, text: &str, config: QueryConfig) -> Result<SelectResult> {
    config.validate()?;
    GraphRuntime::open(root)?.query(text, config)
}

pub fn rebuild(root: &Path) -> Result<RebuildResult> {
    Ok(RebuildResult {
        knowledge_version: graph::rebuild(root, Duration::from_secs(2))?,
    })
}
