use std::path::Path;
use std::time::Duration;

use serde::Serialize;

use crate::domain::remove::{RemoveInput, RemovedSource};
use crate::storage::{
    database::{SqliteDatabase, storage_error},
    documents, graph_snapshot,
};
use crate::{CommonplaceError, Result};

pub(crate) const RECOVERY_GUIDANCE: &str = "Do not retry remove; verify the removed document is not_found and inspect affected knowledge with get.";

#[derive(Debug, Serialize)]
pub struct RemoveResult {
    #[serde(flatten)]
    pub removed: RemovedSource,
    pub knowledge_version: i64,
    #[serde(skip)]
    pub receipt: String,
}

pub fn remove(root: &Path, input: RemoveInput, timeout: Duration) -> Result<RemoveResult> {
    input.validate()?;
    let mut writer = SqliteDatabase::write(root, timeout)?;
    let transaction = writer.transaction()?;
    let operation = (|| {
        let removed = documents::remove(&transaction, &input.source_key)?;
        let knowledge_version = graph_snapshot::version(&transaction)?
            .checked_add(1)
            .ok_or_else(|| CommonplaceError::Storage("knowledge version exhausted".into()))?;
        transaction
            .execute(
                "UPDATE store_state SET knowledge_version=?1 WHERE singleton=1",
                [knowledge_version],
            )
            .map_err(storage_error)?;
        let ids: Vec<String> = removed
            .affected_knowledge_ids
            .iter()
            .map(ToString::to_string)
            .collect();
        let receipt = format!(
            "remove committed at knowledge_version {knowledge_version} (removed {} with source_key {:?}; affected knowledge: [{}])",
            removed.document_id,
            removed.source_key,
            ids.join(", ")
        );
        let result = RemoveResult {
            removed,
            knowledge_version,
            receipt,
        };
        crate::graph::publish(
            root,
            &transaction,
            timeout,
            &result.receipt,
            RECOVERY_GUIDANCE,
        )?;
        Ok(result)
    })();
    if let Err(error) = &operation
        && !transaction.is_autocommit()
        && let Err(rollback) = transaction.execute_batch("ROLLBACK")
    {
        return Err(CommonplaceError::Storage(format!(
            "{error}; SQLite rollback failed: {rollback}"
        )));
    }
    operation
}
