pub mod database;
pub mod documents;
pub mod evidence;
pub(crate) mod graph_snapshot;
pub mod search_index;
pub mod vocabulary;

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{CommonplaceError, Result};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoreConfig {
    format: String,
    database: String,
    graph: String,
}

impl StoreConfig {
    pub(crate) fn expected() -> Self {
        Self {
            format: "commonplace-config/2".into(),
            database: "commonplace.sqlite3".into(),
            graph: "graph/current".into(),
        }
    }

    pub(crate) fn validate(root: &Path) -> Result<()> {
        let path = root.join("config.json");
        let bytes = std::fs::read(&path).map_err(|error| {
            CommonplaceError::Conflict(format!(
                "knowledge-base configuration is unavailable at {}: {error}",
                path.display()
            ))
        })?;
        let config: Self = serde_json::from_slice(&bytes).map_err(|error| {
            CommonplaceError::Conflict(format!(
                "knowledge-base configuration is invalid at {}: {error}",
                path.display()
            ))
        })?;
        let expected = Self::expected();
        if config.format != expected.format
            || config.database != expected.database
            || config.graph != expected.graph
        {
            return Err(CommonplaceError::Conflict(format!(
                "unsupported configuration in {}",
                path.display()
            )));
        }
        if root.join("graph/current.grafeo").try_exists()? {
            return Err(CommonplaceError::Conflict(format!(
                "incompatible version-1 graph layout in {}; create a fresh store",
                root.display()
            )));
        }
        Ok(())
    }
}
