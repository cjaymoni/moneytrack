//! What the store refuses, and what it could not do.
//!
//! Three rules shape this type. Nothing `rusqlite` owns appears in it, because a caller
//! that has to match on a SQLite error code is a caller the store has failed. Every
//! variant names the operation it failed during and the row it was about, because the
//! thing reading this is a crash report from a handset with no debugger attached. And no
//! variant carries an amount, a label or a counterparty: an error string travels, and
//! someone's rent is not a thing to put in one.

use mt_core::model::ValidationError;

/// Something the store would not do, or could not.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// The database file would not open, or a statement would not run.
    ///
    /// `operation` is what was being attempted, in the store's own words — `insert
    /// transaction`, `migrate`, `open` — and `reason` is SQLite's message. Entity and id
    /// are carried where the operation knows them.
    #[error("{operation} failed: {reason}")]
    Database {
        /// What was being attempted.
        operation: String,
        /// What SQLite said.
        reason: String,
    },

    /// The key is not the length the cipher wants.
    ///
    /// Never carries the key, obviously, and never the file path: both end up in logs.
    #[error("the database key must be {expected} bytes, not {supplied}")]
    KeyLength {
        /// Bytes wanted.
        expected: usize,
        /// Bytes supplied.
        supplied: usize,
    },

    /// The file exists and is not readable with this key.
    ///
    /// Either the key is wrong or the file is not a database. SQLCipher cannot tell those
    /// apart — a wrong key decrypts the header to noise, which is indistinguishable from a
    /// file that never held a header — so neither can this.
    #[error("the database could not be read: wrong key, or not a database")]
    Unreadable,

    /// The file was written by a newer build than this one.
    ///
    /// Refused rather than migrated backwards. A device that has synced a schema it does
    /// not understand would otherwise quietly drop the columns it cannot see.
    #[error("the database is at schema {found}, and this build knows {known}")]
    SchemaTooNew {
        /// What the file says.
        found: i64,
        /// What this build can run.
        known: i64,
    },

    /// A row that is in the database but will not read back into a core value.
    ///
    /// Reachable when a sync merge from a newer client writes a variant this build does
    /// not know, or when a column holds something the `CHECK` constraints do not cover.
    /// Named as loudly as a malformed input, because that is what it is.
    #[error("{entity} {id}: column {column} does not read back: {reason}")]
    UnreadableRow {
        /// Which table.
        entity: &'static str,
        /// Which row.
        id: String,
        /// Which column.
        column: &'static str,
        /// What is wrong with it.
        reason: String,
    },

    /// A row that does not satisfy the model's own rules.
    ///
    /// Checked before the insert rather than left to the schema's `CHECK` constraints: the
    /// core's message says which field and why, and SQLite's says `CHECK constraint
    /// failed`.
    #[error("{entity} is not valid: {reason}")]
    Invalid {
        /// Which table.
        entity: &'static str,
        /// What the core said.
        reason: String,
    },

    /// A row the operation needed is not there.
    #[error("no {entity} with id {id}")]
    NotFound {
        /// Which table.
        entity: &'static str,
        /// The id looked for.
        id: String,
    },

    /// A rule refused what the rows add up to — an overflowing balance walk, most likely.
    #[error("{rule} failed: {reason}")]
    Rule {
        /// Which rule.
        rule: &'static str,
        /// What the core said.
        reason: String,
    },

    /// The connection lock was left poisoned by a thread that panicked holding it.
    ///
    /// Not recoverable here: the database may be mid-statement, and the honest thing is to
    /// say so rather than take the lock anyway. Surfaced rather than swallowed because a
    /// panic in a mutex guard is a bug worth seeing.
    #[error("the database lock is poisoned: a thread failed while holding it")]
    LockPoisoned,
}

impl StoreError {
    /// Wraps a SQLite failure with the operation that hit it.
    pub(crate) fn database(operation: impl Into<String>, reason: &rusqlite::Error) -> Self {
        Self::Database {
            operation: operation.into(),
            reason: reason.to_string(),
        }
    }

    /// Wraps a row the core would not accept.
    pub(crate) fn invalid(entity: &'static str, reason: &ValidationError) -> Self {
        Self::Invalid {
            entity,
            reason: reason.to_string(),
        }
    }

    /// Wraps a column that will not read back into the value its type wants.
    pub(crate) fn column(
        entity: &'static str,
        id: impl core::fmt::Display,
        column: &'static str,
        reason: impl core::fmt::Display,
    ) -> Self {
        Self::UnreadableRow {
            entity,
            id: id.to_string(),
            column,
            reason: reason.to_string(),
        }
    }
}
