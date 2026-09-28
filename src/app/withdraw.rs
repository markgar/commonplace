use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::domain::knowledge::Knowledge;
use crate::domain::withdraw::WithdrawInput;
use crate::storage::{
    database::{SqliteDatabase, storage_error},
    graph_snapshot, knowledge,
};
use crate::{CommonplaceError, Result};

pub(crate) const RECOVERY_GUIDANCE: &str = "Do not retry withdraw; inspect these IDs with get.";

#[derive(Debug, Serialize)]
pub struct Summary {
    pub items: usize,
    pub withdrawn: usize,
}

#[derive(Debug, Serialize)]
pub struct ItemResult {
    pub index: usize,
    #[serde(flatten)]
    pub knowledge: Knowledge,
}

#[derive(Debug, Serialize)]
pub struct WithdrawResult {
    pub knowledge_version: i64,
    pub summary: Summary,
    pub items: Vec<ItemResult>,
    #[serde(skip)]
    pub receipt: String,
}

pub fn withdraw(root: &Path, input: WithdrawInput, timeout: Duration) -> Result<WithdrawResult> {
    let ids = input.validate()?;
    let mut writer = SqliteDatabase::write(root, timeout)?;
    let timestamp = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| CommonplaceError::Storage(error.to_string()))?;
    let transaction = writer.transaction()?;
    let operation = (|| {
        let items = knowledge::withdraw(
            &transaction,
            &ids,
            &timestamp,
            input.withdrawn_by.as_deref(),
        )?
        .into_iter()
        .enumerate()
        .map(|(index, knowledge)| ItemResult { index, knowledge })
        .collect();
        let knowledge_version = graph_snapshot::version(&transaction)?
            .checked_add(1)
            .ok_or_else(|| CommonplaceError::Storage("knowledge version exhausted".into()))?;
        transaction
            .execute(
                "UPDATE store_state SET knowledge_version=?1 WHERE singleton=1",
                [knowledge_version],
            )
            .map_err(storage_error)?;
        let receipt = format!(
            "withdraw committed at knowledge_version {knowledge_version} ({})",
            ids.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
        let result = WithdrawResult {
            knowledge_version,
            summary: Summary {
                items: ids.len(),
                withdrawn: ids.len(),
            },
            items,
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
