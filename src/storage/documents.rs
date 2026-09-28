use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::domain::documents::{DocumentInput, DocumentRevision, Publication, PublicationStatus};
use crate::domain::ids::{DocumentId, PassageId, RevisionId};
use crate::domain::passages::PassageRange;
use crate::providers::embeddings::validate_vectors;
use crate::{CommonplaceError, Result};

use super::{database::storage_error, evidence, search_index};

pub fn current(connection: &Connection, source_key: &str) -> Result<Option<DocumentRevision>> {
    let id: Option<i64> = connection
        .query_row(
            "SELECT r.revision_id FROM document_revisions r JOIN documents d USING(document_id)
         WHERE d.source_key = ?1 ORDER BY r.revision_number DESC LIMIT 1",
            [source_key],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    id.map(|id| evidence::revision(connection, RevisionId::new(id)?))
        .transpose()
}

pub fn matches(revision: &DocumentRevision, input: &DocumentInput) -> bool {
    revision.text == input.text
        && revision.title == input.title
        && revision.source_type == input.source_type
        && revision.occurred_at == input.occurred_at
        && revision.metadata == input.metadata
}

pub struct PreparedDocument {
    pub ranges: Vec<PassageRange>,
    pub vectors: Vec<Vec<f32>>,
}

pub fn publish(
    transaction: &Transaction<'_>,
    input: &DocumentInput,
    expected_revision: Option<RevisionId>,
    prepared: Option<&PreparedDocument>,
    timestamp: &str,
) -> Result<Publication> {
    let current = current(transaction, &input.source_key)?;
    if let Some(current) = &current
        && matches(current, input)
    {
        transaction
            .execute(
                "UPDATE documents SET last_ingested_at = ?1 WHERE document_id = ?2",
                params![timestamp, current.document_id.value()],
            )
            .map_err(storage_error)?;
        return Ok(Publication {
            status: PublicationStatus::Unchanged,
            document_id: current.document_id,
            revision_id: current.revision_id,
            passage_ids: current.passage_ids.clone(),
        });
    }
    if current.as_ref().map(|revision| revision.revision_id) != expected_revision {
        return Err(CommonplaceError::Conflict(
            "source changed during preparation; retry ingestion".into(),
        ));
    }
    let prepared = prepared.ok_or_else(|| {
        CommonplaceError::Conflict(
            "source no longer matches the prepared input; retry ingestion".into(),
        )
    })?;
    validate_vectors(&prepared.vectors, prepared.ranges.len())?;
    let mut end = 0;
    for (ordinal, range) in prepared.ranges.iter().enumerate() {
        if range.ordinal != ordinal || range.start_byte != end || range.end_byte <= end {
            return Err(CommonplaceError::InvalidInput(
                "passages must cover the source without gaps or overlap".into(),
            ));
        }
        range.text(&input.text)?;
        end = range.end_byte;
    }
    if end != input.text.len() {
        return Err(CommonplaceError::InvalidInput(
            "passages must cover the complete source".into(),
        ));
    }
    let (document_id, revision_number, status) = if let Some(current) = current {
        search_index::remove_revision(transaction, current.revision_id)?;
        transaction
            .execute(
                "UPDATE documents SET last_ingested_at = ?1 WHERE document_id = ?2",
                params![timestamp, current.document_id.value()],
            )
            .map_err(storage_error)?;
        (
            current.document_id,
            current.revision_number.checked_add(1).ok_or_else(|| {
                CommonplaceError::LimitExceeded("revision number exhausted".into())
            })?,
            PublicationStatus::Updated,
        )
    } else {
        transaction.execute(
            "INSERT INTO documents(source_key, created_at, last_ingested_at) VALUES (?1, ?2, ?2)",
            params![input.source_key, timestamp],
        ).map_err(storage_error)?;
        (
            DocumentId::new(transaction.last_insert_rowid())?,
            1,
            PublicationStatus::Added,
        )
    };
    transaction
        .execute(
            "INSERT INTO document_revisions(document_id, revision_number, revision_digest, text,
            title, source_type, occurred_at, metadata_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                document_id.value(),
                revision_number,
                input.revision_digest()?,
                input.text,
                input.title,
                input.source_type,
                input.occurred_at,
                input.metadata_json()?,
                timestamp
            ],
        )
        .map_err(storage_error)?;
    let revision_id = RevisionId::new(transaction.last_insert_rowid())?;
    let mut passage_ids = Vec::new();
    for (range, vector) in prepared.ranges.iter().zip(&prepared.vectors) {
        let text = range.text(&input.text)?;
        transaction.execute(
            "INSERT INTO passages(revision_id, ordinal, start_byte, end_byte, text) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![revision_id.value(), range.ordinal, range.start_byte, range.end_byte, text],
        ).map_err(storage_error)?;
        let id = PassageId::new(transaction.last_insert_rowid())?;
        search_index::insert(transaction, id, text, vector)?;
        passage_ids.push(id);
    }
    search_index::verify_document(transaction, document_id.value(), revision_id)?;
    Ok(Publication {
        status,
        document_id,
        revision_id,
        passage_ids,
    })
}
