use rusqlite::functions::FunctionFlags;
use rusqlite::{Connection, params};

use crate::domain::ids::{DocumentId, PassageId};
use crate::domain::search::{
    CANDIDATE_LIMIT, Candidates, ScopeCoverage, SearchFilters, TemporalCoverage,
};
use crate::{CommonplaceError, Result};

use super::database::storage_error;

// Normalized UTC timestamps sort chronologically after removing Z, including
// subsecond boundaries. Keep comparison textual to avoid SQLite's millisecond rounding.
const CURRENT: &str = "
    r.revision_number = (
        SELECT max(current.revision_number) FROM document_revisions current
        WHERE current.document_id = r.document_id
    )
";
const SOURCE_TYPES: &str = "
    AND (json_array_length(?3) = 0 OR r.source_type IN (SELECT value FROM json_each(?3)))
";
const PASSAGE_PHRASE: &str = "
    AND (?5 IS NULL OR passage_contains(p.text, ?5))
";
const TEMPORAL: &str = "
    AND ((?2 IS NULL AND ?6 IS NULL) OR r.temporal_state = 'timeless'
        OR (r.temporal_state = 'dated'
            AND (?2 IS NULL OR rtrim(r.occurred_at, 'Z') >= rtrim(?2, 'Z'))
            AND (?6 IS NULL OR rtrim(r.occurred_at, 'Z') <= rtrim(?6, 'Z'))))
";
const SCOPE: &str = "
    AND (?7 IS NULL OR r.document_id IN (SELECT value FROM json_each(?7)))
";

fn scope_json(filters: &SearchFilters) -> Result<Option<String>> {
    filters
        .document_ids
        .as_ref()
        .map(|ids| {
            serde_json::to_string(&ids.iter().map(|id| id.value()).collect::<Vec<_>>())
                .map_err(CommonplaceError::from)
        })
        .transpose()
}

pub fn temporal_coverage(
    connection: &Connection,
    filters: &SearchFilters,
) -> Result<TemporalCoverage> {
    connection.query_row(
        &format!("SELECT
            count(*) FILTER (WHERE r.temporal_state = 'dated'
                AND (?2 IS NULL OR rtrim(r.occurred_at, 'Z') >= rtrim(?2, 'Z'))
                AND (?6 IS NULL OR rtrim(r.occurred_at, 'Z') <= rtrim(?6, 'Z'))),
            count(*) FILTER (WHERE r.temporal_state = 'timeless'),
            count(*) FILTER (WHERE r.temporal_state = 'dated' AND ?2 IS NOT NULL AND rtrim(r.occurred_at, 'Z') < rtrim(?2, 'Z')),
            count(*) FILTER (WHERE r.temporal_state = 'dated' AND ?6 IS NOT NULL AND rtrim(r.occurred_at, 'Z') > rtrim(?6, 'Z')),
            count(*) FILTER (WHERE r.temporal_state = 'unknown')
            FROM document_revisions r WHERE {CURRENT} {SOURCE_TYPES} {SCOPE}"),
        params![rusqlite::types::Null, filters.since, serde_json::to_string(&filters.source_types)?,
            rusqlite::types::Null, rusqlite::types::Null, filters.until, scope_json(filters)?],
        |row| Ok(TemporalCoverage {
            eligible_dated: row.get(0)?,
            timeless: row.get(1)?,
            older_dated: row.get(2)?,
            newer_dated: row.get(3)?,
            excluded_unknown: row.get(4)?,
        }),
    ).map_err(storage_error)
}

pub fn scope_coverage(connection: &Connection, filters: &SearchFilters) -> Result<ScopeCoverage> {
    let ids = filters.document_ids.as_ref().ok_or_else(|| {
        CommonplaceError::Storage("scope coverage requires an explicit document scope".into())
    })?;
    let selected_sources = ids.len();
    let mut statement = connection
        .prepare(
            "SELECT value FROM json_each(?1)
         WHERE value NOT IN (SELECT document_id FROM documents) ORDER BY value",
        )
        .map_err(storage_error)?;
    let missing_document_ids = statement
        .query_map([scope_json(filters)?], |row| row.get::<_, i64>(0))
        .map_err(storage_error)?
        .map(|row| DocumentId::new(row.map_err(storage_error)?))
        .collect::<Result<Vec<_>>>()?;
    let existing_sources = selected_sources - missing_document_ids.len();
    let source_types = serde_json::to_string(&filters.source_types)?;
    let scope = scope_json(filters)?;
    let parameters = params![
        rusqlite::types::Null,
        filters.since,
        source_types,
        rusqlite::types::Null,
        filters.must_contain,
        filters.until,
        scope
    ];
    let source_type_sources: usize = connection
        .query_row(
            &format!(
                "SELECT count(*) FROM document_revisions r WHERE {CURRENT} {SOURCE_TYPES} {SCOPE}"
            ),
            parameters,
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let eligible_sources = connection.query_row(
        &format!("SELECT count(*) FROM document_revisions r WHERE {CURRENT} {SOURCE_TYPES} {SCOPE} {TEMPORAL}"),
        parameters, |row| row.get(0),
    ).map_err(storage_error)?;
    register_phrase(connection)?;
    let eligible_passages = connection
        .query_row(
            &format!(
                "SELECT count(*) FROM passages p JOIN document_revisions r USING(revision_id)
            WHERE {CURRENT} {SOURCE_TYPES} {SCOPE} {TEMPORAL} {PASSAGE_PHRASE}"
            ),
            parameters,
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    Ok(ScopeCoverage {
        supplied_ids: 0,
        duplicate_ids: 0,
        selected_sources,
        missing_document_ids,
        existing_sources,
        excluded_source_type: existing_sources - source_type_sources,
        eligible_sources,
        eligible_passages,
        selection_truncated: false,
        lexical_truncated: false,
        vector_truncated: false,
        fusion_truncated: false,
        result_truncated: false,
        diagnostics: vec![],
    })
}

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
            "incompatible passage_vectors declaration; expected the version-3 384-dimensional float index; use a compatible store".into(),
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
             WHERE passage_fts MATCH ?1 AND {CURRENT} {SOURCE_TYPES} {TEMPORAL} {PASSAGE_PHRASE} {SCOPE}
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
             WHERE {CURRENT} {SOURCE_TYPES} {TEMPORAL} {PASSAGE_PHRASE} {SCOPE}
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
    register_phrase(connection)?;
    let mut statement = connection.prepare(sql).map_err(storage_error)?;
    let mut ids = statement
        .query_map(
            params![
                query,
                filters.since,
                serde_json::to_string(&filters.source_types)?,
                CANDIDATE_LIMIT + 1,
                filters.must_contain,
                filters.until,
                scope_json(filters)?,
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

fn register_phrase(connection: &Connection) -> Result<()> {
    connection
        .create_scalar_function(
            "passage_contains",
            2,
            FunctionFlags::SQLITE_UTF8
                | FunctionFlags::SQLITE_DETERMINISTIC
                | FunctionFlags::SQLITE_INNOCUOUS,
            |context| {
                let text = context.get::<String>(0)?;
                let phrase = context.get::<String>(1)?;
                Ok(text.to_lowercase().contains(&phrase))
            },
        )
        .map_err(storage_error)
}
