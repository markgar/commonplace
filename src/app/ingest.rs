use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::domain::documents::{DocumentInput, Publication, PublicationStatus};
use crate::domain::ids::{DocumentId, PassageId, RevisionId};
use crate::domain::passages;
use crate::providers::embeddings::{EMBEDDING_IDENTITY, EmbeddingModel, validate_vectors};
use crate::storage::{
    database::{SqliteDatabase, storage_error},
    documents::{self, PreparedDocument},
};
use crate::{CommonplaceError, Result};

#[derive(Debug, Clone)]
pub struct OperationConfig {
    pub maximum_source_bytes: usize,
    pub maximum_documents: usize,
    pub maximum_passages: usize,
    pub embedding_batch_size: usize,
    pub maximum_json_bytes: usize,
    pub writer_lock_timeout: Duration,
}

impl Default for OperationConfig {
    fn default() -> Self {
        Self {
            maximum_source_bytes: 10 * 1024 * 1024,
            maximum_documents: 1000,
            maximum_passages: 10000,
            embedding_batch_size: 32,
            maximum_json_bytes: 1024 * 1024,
            writer_lock_timeout: Duration::from_secs(2),
        }
    }
}

impl OperationConfig {
    pub fn validate(&self) -> Result<()> {
        for (name, limit) in [
            ("source bytes", self.maximum_source_bytes),
            ("documents", self.maximum_documents),
            ("passages", self.maximum_passages),
            ("embedding batch size", self.embedding_batch_size),
            ("JSON bytes", self.maximum_json_bytes),
        ] {
            if limit == 0 || i64::try_from(limit).is_err() {
                return Err(CommonplaceError::InvalidInput(format!(
                    "{name} limit must be positive and fit a signed 64-bit integer"
                )));
            }
        }
        if self.writer_lock_timeout.is_zero() {
            return Err(CommonplaceError::InvalidInput(
                "writer-lock timeout must be positive".into(),
            ));
        }
        Ok(())
    }
}

pub struct InputItem {
    pub input: String,
    pub source_key: Option<String>,
    pub document: Result<DocumentInput>,
}

#[derive(Debug, Serialize)]
pub struct ItemError {
    pub stage: &'static str,
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ItemResult {
    pub input: String,
    pub source_key: Option<String>,
    pub status: &'static str,
    pub document_id: Option<DocumentId>,
    pub revision_id: Option<RevisionId>,
    pub passage_ids: Vec<PassageId>,
    pub error: Option<ItemError>,
}

#[derive(Debug, Default, Serialize)]
pub struct Summary {
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub failed: usize,
}

#[derive(Debug, Serialize)]
pub struct IngestResult {
    pub summary: Summary,
    pub items: Vec<ItemResult>,
    #[serde(skip)]
    pub exit_code: u8,
}

impl IngestResult {
    pub fn status(&self) -> &'static str {
        if self.summary.failed == 0 {
            "complete"
        } else if self.summary.failed == self.items.len() {
            "failed"
        } else {
            "partial"
        }
    }
}

struct ItemFailure {
    stage: &'static str,
    error: CommonplaceError,
}

fn stage<T>(name: &'static str, result: Result<T>) -> std::result::Result<T, ItemFailure> {
    result.map_err(|error| ItemFailure { stage: name, error })
}

