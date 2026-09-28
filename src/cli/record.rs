use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Args;
use serde::Serialize;

use super::input;
use super::output::{CommandResponse, CommandResult};
use crate::domain::knowledge::{RecordInput, input_schema};
use crate::domain::schema::Vocabulary;
use crate::{CommonplaceError, Result};

#[derive(Debug, Args)]
pub struct RecordArgs {
    #[arg(required_unless_present = "describe", conflicts_with = "describe")]
    file: Option<PathBuf>,
    /// Describe supported JSON entity/metadata/type forms and current vocabulary.
    #[arg(long)]
    pub describe: bool,
}

#[derive(Debug, Serialize)]
pub struct Description {
    input_schema: schemars::Schema,
    example: serde_json::Value,
    vocabulary: Vocabulary,
}

pub fn execute(root: &Path, args: RecordArgs) -> Result<CommandResponse> {
    let schema = input_schema();
    if args.describe {
        let example = serde_json::json!({"items":[{"kind":"entity","name":"Riley"}]});
        input::validate(&example, &schema)?;
        let input: RecordInput = serde_json::from_value(example.clone())?;
        input.validate()?;
        return Ok(CommandResponse::new(
            "record.describe",
            "complete",
            CommandResult::RecordDescription(Description {
                input_schema: schema,
                example,
                vocabulary: crate::app::schema::show(root)?,
            }),
        ));
    }
    let file = args
        .file
        .ok_or_else(|| CommonplaceError::InvalidInput("record JSON file is required".into()))?;
    let maximum_bytes = crate::app::schema::OperationConfig::default().maximum_input_bytes;
    let bytes = input::read_bounded(std::fs::File::open(file)?, maximum_bytes, "record")?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
        CommonplaceError::InvalidInput(format!(
            "record expects a JSON object; facts and JSONL are unsupported: {error}"
        ))
    })?;
    input::validate(&value, &schema)?;
    let request: RecordInput = serde_json::from_slice(&bytes)
        .map_err(|error| CommonplaceError::InvalidInput(error.to_string()))?;
    let result = crate::app::record::record(root, request, Duration::from_secs(2))?;
    Ok(CommandResponse::new(
        "record",
        "complete",
        CommandResult::Record(result),
    ))
}
