use std::path::Path;
use std::sync::Once;

use rusqlite::ffi::sqlite3_auto_extension;
use rusqlite::{Connection, OptionalExtension, params};
use sqlite_vec::sqlite3_vec_init;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::{CommonplaceError, Result};

pub const STORE_FORMAT: &str = "commonplace-store/1";

static REGISTER_SQLITE_VEC: Once = Once::new();

type SqliteExtensionEntry = unsafe extern "C" fn(
    *mut rusqlite::ffi::sqlite3,
    *mut *mut std::ffi::c_char,
    *const rusqlite::ffi::sqlite3_api_routines,
) -> std::ffi::c_int;

pub struct SqliteDatabase;

impl SqliteDatabase {
    pub fn initialize(path: &Path) -> Result<()> {
        let mut connection = open_connection(path)?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(storage_error)?;

        let created_at = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|error| CommonplaceError::Storage(error.to_string()))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        transaction
            .execute_batch(include_str!("schema.sql"))
            .map_err(storage_error)?;
        transaction
            .execute(
                "INSERT INTO store_state (
                    singleton, format, schema_version, knowledge_version, created_at
                 ) VALUES (1, ?1, 0, 0, ?2)",
                params![STORE_FORMAT, created_at],
            )
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
        connection
            .close()
            .map_err(|(_, error)| storage_error(error))
    }

    pub fn validate(path: &Path) -> Result<()> {
        if !path.is_file() {
            return Err(CommonplaceError::Conflict(format!(
                "initialized store is missing its database: {}",
                path.display()
            )));
        }

        let connection = open_connection(path)?;
        let state = connection
            .query_row(
                "SELECT format, schema_version, knowledge_version
                 FROM store_state
                 WHERE singleton = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(storage_error)?;

        let Some((format, schema_version, knowledge_version)) = state else {
            return Err(CommonplaceError::Conflict(
                "database has no store_state row".to_owned(),
            ));
        };
        if format != STORE_FORMAT {
            return Err(CommonplaceError::Conflict(format!(
                "unsupported store format {format:?}; expected {STORE_FORMAT:?}"
            )));
        }
        if schema_version < 0 || knowledge_version < 0 {
            return Err(CommonplaceError::Conflict(
                "database contains invalid version counters".to_owned(),
            ));
        }
        Ok(())
    }
}

fn open_connection(path: &Path) -> Result<Connection> {
    register_sqlite_vec();
    let connection = Connection::open(path).map_err(storage_error)?;
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(storage_error)?;
    Ok(connection)
}

fn register_sqlite_vec() {
    REGISTER_SQLITE_VEC.call_once(|| {
        // SAFETY: sqlite-vec exposes the exact entry-point ABI required by
        // sqlite3_auto_extension and remains loaded for the process lifetime.
        unsafe {
            sqlite3_auto_extension(Some(
                std::mem::transmute::<*const (), SqliteExtensionEntry>(
                    sqlite3_vec_init as *const (),
                ),
            ));
        }
    });
}

fn storage_error(error: rusqlite::Error) -> CommonplaceError {
    CommonplaceError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{STORE_FORMAT, SqliteDatabase};

    #[test]
    fn initializes_the_required_store_schema() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("commonplace.sqlite3");

        SqliteDatabase::initialize(&path).expect("initialize database");
        SqliteDatabase::validate(&path).expect("validate database");

        let connection = Connection::open(path).expect("open database");
        let format: String = connection
            .query_row("SELECT format FROM store_state", [], |row| row.get(0))
            .expect("read format");
        assert_eq!(format, STORE_FORMAT);

        let table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema
                 WHERE type IN ('table', 'view')
                   AND name IN (
                     'store_state', 'documents', 'document_revisions', 'passages',
                     'passage_fts', 'passage_vectors', 'entity_types',
                     'identifier_schemes', 'predicates',
                     'predicate_entity_types', 'entities', 'entity_aliases',
                     'entity_identifiers', 'knowledge_items',
                     'entity_type_memberships', 'facts',
                     'knowledge_item_evidence'
                   )",
                [],
                |row| row.get(0),
            )
            .expect("count schema objects");
        assert_eq!(table_count, 17);
    }
}
