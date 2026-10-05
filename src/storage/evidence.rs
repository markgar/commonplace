use rusqlite::{Connection, OptionalExtension};

use crate::domain::documents::{Document, DocumentRevision, Evidence, TemporalState};
use crate::domain::ids::{DocumentId, PassageId, RevisionId};
use crate::{CommonplaceError, Result};

use super::database::storage_error;

pub fn document(connection: &Connection, id: DocumentId) -> Result<Document> {
    let (source_key, created_at, last_ingested_at) = connection
        .query_row(
            "SELECT source_key, created_at, last_ingested_at FROM documents WHERE document_id = ?1",
            [id.value()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(storage_error)?
        .ok_or_else(|| not_found(id))?;
    let mut statement = connection.prepare(
        "SELECT revision_id FROM document_revisions WHERE document_id = ?1 ORDER BY revision_number"
    ).map_err(storage_error)?;
    let revision_ids = statement
        .query_map([id.value()], |row| row.get::<_, i64>(0))
        .map_err(storage_error)?
        .map(|row| RevisionId::new(row.map_err(storage_error)?))
        .collect::<Result<Vec<_>>>()?;
    let current_revision_id = *revision_ids
        .last()
        .ok_or_else(|| CommonplaceError::Storage(format!("document {id} has no revision")))?;
    Ok(Document {
        document_id: id,
        source_key,
        created_at,
        last_ingested_at,
        current_revision_id,
        revision_ids,
    })
}

pub fn revision(connection: &Connection, id: RevisionId) -> Result<DocumentRevision> {
    let (
        document_id,
        source_key,
        revision_number,
        revision_digest,
        text,
        title,
        source_type,
        occurred_at,
        metadata_json,
        created_at,
        temporal_state,
    ) = connection
        .query_row(
            "SELECT r.document_id, d.source_key, r.revision_number, r.revision_digest, r.text,
                r.title, r.source_type, r.occurred_at, r.metadata_json, r.created_at, r.temporal_state
         FROM document_revisions r JOIN documents d USING(document_id) WHERE r.revision_id = ?1",
            [id.value()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get(9)?,
                    row.get::<_, String>(10)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?
        .ok_or_else(|| not_found(id))?;
    Ok(DocumentRevision {
        document_id: DocumentId::new(document_id)?,
        revision_id: id,
        source_key,
        revision_number,
        revision_digest,
        text,
        title,
        source_type,
        temporal_state: decode_temporal_state(&temporal_state, occurred_at.as_deref())?,
        occurred_at,
        metadata: serde_json::from_str(&metadata_json)?,
        created_at,
        passage_ids: passage_ids(connection, id)?,
    })
}

pub fn passage_ids(connection: &Connection, revision_id: RevisionId) -> Result<Vec<PassageId>> {
    let mut statement = connection
        .prepare("SELECT passage_id FROM passages WHERE revision_id = ?1 ORDER BY ordinal")
        .map_err(storage_error)?;
    statement
        .query_map([revision_id.value()], |row| row.get::<_, i64>(0))
        .map_err(storage_error)?
        .map(|row| PassageId::new(row.map_err(storage_error)?))
        .collect()
}

pub fn passage(connection: &Connection, id: PassageId) -> Result<Evidence> {
    let (revision_id, ordinal, start_byte, end_byte, text) = connection.query_row(
        "SELECT revision_id, ordinal, start_byte, end_byte, text FROM passages WHERE passage_id = ?1",
        [id.value()],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, usize>(1)?,
                  row.get::<_, usize>(2)?, row.get::<_, usize>(3)?, row.get::<_, String>(4)?)),
    ).optional().map_err(storage_error)?.ok_or_else(|| not_found(id))?;
    let revision = revision(connection, RevisionId::new(revision_id)?)?;
    if revision.text.get(start_byte..end_byte) != Some(text.as_str()) {
        return Err(CommonplaceError::Storage(format!(
            "stored passage {id} does not select its exact revision bytes"
        )));
    }
    Ok(Evidence {
        document_id: revision.document_id,
        revision_id: revision.revision_id,
        passage_id: id,
        source_key: revision.source_key,
        revision_number: revision.revision_number,
        ordinal,
        start_byte,
        end_byte,
        text,
        title: revision.title,
        source_type: revision.source_type,
        temporal_state: revision.temporal_state,
        occurred_at: revision.occurred_at,
        metadata: revision.metadata,
    })
}

pub(crate) fn decode_temporal_state(
    value: &str,
    occurred_at: Option<&str>,
) -> Result<TemporalState> {
    let state: TemporalState = value.parse().map_err(|error| {
        CommonplaceError::Storage(format!("invalid stored temporal_state: {error}"))
    })?;
    state.validate(occurred_at).map_err(|error| {
        CommonplaceError::Storage(format!("inconsistent stored temporal state: {error}"))
    })?;
    Ok(state)
}

fn not_found(id: impl std::fmt::Display) -> CommonplaceError {
    CommonplaceError::NotFound(format!(
        "{id} does not exist; use an ID returned by ingest or get"
    ))
}
