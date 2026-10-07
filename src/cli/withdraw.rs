use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Args;
use serde::Serialize;

use super::input;
use super::output::{CommandResponse, CommandResult};
use crate::app::withdraw::{ItemResult, Summary, WithdrawResult};
use crate::domain::ids::{EntityId, KnowledgeItemId, PredicateId};
use crate::domain::knowledge::{
    CanonicalLiteral, FactObject, Knowledge, KnowledgeDetail, LiteralValue,
};
use crate::domain::schema::ObjectKind;
use crate::domain::withdraw::{WithdrawInput, input_schema};
use crate::{CommonplaceError, Result};

#[derive(Debug, Args)]
pub struct WithdrawArgs {
    #[arg(required_unless_present = "describe", conflicts_with = "describe")]
    file: Option<PathBuf>,
    /// Describe atomic withdrawal input and retained output without opening a store.
    #[arg(long)]
    pub describe: bool,
}

#[derive(Debug, Serialize)]
pub struct Description {
    input_schema: schemars::Schema,
    example: serde_json::Value,
    output_example: CommandResponse,
    validation: serde_json::Value,
}

pub fn execute(root: &Path, args: WithdrawArgs) -> Result<CommandResponse> {
    let schema = input_schema();
    if args.describe {
        let example = serde_json::json!({
            "withdrawn_by":"manual", "knowledge_ids":["knowledge:5"]
        });
        input::validate(&example, &schema)?;
        serde_json::from_value::<WithdrawInput>(example.clone())?.validate()?;
        return Ok(CommandResponse::new(
            "withdraw.describe",
            "complete",
            CommandResult::WithdrawDescription(Box::new(Description {
                input_schema: schema,
                example,
                output_example: CommandResponse::new(
                    "withdraw",
                    "complete",
                    CommandResult::Withdraw(WithdrawResult {
                        knowledge_version: 2,
                        summary: Summary {
                            items: 1,
                            withdrawn: 1,
                        },
                        items: vec![ItemResult {
                            index: 0,
                            knowledge: Knowledge {
                                knowledge_id: KnowledgeItemId::new(5)?,
                                schema_version: 1,
                                created_at: "2026-09-28T12:00:00Z".into(),
                                created_by: Some("manual".into()),
                                withdrawn_at: Some("2026-09-28T13:00:00Z".into()),
                                withdrawn_by: Some("manual".into()),
                                detail: KnowledgeDetail::Fact {
                                    subject_entity_id: EntityId::new(1)?,
                                    predicate_id: PredicateId::new(2)?,
                                    object: FactObject::Literal(CanonicalLiteral::new(
                                        ObjectKind::String,
                                        &LiteralValue::String("approved".into()),
                                    )?),
                                },
                                support: vec![],
                            },
                        }],
                        receipt: String::new(),
                    }),
                ),
                validation: serde_json::json!({
                    "maximum_total_bytes": 1048576,
                    "ids": "Supply 1-1000 unique canonical knowledge:<positive i64> IDs. Empty and duplicate batches are rejected.",
                    "state": "Every ID must exist and be active; any invalid or already-withdrawn ID rejects the complete batch.",
                    "endpoints": "Remaining facts must retain a permitted active type at every required endpoint. Include dependent facts explicitly; there is no cascading withdrawal.",
                    "history": "Withdrawal is final. Subtype, canonical literal, creation/schema provenance and support are retained; one timestamp and version advance cover the batch.",
                    "recovery": "Do not retry a committed withdrawal; inspect the submitted IDs with get. Post-commit cleanup failures also require graph rebuild."
                }),
            })),
        ));
    }
    let file = args
        .file
        .ok_or_else(|| CommonplaceError::InvalidInput("withdraw JSON file is required".into()))?;
    let bytes = input::read_file(
        &file,
        crate::app::schema::OperationConfig::default().maximum_input_bytes,
        "withdraw",
    )?;
    let request = (|| -> Result<WithdrawInput> {
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| CommonplaceError::InvalidInput(error.to_string()))?;
        input::validate(&value, &schema)?;
        // Decode original bytes so duplicate fields cannot be silently collapsed.
        serde_json::from_slice(&bytes)
            .map_err(|error| CommonplaceError::InvalidInput(error.to_string()))
    })()
    .map_err(|error| error.context(format!("withdraw input file {}", file.display())))?;
    Ok(CommandResponse::new(
        "withdraw",
        "complete",
        CommandResult::Withdraw(crate::app::withdraw::withdraw(
            root,
            request,
            Duration::from_secs(2),
        )?),
    ))
}
