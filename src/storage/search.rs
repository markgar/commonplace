use rusqlite::{Connection, params};

use crate::domain::ids::PassageId;
use crate::domain::search::{CANDIDATE_LIMIT, Candidates, SearchFilters};
use crate::{CommonplaceError, Result};

use super::database::storage_error;

// Normalized UTC timestamps sort chronologically after removing Z, including
// subsecond boundaries. Keep comparison textual to avoid SQLite's millisecond rounding.
const ELIGIBLE: &str = "
    r.revision_number = (
        SELECT max(current.revision_number) FROM document_revisions current
        WHERE current.document_id = r.document_id
    )
    AND (?2 IS NULL OR rtrim(r.occurred_at, 'Z') >= rtrim(?2, 'Z'))
    AND (json_array_length(?3) = 0 OR r.source_type IN (SELECT value FROM json_each(?3)))
";

pub fn validate_representation(connection: &Connection) -> Result<()> {
    let sql: String = connection
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name = 'passage_vectors' AND type = 'table'",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let normalized: String = sql
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if normalized
        != "createvirtualtablepassage_vectorsusingvec0(passage_idintegerprimarykey,embeddingfloat[384])"
    {
        return Err(CommonplaceError::Conflict(
            "incompatible passage_vectors declaration; expected the version-2 384-dimensional float index; use a compatible store".into(),
        ));
    }
    for (table, key) in [("passage_fts", "rowid"), ("passage_vectors", "passage_id")] {
        let mismatch: bool = connection
            .query_row(
                &format!(
                    "WITH current AS (
                    SELECT p.passage_id FROM passages p JOIN document_revisions r USING(revision_id)
                    WHERE r.revision_number = (
                        SELECT max(revision_number) FROM document_revisions
                        WHERE document_id = r.document_id
                    )
                )
                SELECT EXISTS(SELECT passage_id FROM current EXCEPT SELECT {key} FROM {table})
                    OR EXISTS(SELECT {key} FROM {table} EXCEPT SELECT passage_id FROM current)"
                ),
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if mismatch {
            return Err(CommonplaceError::Conflict(format!(
                "{table} rows do not match current passages; create a fresh store and reingest sources"
            )));
        }
    }
    Ok(())
}

pub fn lexical(
    connection: &Connection,
    query: &str,
    filters: &SearchFilters,
) -> Result<Candidates> {
    select(
        connection,
        &format!(
            "SELECT p.passage_id FROM passage_fts
             JOIN passages p ON p.passage_id = passage_fts.rowid
             JOIN document_revisions r USING(revision_id)
             WHERE passage_fts MATCH ?1 AND {ELIGIBLE}
             ORDER BY bm25(passage_fts), p.passage_id LIMIT ?4"
        ),
        &query,
        filters,
    )
}

pub fn vector(
    connection: &Connection,
    embedding: &[f32],
    filters: &SearchFilters,
) -> Result<Candidates> {
    let bytes: Vec<u8> = embedding
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    select(
        connection,
        &format!(
            "SELECT p.passage_id FROM passage_vectors v
             JOIN passages p ON p.passage_id = v.passage_id
             JOIN document_revisions r USING(revision_id)
             WHERE {ELIGIBLE}
             ORDER BY vec_distance_L2(v.embedding, ?1), p.passage_id LIMIT ?4"
        ),
        &bytes,
        filters,
    )
}

fn select(
    connection: &Connection,
    sql: &str,
    query: &dyn rusqlite::ToSql,
    filters: &SearchFilters,
) -> Result<Candidates> {
    let mut statement = connection.prepare(sql).map_err(storage_error)?;
    let mut ids = statement
        .query_map(
            params![
                query,
                filters.since,
                serde_json::to_string(&filters.source_types)?,
                CANDIDATE_LIMIT + 1,
            ],
            |row| row.get::<_, i64>(0),
        )
        .map_err(storage_error)?
        .map(|row| PassageId::new(row.map_err(storage_error)?))
        .collect::<Result<Vec<_>>>()?;
    let truncated = ids.len() > CANDIDATE_LIMIT;
    ids.truncate(CANDIDATE_LIMIT);
    Ok(Candidates { ids, truncated })
}
