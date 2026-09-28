use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Args;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Map;

use crate::adapters::sources::{self, FileOptions, MetadataOverrides};
use crate::app::ingest::{self, InputItem, OperationConfig};
use crate::domain::documents::DocumentInput;
use crate::providers::embeddings::LocalEmbeddingModel;
use crate::{CommonplaceError, Result};

use super::input::{Description, generated_schema, validate};
use super::output::{CommandResponse, CommandResult};

/// JSON representation of the implemented file-command options.
#[derive(Debug, Args, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct IngestArgs {
    #[arg(required_unless_present = "describe", conflicts_with = "describe")]
    #[schemars(length(min = 1))]
    paths: Vec<PathBuf>,
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
    #[arg(long, default_value = "file", conflicts_with = "describe")]
    #[schemars(length(min = 1))]
    source_type: String,
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
    /// Describe implemented file inputs without opening a store or loading models.
    #[arg(long)]
    #[serde(skip)]
    pub describe: bool,
}

impl IngestArgs {
    fn options(&self) -> Result<(OperationConfig, MetadataOverrides)> {
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
            source_key: "command-options".into(),
            text: String::new(),
            title: self.title.clone(),
            source_type: self.source_type.clone(),
            occurred_at: self.occurred_at.clone(),
            metadata,
        }
        .normalize()?;
        let metadata = MetadataOverrides {
            title: normalized.title,
            source_type: normalized.source_type,
            occurred_at: normalized.occurred_at,
            metadata: normalized.metadata,
        };
        Ok((config, metadata))
    }
}

pub(super) fn execute(root: &Path, args: IngestArgs) -> Result<CommandResponse> {
    if args.describe {
        let example_args = IngestArgs {
            paths: vec![PathBuf::from("notes.md")],
            recursive: false,
            include: Vec::new(),
            exclude: Vec::new(),
            title: None,
            source_type: "file".into(),
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
        return Ok(CommandResponse::new(
            "ingest.describe",
            "complete",
            CommandResult::Description(Description {
                input_schema: generated_schema::<IngestArgs>(),
                example: serde_json::to_value(example_args)?,
            }),
        ));
    }
    let (config, metadata) = args.options()?;
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
    let mut model = LocalEmbeddingModel::new(
        std::env::var_os("COMMONPLACE_MODEL_CACHE").map(PathBuf::from),
        config.embedding_batch_size,
    );
    let result = ingest::ingest(root, items, &mut model, &config)?;
    Ok(CommandResponse::new(
        "ingest",
        result.status(),
        CommandResult::Ingest(result),
    ))
}
