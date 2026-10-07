use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Args;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Map;

use crate::adapters::sources::{self, FileOptions, MetadataOverrides};
use crate::adapters::streams::{self, JsonLines};
use crate::app::ingest::{self, InputItem, OperationConfig};
use crate::domain::documents::{DocumentInput, TemporalState};
use crate::providers::embeddings::LocalEmbeddingModel;
use crate::{CommonplaceError, Result};

use super::input::{generated_schema, validate};
use super::output::{CommandResponse, CommandResult};

/// JSON representation of the implemented ingestion command options.
#[derive(Debug, Args, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = input_modes)]
pub(super) struct IngestArgs {
    #[arg(required_unless_present_any = ["describe", "stdin", "jsonl"], conflicts_with_all = ["describe", "stdin", "jsonl"])]
    paths: Vec<PathBuf>,
    /// Read one exact UTF-8 document from stdin; requires --source-key.
    #[arg(long, requires = "source_key", conflicts_with_all = ["describe", "jsonl", "recursive", "include", "exclude"])]
    stdin: bool,
    /// Opaque stable identity for --stdin (not derived from a filename).
    #[arg(long, requires = "stdin", conflicts_with = "describe")]
    #[schemars(length(min = 1), regex(pattern = "^[^\\u0000]+$"))]
    source_key: Option<String>,
    /// Read self-contained JSON records, one per line; use - for stdin.
    #[arg(long, conflicts_with_all = ["describe", "stdin", "source_key", "recursive", "include", "exclude", "title", "source_type", "temporal_state", "occurred_at", "metadata"])]
    jsonl: Option<PathBuf>,
    /// Recurse into directories (discovered symlinks are always skipped).
    #[arg(long, conflicts_with = "describe")]
    recursive: bool,
    /// Include a root-relative scan glob; repeatable, excludes win.
    #[arg(long, conflicts_with = "describe")]
    include: Vec<String>,
    /// Exclude a root-relative scan glob; repeatable.
    #[arg(long, conflicts_with = "describe")]
    exclude: Vec<String>,
    /// Override every document's title; defaults to its filename.
    #[arg(long, conflicts_with = "describe")]
    title: Option<String>,
    /// Source category; defaults to file for paths and text for stdin.
    #[arg(long, conflicts_with = "describe")]
    #[schemars(length(min = 1))]
    source_type: Option<String>,
    /// Required for files/stdin: dated (known event time), timeless (not applicable), unknown (unavailable).
    #[arg(long, conflicts_with = "describe")]
    temporal_state: Option<TemporalState>,
    /// RFC 3339 source time for every document, normalized to UTC.
    #[arg(long, conflicts_with = "describe")]
    occurred_at: Option<String>,
    /// JSON object for every document; numbers must be signed 64-bit integers.
    #[arg(long, conflicts_with = "describe")]
    metadata: Option<String>,
    #[arg(long, default_value_t = OperationConfig::default().maximum_source_bytes, conflicts_with = "describe")]
    #[schemars(range(min = 1, max = 9223372036854775807_i64))]
    max_source_bytes: usize,
    #[arg(long, default_value_t = OperationConfig::default().maximum_documents, conflicts_with = "describe")]
    #[schemars(range(min = 1, max = 9223372036854775807_i64))]
    max_documents: usize,
    #[arg(long, default_value_t = OperationConfig::default().maximum_passages, conflicts_with = "describe")]
    #[schemars(range(min = 1, max = 9223372036854775807_i64))]
    max_passages: usize,
    #[arg(long, default_value_t = OperationConfig::default().embedding_batch_size, conflicts_with = "describe")]
    #[schemars(range(min = 1, max = 9223372036854775807_i64))]
    embedding_batch_size: usize,
    #[arg(long, default_value_t = OperationConfig::default().maximum_json_bytes, conflicts_with = "describe")]
    #[schemars(range(min = 1, max = 9223372036854775807_i64))]
    max_json_bytes: usize,
    #[arg(long, default_value_t = 2000, conflicts_with = "describe")]
    #[schemars(range(min = 1, max = 9223372036854775807_i64))]
    writer_lock_timeout_ms: u64,
    /// Describe implemented input options and JSONL records without opening a store.
    #[arg(long)]
    #[serde(skip)]
    pub describe: bool,
}