pub fn ingest(
    root: &Path,
    items: impl Iterator<Item = InputItem>,
    model: &mut impl EmbeddingModel,
    config: &OperationConfig,
) -> Result<IngestResult> {
    config.validate()?;
    let (minimum, maximum) = items.size_hint();
    if maximum == Some(minimum) && minimum > config.maximum_documents {
        return Err(CommonplaceError::LimitExceeded(format!(
            "request exceeds the {}-document limit",
            config.maximum_documents
        )));
    }
    drop(SqliteDatabase::read(root)?);
    let mut seen = HashSet::new();
    let mut result = IngestResult {
        summary: Summary::default(),
        items: Vec::new(),
        exit_code: 0,
    };
    for (index, mut item) in items.enumerate() {
        let exceeded = index >= config.maximum_documents;
        if exceeded && item.document.is_ok() {
            item.document = Err(CommonplaceError::LimitExceeded(format!(
                "request exceeds the {}-document limit",
                config.maximum_documents
            )));
        }
        let source_key = item.source_key.clone();
        let outcome = (|| {
            if let Some(key) = &source_key
                && !seen.insert(key.clone())
            {
                return stage(
                    "input",
                    Err(CommonplaceError::InvalidInput(format!(
                        "source key {key:?} is repeated in this request"
                    ))),
                );
            }
            let document = stage("input", item.document)?;
            if source_key.as_deref() != Some(document.source_key.as_str()) {
                return stage(
                    "input",
                    Err(CommonplaceError::InvalidInput(
                        "adapter source key does not match document".into(),
                    )),
                );
            }
            ingest_one(root, document, model, config)
        })();
        match outcome {
            Ok(publication) => {
                let status = match publication.status {
                    PublicationStatus::Added => {
                        result.summary.added += 1;
                        "added"
                    }
                    PublicationStatus::Updated => {
                        result.summary.updated += 1;
                        "updated"
                    }
                    PublicationStatus::Unchanged => {
                        result.summary.unchanged += 1;
                        "unchanged"
                    }
                };
                result.items.push(ItemResult {
                    input: item.input,
                    source_key,
                    status,
                    document_id: Some(publication.document_id),
                    revision_id: Some(publication.revision_id),
                    passage_ids: publication.passage_ids,
                    error: None,
                });
            }
            Err(failure) => {
                result.summary.failed += 1;
                let code = failure.error.exit_code();
                result.exit_code = match (result.exit_code, code) {
                    (1, _) | (_, 1) => 1,
                    (3, _) | (_, 3) => 3,
                    _ => 2,
                };
                result.items.push(ItemResult {
                    input: item.input,
                    source_key,
                    status: "failed",
                    document_id: None,
                    revision_id: None,
                    passage_ids: Vec::new(),
                    error: Some(ItemError {
                        stage: failure.stage,
                        code: failure.error.code(),
                        message: failure.error.to_string(),
                    }),
                });
            }
        }
        if exceeded {
            break;
        }
    }
    Ok(result)
}

fn ingest_one(
    root: &Path,
    input: DocumentInput,
    model: &mut impl EmbeddingModel,
    config: &OperationConfig,
) -> std::result::Result<Publication, ItemFailure> {
    let input = stage("input", input.normalize())?;
    if input.text.len() > config.maximum_source_bytes {
        return stage(
            "input",
            Err(CommonplaceError::LimitExceeded(format!(
                "source exceeds the {}-byte limit",
                config.maximum_source_bytes
            ))),
        );
    }
    if stage("input", input.metadata_json())?.len() > config.maximum_json_bytes {
        return stage(
            "input",
            Err(CommonplaceError::LimitExceeded(format!(
                "metadata exceeds the {}-byte JSON limit",
                config.maximum_json_bytes
            ))),
        );
    }
    let current = {
        let session = stage("read", SqliteDatabase::read(root))?;
        stage(
            "read",
            documents::current(session.connection(), &input.source_key),
        )?
    };
    let expected = current.as_ref().map(|revision| revision.revision_id);
    let unchanged = current
        .as_ref()
        .is_some_and(|revision| documents::matches(revision, &input));
    drop(current);
    let prepared = if unchanged {
        None
    } else {
        let ranges = stage(
            "prepare",
            passages::prepare(&input.text, config.maximum_passages),
        )?;
        let mut vectors = Vec::new();
        for batch in ranges.chunks(config.embedding_batch_size) {
            if model.identity() != EMBEDDING_IDENTITY {
                return stage(
                    "embed",
                    Err(CommonplaceError::ModelUnavailable(
                        "embedding identity does not match the store representation".into(),
                    )),
                );
            }
            let texts = stage(
                "prepare",
                batch
                    .iter()
                    .map(|range| range.text(&input.text))
                    .collect::<Result<Vec<_>>>(),
            )?;
            let embeddings = stage("embed", model.embed(&texts))?;
            stage("embed", validate_vectors(&embeddings, texts.len()))?;
            vectors.extend(embeddings);
        }
        Some(PreparedDocument { ranges, vectors })
    };
    let mut session = stage(
        "publish",
        SqliteDatabase::write(root, config.writer_lock_timeout),
    )?;
    let timestamp = stage(
        "publish",
        OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|error| CommonplaceError::Storage(error.to_string())),
    )?;
    let transaction = stage("publish", session.transaction())?;
    let publication = stage(
        "publish",
        documents::publish(
            &transaction,
            &input,
            expected,
            prepared.as_ref(),
            &timestamp,
        ),
    )?;
    stage("publish", transaction.commit().map_err(storage_error))?;
    Ok(publication)
}
