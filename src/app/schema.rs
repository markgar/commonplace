use std::path::Path;
use std::time::Duration;

use serde::Serialize;

use crate::Result;
use crate::domain::schema::{self, Additions, SchemaInput, SchemaPlan, Vocabulary};
use crate::storage::{database::SqliteDatabase, vocabulary};

#[derive(Debug, Clone, Copy)]
pub struct OperationConfig {
    pub writer_lock_timeout: Duration,
    pub maximum_input_bytes: usize,
}

impl Default for OperationConfig {
    fn default() -> Self {
        Self {
            writer_lock_timeout: Duration::from_secs(2),
            maximum_input_bytes: 1024 * 1024,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ApplyResult {
    pub schema_version: i64,
    pub projected_schema_version: i64,
    pub changed: bool,
    pub checked: bool,
    pub summary: Additions,
}

impl ApplyResult {
    fn from_plan(plan: &SchemaPlan, checked: bool, current_version: i64) -> Self {
        Self {
            schema_version: if checked {
                current_version
            } else {
                plan.schema_version
            },
            projected_schema_version: plan.schema_version,
            changed: plan.changed(),
            checked,
            summary: plan.summary(),
        }
    }
}

pub fn show(root: &Path) -> Result<Vocabulary> {
    let session = SqliteDatabase::read(root)?;
    vocabulary::read(session.connection())
}

pub fn apply(
    root: &Path,
    input: SchemaInput,
    check: bool,
    config: &OperationConfig,
) -> Result<ApplyResult> {
    if check {
        let session = SqliteDatabase::read(root)?;
        let current = vocabulary::read(session.connection())?;
        let plan = schema::plan(input, &current)?;
        return Ok(ApplyResult::from_plan(&plan, true, current.schema_version));
    }
    let mut session = SqliteDatabase::write(root, config.writer_lock_timeout)?;
    let transaction = session.transaction()?;
    let current = vocabulary::read(&transaction)?;
    let plan = schema::plan(input, &current)?;
    vocabulary::apply(&transaction, &plan)?;
    transaction
        .commit()
        .map_err(crate::storage::database::storage_error)?;
    Ok(ApplyResult::from_plan(&plan, false, current.schema_version))
}