fn input_modes(schema: &mut schemars::Schema) {
    schema.insert("oneOf".into(), serde_json::json!([
        {"required": ["temporal_state"], "properties": {
            "paths": {"minItems": 1}, "stdin": {"const": false},
            "source_key": {"type": "null"}, "jsonl": {"type": "null"},
            "temporal_state": {"type": "string"}
        }},
        {"required": ["temporal_state"], "properties": {
            "paths": {"maxItems": 0}, "stdin": {"const": true},
            "source_key": {"type": "string"}, "jsonl": {"type": "null"},
            "temporal_state": {"type": "string"},
            "recursive": {"const": false}, "include": {"maxItems": 0}, "exclude": {"maxItems": 0}
        }},
        {"properties": {
            "paths": {"maxItems": 0}, "stdin": {"const": false},
            "source_key": {"type": "null"}, "jsonl": {"type": "string"},
            "recursive": {"const": false}, "include": {"maxItems": 0}, "exclude": {"maxItems": 0},
            "title": {"type": "null"}, "source_type": {"type": "null"},
            "temporal_state": {"type": "null"}, "occurred_at": {"type": "null"}, "metadata": {"type": "null"}
        }}
    ]));
    schema.insert(
        "allOf".into(),
        serde_json::json!([{
            "if": {"required": ["temporal_state"], "properties": {"temporal_state": {"const": "dated"}}},
            "then": {"required": ["occurred_at"], "properties": {"occurred_at": {"type": "string", "format": "date-time"}}},
            "else": {"properties": {"occurred_at": {"type": "null"}}}
        }]),
    );
}

#[derive(Debug, Serialize)]
pub struct IngestDescription {
    input_schema: schemars::Schema,
    example: serde_json::Value,
    record_schema: schemars::Schema,
    record_example: serde_json::Value,
    record_examples: Vec<serde_json::Value>,
}

impl IngestArgs {
    fn options(&self) -> Result<(OperationConfig, Option<MetadataOverrides>)> {
        if self.jsonl.is_none() {
            self.temporal_state.ok_or_else(|| {
                CommonplaceError::InvalidInput(
                    "explicit temporal intent is required: use --temporal-state dated with --occurred-at, timeless when event time does not apply, or unknown when event time is unavailable".into(),
                )
            })?.validate(self.occurred_at.as_deref())?;
        }
        validate(&serde_json::to_value(self)?, &generated_schema::<Self>())?;
        let config = OperationConfig {
            maximum_source_bytes: self.max_source_bytes,
            maximum_documents: self.max_documents,
            maximum_passages: self.max_passages,
            embedding_batch_size: self.embedding_batch_size,
            maximum_json_bytes: self.max_json_bytes,
            writer_lock_timeout: Duration::from_millis(self.writer_lock_timeout_ms),
        };
        config.validate()?;
        if self.jsonl.is_some() {
            return Ok((config, None));
        }
        if self
            .metadata
            .as_ref()
            .is_some_and(|value| value.len() > config.maximum_json_bytes)
        {
            return Err(CommonplaceError::LimitExceeded(format!(
                "metadata exceeds the {}-byte JSON limit",
                config.maximum_json_bytes
            )));
        }
        let metadata = self
            .metadata
            .as_ref()
            .map(|value| serde_json::from_str(value))
            .transpose()
            .map_err(|error| {
                CommonplaceError::InvalidInput(format!("metadata must be a JSON object: {error}"))
            })?
            .unwrap_or_else(Map::new);
        let normalized = DocumentInput {
            source_key: self
                .source_key
                .clone()
                .unwrap_or_else(|| "command-options".into()),
            text: String::new(),
            title: self.title.clone(),
            source_type: self
                .source_type
                .clone()
                .unwrap_or_else(|| if self.stdin { "text" } else { "file" }.into()),
            temporal_state: self
                .temporal_state
                .ok_or_else(|| CommonplaceError::InvalidInput("missing --temporal-state".into()))?,
            occurred_at: self.occurred_at.clone(),
            metadata,
        }
        .normalize()?;
        let metadata = MetadataOverrides {
            title: normalized.title,
            source_type: normalized.source_type,
            temporal_state: normalized.temporal_state,
            occurred_at: normalized.occurred_at,
            metadata: normalized.metadata,
        };
        Ok((config, Some(metadata)))
    }
}

