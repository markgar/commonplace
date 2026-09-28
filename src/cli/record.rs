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
    #[arg(required_unless_present_any = ["describe", "jsonl"], conflicts_with_all = ["describe", "jsonl"])]
    file: Option<PathBuf>,
    /// Read JSON Lines request fragments as one atomic request (not per-line writes).
    #[arg(long, conflicts_with = "describe")]
    jsonl: Option<PathBuf>,
    /// Describe supported record forms, JSONL framing, and current vocabulary.
    #[arg(long, conflicts_with_all = ["file", "jsonl"])]
    pub describe: bool,
}

#[derive(Debug, Serialize)]
pub struct Description {
    input_schema: schemars::Schema,
    example: serde_json::Value,
    vocabulary: Vocabulary,
    jsonl: serde_json::Value,
    validation: serde_json::Value,
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
                jsonl: serde_json::json!({
                    "fragment_schema": "input_schema",
                    "example": [
                        {"created_by":"manual","items":[{"kind":"entity","ref":"riley","name":"Riley"}]},
                        {"created_by":"manual","items":[]}
                    ],
                    "atomicity": "All fragments concatenate into one request; no partial success.",
                    "created_by": "Every fragment must have the same optional request-level created_by; no inheritance.",
                    "framing": "One JSON object per LF/CRLF line; optional final newline; no blank lines, BOM, or empty file.",
                    "maximum_total_bytes": 1048576,
                    "maximum_total_items": 1000
                }),
                validation: serde_json::json!({
                    "references": "Local refs name preceding entity declarations, including earlier JSONL fragments.",
                    "endpoints": "Predicate endpoints require a permitted type in the resulting active state.",
                    "literals": "Predicate determines string/integer/boolean/timestamp; timestamps normalize to UTC RFC3339. Integer tokens must be signed i64, never float/exponent tokens.",
                    "support": "Each supplied quote/paired byte offsets must match the whole canonical passage; duplicate passage IDs are rejected."
                }),
            }),
        ));
    }
    let jsonl = args.jsonl.is_some();
    let file = args
        .file
        .or(args.jsonl)
        .ok_or_else(|| CommonplaceError::InvalidInput("record JSON file is required".into()))?;
    let maximum_bytes = crate::app::schema::OperationConfig::default().maximum_input_bytes;
    let bytes = input::read_bounded(std::fs::File::open(file)?, maximum_bytes, "record")?;
    let request = decode(&bytes, jsonl, &schema)?;
    let result = crate::app::record::record(root, request, Duration::from_secs(2))?;
    Ok(CommandResponse::new(
        "record",
        "complete",
        CommandResult::Record(result),
    ))
}

fn fragment(bytes: &[u8], schema: &schemars::Schema) -> Result<RecordInput> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|error| CommonplaceError::InvalidInput(error.to_string()))?;
    input::validate(&value, schema)?;
    // Preserve token types and duplicate fields rather than decoding the Value.
    serde_json::from_slice(bytes).map_err(|error| CommonplaceError::InvalidInput(error.to_string()))
}

fn decode(bytes: &[u8], jsonl: bool, schema: &schemars::Schema) -> Result<RecordInput> {
    if !jsonl {
        return fragment(bytes, schema);
    }
    if bytes.is_empty() {
        return Err(CommonplaceError::InvalidInput(
            "record JSONL is empty; use an explicit {\"items\":[]} no-op fragment".into(),
        ));
    }
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    let mut request: Option<RecordInput> = None;
    for (line, bytes) in bytes.split(|byte| *byte == b'\n').enumerate() {
        let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
        let part = fragment(bytes, schema).map_err(|error| {
            CommonplaceError::InvalidInput(format!("JSONL line {}: {error}", line + 1))
        })?;
        if let Some(request) = &mut request {
            if request.created_by != part.created_by {
                return Err(CommonplaceError::InvalidInput(format!(
                    "JSONL line {}: created_by must match every fragment, including omission",
                    line + 1
                )));
            }
            if request.items.len() + part.items.len() > 1000 {
                return Err(CommonplaceError::InvalidInput(format!(
                    "JSONL line {}: combined request exceeds 1000 items",
                    line + 1
                )));
            }
            request.items.extend(part.items);
        } else {
            request = Some(part);
        }
    }
    let request = request.ok_or_else(|| {
        CommonplaceError::InvalidInput(
            "record JSONL is empty; use an explicit {\"items\":[]} no-op fragment".into(),
        )
    })?;
    request.validate()?;
    Ok(request)
}
