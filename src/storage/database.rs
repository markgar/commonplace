use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::Once;
use std::time::{Duration, Instant};

use rusqlite::ffi::sqlite3_auto_extension;
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use sqlite_vec::sqlite3_vec_init;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::{CommonplaceError, Result};

pub const STORE_FORMAT: &str = "commonplace-store/3";

const READ_BUSY_TIMEOUT: Duration = Duration::from_secs(2);

static REGISTER_SQLITE_VEC: Once = Once::new();

type SqliteExtensionEntry = unsafe extern "C" fn(
    *mut rusqlite::ffi::sqlite3,
    *mut *mut std::ffi::c_char,
    *const rusqlite::ffi::sqlite3_api_routines,
) -> std::ffi::c_int;

pub struct SqliteDatabase;

pub struct SqliteReadSession {
    connection: Connection,
}

impl SqliteReadSession {
    pub fn connection(&self) -> &Connection {
        &self.connection
    }
}

pub struct SqliteWriteSession {
    connection: Connection,
    // Keep the lock until the connection (and any transaction) has closed.
    _lock: File,
}

impl SqliteWriteSession {
    pub fn transaction(&mut self) -> Result<Transaction<'_>> {
        self.connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)
    }
}

impl SqliteDatabase {
    pub fn initialize(path: &Path) -> Result<()> {
        let mut connection = open_connection(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )?;
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
        Self::read_database(path).map(|_| ())
    }

    pub fn read(root: &Path) -> Result<SqliteReadSession> {
        super::StoreConfig::validate(root)?;
        Self::read_database(&root.join("commonplace.sqlite3"))
    }

    pub fn write(root: &Path, lock_timeout: Duration) -> Result<SqliteWriteSession> {
        // Validate before creating operational lock state or opening a writable handle.
        drop(Self::read(root)?);
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("writer.lock"))?;
        let start = Instant::now();
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) if start.elapsed() < lock_timeout => {
                    std::thread::sleep(
                        Duration::from_millis(10).min(lock_timeout.saturating_sub(start.elapsed())),
                    );
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    return Err(CommonplaceError::Conflict(
                        "store writer is busy; retry the operation".into(),
                    ));
                }
                Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
            }
        }
        let connection = open_connection(
            &root.join("commonplace.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )?;
        connection
            .busy_timeout(lock_timeout)
            .map_err(storage_error)?;
        validate_connection(&connection)?;
        Ok(SqliteWriteSession {
            connection,
            _lock: lock,
        })
    }

    fn read_database(path: &Path) -> Result<SqliteReadSession> {
        if !path.is_file() {
            return Err(CommonplaceError::Conflict(format!(
                "initialized store is missing its database: {}",
                path.display()
            )));
        }

        let connection = open_connection(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        connection
            .busy_timeout(READ_BUSY_TIMEOUT)
            .map_err(storage_error)?;
        connection
            .execute_batch("BEGIN DEFERRED")
            .map_err(storage_error)?;
        validate_connection(&connection)?;
        Ok(SqliteReadSession { connection })
    }
}

fn validate_connection(connection: &Connection) -> Result<()> {
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
            "unsupported store format {format:?}; expected {STORE_FORMAT:?}; use a fresh directory and explicitly reingest sources; existing data is not modified"
        )));
    }
    let (state_rows, state_ddl): (i64, String) = connection
        .query_row(
            "SELECT (SELECT count(*) FROM store_state), sql
             FROM sqlite_schema WHERE type='table' AND name='store_state'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(storage_error)?;
    if state_rows != 1 || !state_ddl.contains(&format!("CHECK (format = '{STORE_FORMAT}')")) {
        return Err(CommonplaceError::Conflict(
            "incompatible store_state layout; create a fresh store".into(),
        ));
    }
    if schema_version < 0 || knowledge_version < 0 {
        return Err(CommonplaceError::Conflict(
            "database contains invalid version counters".to_owned(),
        ));
    }
    let journal_mode: String = connection
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .map_err(storage_error)?;
    if journal_mode != "wal" {
        return Err(CommonplaceError::Conflict(
            "store must use WAL journal mode".into(),
        ));
    }
    let vec_version: String = connection
        .query_row("SELECT vec_version()", [], |row| row.get(0))
        .map_err(storage_error)?;
    if vec_version != "v0.1.6" {
        return Err(CommonplaceError::Conflict(format!(
            "unsupported sqlite-vec version {vec_version}"
        )));
    }
    for (table, module) in [("passage_fts", "fts5"), ("passage_vectors", "vec0")] {
        let sql: Option<String> = connection
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE name = ?1 AND type = 'table'",
                [table],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        if !sql.is_some_and(|sql| sql.contains(&format!("USING {module}("))) {
            return Err(CommonplaceError::Conflict(format!(
                "missing required {module} index"
            )));
        }
        connection
            .prepare(&format!("SELECT * FROM {table} LIMIT 0"))
            .map_err(storage_error)?;
    }
    Ok(())
}

fn open_connection(path: &Path, flags: OpenFlags) -> Result<Connection> {
    register_sqlite_vec();
    let connection = Connection::open_with_flags(path, flags).map_err(storage_error)?;
    connection
        .busy_timeout(Duration::ZERO)
        .map_err(storage_error)?;
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

pub(crate) fn storage_error(error: rusqlite::Error) -> CommonplaceError {
    match error.sqlite_error_code() {
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
            CommonplaceError::Conflict("database is busy; retry the operation".into())
        }
        _ => CommonplaceError::Storage(error.to_string()),
    }
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
