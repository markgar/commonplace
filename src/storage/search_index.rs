use rusqlite::{Connection, Transaction, params};

use crate::Result;
use crate::domain::ids::{PassageId, RevisionId};

use super::database::storage_error;

pub fn remove_revision(transaction: &Transaction<'_>, revision_id: RevisionId) -> Result<()> {
    for table in ["passage_fts", "passage_vectors"] {
        let key = if table == "passage_fts" {
            "rowid"
        } else {
            "passage_id"
        };
        transaction.execute(
            &format!("DELETE FROM {table} WHERE {key} IN (SELECT passage_id FROM passages WHERE revision_id = ?1)"),
            [revision_id.value()],
        ).map_err(storage_error)?;
    }
    Ok(())
}

pub fn insert(
    transaction: &Transaction<'_>,
    passage_id: PassageId,
    text: &str,
    vector: &[f32],
) -> Result<()> {
    let bytes: Vec<u8> = vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    transaction
        .execute(
            "INSERT INTO passage_fts(rowid, text) VALUES (?1, ?2)",
            params![passage_id.value(), text],
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "INSERT INTO passage_vectors(passage_id, embedding) VALUES (?1, ?2)",
            params![passage_id.value(), bytes],
        )
        .map_err(storage_error)?;
    Ok(())
}

pub fn verify_document(
    connection: &Connection,
    document_id: i64,
    revision_id: RevisionId,
) -> Result<()> {
    for (table, key) in [("passage_fts", "rowid"), ("passage_vectors", "passage_id")] {
        let mismatch: bool = connection
            .query_row(
                &format!(
                    "SELECT EXISTS(
                    SELECT p.passage_id FROM passages p JOIN document_revisions r USING(revision_id)
                    LEFT JOIN {table} i ON i.{key} = p.passage_id
                    WHERE r.document_id = ?1 AND
                        ((r.revision_id = ?2 AND i.{key} IS NULL) OR
                         (r.revision_id != ?2 AND i.{key} IS NOT NULL))
                )"
                ),
                params![document_id, revision_id.value()],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if mismatch {
            return Err(crate::CommonplaceError::Storage(format!(
                "{table} rows do not match the current document passages"
            )));
        }
    }
    Ok(())
}
