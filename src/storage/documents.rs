use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::domain::documents::{DocumentInput, DocumentRevision, Publication, PublicationStatus};
use crate::domain::ids::{DocumentId, KnowledgeItemId, PassageId, RevisionId};
use crate::domain::passages::PassageRange;
use crate::domain::remove::RemovedSource;
use crate::providers::embeddings::validate_vectors;
use crate::{CommonplaceError, Result};

use super::{database::storage_error, evidence, search_index};

pub fn remove(transaction: &Transaction<'_>, source_key: &str) -> Result<RemovedSource> {
    let document_id: i64 = transaction
        .query_row(
            "SELECT document_id FROM documents WHERE source_key=?1",
            [source_key],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?
        .ok_or_else(|| {
            CommonplaceError::NotFound(format!("source key not found: {source_key:?}"))
        })?;
    let document_id = DocumentId::new(document_id)?;
    let revisions = transaction
        .prepare(
            "SELECT revision_id FROM document_revisions WHERE document_id=?1 ORDER BY revision_id",
        )
        .map_err(storage_error)?
        .query_map([document_id.value()], |row| row.get::<_, i64>(0))
        .map_err(storage_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage_error)?;
    let passages = transaction
        .prepare(
            "SELECT passage_id FROM passages JOIN document_revisions USING(revision_id)
            WHERE document_id=?1 ORDER BY passage_id",
        )
        .map_err(storage_error)?
        .query_map([document_id.value()], |row| row.get::<_, i64>(0))
        .map_err(storage_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage_error)?;
    let evidence = transaction
        .prepare(
            "SELECT knowledge_item_id, count(*) FROM knowledge_item_evidence
            JOIN passages USING(passage_id) JOIN document_revisions USING(revision_id)
            WHERE document_id=?1 GROUP BY knowledge_item_id ORDER BY knowledge_item_id",
        )
        .map_err(storage_error)?
        .query_map([document_id.value()], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, usize>(1)?))
        })
        .map_err(storage_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage_error)?;
    let removed = RemovedSource {
        source_key: source_key.to_owned(),
        document_id,
        deleted_revisions: revisions.len(),
        deleted_passages: passages.len(),
        detached_evidence: evidence.iter().map(|(_, count)| count).sum(),
        affected_knowledge_ids: evidence
            .into_iter()
            .map(|(id, _)| KnowledgeItemId::new(id))
            .collect::<Result<_>>()?,
    };
    for revision in revisions {
        search_index::remove_revision(transaction, RevisionId::new(revision)?)?;
    }
    transaction
        .execute(
            "DELETE FROM documents WHERE document_id=?1",
            [document_id.value()],
        )
        .map_err(storage_error)?;
    Ok(removed)
}

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
        && revision.temporal_state == input.temporal_state
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
    let mut previous_start = None;
    for (ordinal, range) in prepared.ranges.iter().enumerate() {
        if range.ordinal != ordinal
            || range.start_byte > end
            || previous_start.is_some_and(|start| range.start_byte <= start)
            || range.end_byte <= end
            || range.end_byte - range.start_byte > crate::domain::passages::PASSAGE_TARGET_BYTES
        {
            return Err(CommonplaceError::InvalidInput(
                "passages must cover the source without gaps, advance both offsets, and fit 1024 bytes including overlap".into(),
            ));
        }
        range.text(&input.text)?;
        previous_start = Some(range.start_byte);
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
            title, source_type, temporal_state, occurred_at, metadata_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                document_id.value(),
                revision_number,
                input.revision_digest()?,
                input.text,
                input.title,
                input.source_type,
                input.temporal_state.as_str(),
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