pub(super) fn execute(
    root: &Path,
    model_cache: Option<PathBuf>,
    args: IngestArgs,
) -> Result<CommandResponse> {
    if args.describe {
        let example_args = IngestArgs {
            paths: vec![PathBuf::from("notes.md")],
            stdin: false,
            source_key: None,
            jsonl: None,
            recursive: false,
            include: Vec::new(),
            exclude: Vec::new(),
            title: None,
            source_type: None,
            temporal_state: Some(TemporalState::Timeless),
            occurred_at: None,
            metadata: None,
            max_source_bytes: OperationConfig::default().maximum_source_bytes,
            max_documents: OperationConfig::default().maximum_documents,
            max_passages: OperationConfig::default().maximum_passages,
            embedding_batch_size: OperationConfig::default().embedding_batch_size,
            max_json_bytes: OperationConfig::default().maximum_json_bytes,
            writer_lock_timeout_ms: 2000,
            describe: false,
        };
        example_args.options()?;
        let record_schema = streams::record_schema();
        let record_example = serde_json::json!({"source_key": "notes/42", "text": "Exact text\n", "temporal_state": "timeless"});
        let record_examples = vec![
            record_example.clone(),
            serde_json::json!({"source_key": "notes/43", "text": "Dated evidence\n", "temporal_state": "dated", "occurred_at": "2026-09-28T10:00:00-05:00"}),
            serde_json::json!({"source_key": "notes/44", "text": "Event date unavailable\n", "temporal_state": "unknown"}),
        ];
        for example in &record_examples {
            validate(example, &record_schema)?;
            let record = streams::parse_record(&serde_json::to_vec(example)?)?;
            DocumentInput::from(record).normalize()?;
        }
        return Ok(CommandResponse::new(
            "ingest.describe",
            "complete",
            CommandResult::IngestDescription(IngestDescription {
                input_schema: generated_schema::<IngestArgs>(),
                example: serde_json::to_value(example_args)?,
                record_schema,
                record_example,
                record_examples,
            }),
        ));
    }
    let (config, metadata) = args.options()?;
    let mut model = LocalEmbeddingModel::new(model_cache, config.embedding_batch_size);
    let result = if args.stdin {
        let metadata = metadata.ok_or_else(|| {
            CommonplaceError::InvalidInput("stdin requires explicit temporal metadata".into())
        })?;
        let source_key = args.source_key.ok_or_else(|| {
            CommonplaceError::InvalidInput("--stdin requires --source-key".into())
        })?;
        let items = std::iter::once_with(|| {
            streams::stdin_item(
                io::stdin().lock(),
                source_key,
                &metadata,
                config.maximum_source_bytes,
            )
        });
        ingest::ingest(root, items, &mut model, &config)?
    } else if let Some(path) = args.jsonl {
        let (reader, label): (Box<dyn BufRead>, String) = if path == Path::new("-") {
            (Box::new(io::stdin().lock()), "stdin".into())
        } else {
            (
                Box::new(BufReader::new(super::input::open_file(
                    &path,
                    "ingest --jsonl",
                )?)),
                path.to_string_lossy().into_owned(),
            )
        };
        let items = JsonLines::new(
            reader,
            label,
            config.maximum_json_bytes,
            config.maximum_documents,
        );
        ingest::ingest(root, items, &mut model, &config)?
    } else {
        let metadata = metadata.ok_or_else(|| {
            CommonplaceError::InvalidInput(
                "file ingestion requires explicit temporal metadata".into(),
            )
        })?;
        let files = sources::enumerate(
            &FileOptions {
                paths: args.paths,
                recursive: args.recursive,
                include: args.include,
                exclude: args.exclude,
            },
            config.maximum_documents,
        )?;
        let items = files.into_iter().map(|file| {
            let input = file.path.to_string_lossy().into_owned();
            if let Some(error) = file.error {
                return InputItem {
                    input,
                    source_key: None,
                    document: Err(error),
                };
            }
            match sources::canonical_file_source_key(&file.path) {
                Ok(source_key) => InputItem {
                    input,
                    source_key: Some(source_key.clone()),
                    document: sources::read_file(
                        &file.path,
                        source_key,
                        &metadata,
                        config.maximum_source_bytes,
                    ),
                },
                Err(error) => InputItem {
                    input,
                    source_key: None,
                    document: Err(error),
                },
            }
        });
        ingest::ingest(root, items, &mut model, &config)?
    };
    Ok(CommandResponse::new(
        "ingest",
        result.status(),
        CommandResult::Ingest(result),
    ))
}
