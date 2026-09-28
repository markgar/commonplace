use std::path::Path;

use grafeo::{Config, GrafeoDB};

use crate::{CommonplaceError, Result};

pub fn initialize(path: &Path) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        CommonplaceError::Graph(format!("graph path has no parent: {}", path.display()))
    })?;
    std::fs::create_dir_all(parent)?;

    let database = GrafeoDB::with_config(Config::persistent(path))
        .map_err(|error| CommonplaceError::Graph(error.to_string()))?;
    database
        .close()
        .map_err(|error| CommonplaceError::Graph(error.to_string()))
}
