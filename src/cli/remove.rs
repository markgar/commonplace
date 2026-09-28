use std::path::Path;
use std::time::Duration;

use clap::Args;
use serde::Serialize;

use super::input;
use super::output::{CommandResponse, CommandResult};
use crate::app::remove::RemoveResult;
use crate::domain::ids::{DocumentId, KnowledgeItemId};
use crate::domain::remove::{RemoveInput, RemovedSource};
use crate::{CommonplaceError, Result};

#[derive(Debug, Args)]
pub struct RemoveArgs {
    /// Exact opaque stored key; for files copy the canonical file:// key from get.
    #[arg(
        long,
        required_unless_present = "describe",
        conflicts_with = "describe"
    )]
    source_key: Option<String>,
    /// Describe CLI arguments and output without opening a store; NOT a JSON input mode.
    #[arg(long)]
    pub describe: bool,
}

#[derive(Debug, Serialize)]
pub struct Description {
    input_schema: schemars::Schema,
    example: RemoveInput,
    output_example: CommandResponse,
}

pub fn execute(root: &Path, args: RemoveArgs) -> Result<CommandResponse> {
    let input_schema = input::generated_schema::<RemoveInput>();
    if args.describe {
        let example = RemoveInput {
            source_key: "opaque:key".into(),
        };
        example.validate()?;
        input::validate(&serde_json::to_value(&example)?, &input_schema)?;
        let output_example = CommandResponse::new(
            "remove",
            "complete",
            CommandResult::Remove(RemoveResult {
                removed: RemovedSource {
                    source_key: example.source_key.clone(),
                    document_id: DocumentId::new(1)?,
                    deleted_revisions: 2,
                    deleted_passages: 3,
                    detached_evidence: 4,
                    affected_knowledge_ids: vec![
                        KnowledgeItemId::new(1)?,
                        KnowledgeItemId::new(2)?,
                    ],
                },
                knowledge_version: 2,
                receipt: String::new(),
            }),
        );
        return Ok(CommandResponse::new(
            "remove.describe",
            "complete",
            CommandResult::RemoveDescription(Box::new(Description {
                input_schema,
                example,
                output_example,
            })),
        ));
    }
    let request = RemoveInput {
        source_key: args
            .source_key
            .ok_or_else(|| CommonplaceError::InvalidInput("--source-key is required".into()))?,
    };
    input::validate(&serde_json::to_value(&request)?, &input_schema)?;
    Ok(CommandResponse::new(
        "remove",
        "complete",
        CommandResult::Remove(crate::app::remove::remove(
            root,
            request,
            Duration::from_secs(2),
        )?),
    ))
}
