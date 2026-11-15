//! Running the schema, and refusing a file this build does not understand.
//!
//! `db/sqlite/0001_init.sql` is not a description of the device database any more. It is
//! the device database: it is compiled into this crate and executed on first open. Under
//! Flutter it was a statement of intent that a drift schema mirrored, and keeping the two
//! in agreement was a job for a test. There is nothing to mirror now.

use rusqlite::Connection;

use crate::error::StoreError;

/// The schema this build runs.
pub const SCHEMA_VERSION: i64 = 1;

/// Revision 1, verbatim. `include_str!` rather than a copy, so there is one file.
const INIT_SQL: &str = include_str!("../../../db/sqlite/0001_init.sql");

/// Brings an open connection up to [`SCHEMA_VERSION`].
///
/// An empty file gets the schema. A file already at this version is left alone. A file from
/// a newer build is refused rather than opened: this build would not see the columns it
/// does not know about, and would then write rows that silently dropped them.
///
/// # Errors
/// [`StoreError::SchemaTooNew`] for a file from a later build, or
/// [`StoreError::Database`] if the schema will not run.
pub(crate) fn migrate(connection: &mut Connection) -> Result<(), StoreError> {
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| StoreError::database("read the schema version", &error))?;

    if version == SCHEMA_VERSION {
        return Ok(());
    }
    if version > SCHEMA_VERSION {
        return Err(StoreError::SchemaTooNew {
            found: version,
            known: SCHEMA_VERSION,
        });
    }

    // One transaction: a half-applied schema is worse than none, and SQLite is happy to
    // run DDL transactionally. The `PRAGMA foreign_keys` line inside the script is a no-op
    // in here, which is why the connection sets it on the way in instead.
    let transaction = connection
        .transaction()
        .map_err(|error| StoreError::database("begin the migration", &error))?;
    transaction
        .execute_batch(INIT_SQL)
        .map_err(|error| StoreError::database("apply schema revision 1", &error))?;
    transaction
        .commit()
        .map_err(|error| StoreError::database("commit the migration", &error))
}
