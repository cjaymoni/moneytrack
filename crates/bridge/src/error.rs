//! What the bridge refuses, and why.
//!
//! Everything arriving from Dart is untrusted: a row read out of SQLite by a client that is
//! a release ahead, an id typed into a test, a timestamp written by a device whose clock
//! disagrees with its database. The conversions in [`crate::dto`] validate at the boundary
//! and fail here rather than letting a malformed value reach a rule.
//!
//! Every variant names the field it came from. An error crossing the bridge is read by a
//! developer looking at a stack trace in Dart, where the Rust types are gone, so the string
//! has to carry enough to find the row — the field, what was supplied, and what was wanted.
//! Amounts and free text are not interpolated: a message that quotes someone's salary ends
//! up in a crash report.

/// A value that could not be converted, or a rule that refused the input.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BridgeError {
    /// An id was not a UUID.
    #[error("{field} is not a UUID: {value}")]
    InvalidId {
        /// The field it was supplied as.
        field: String,
        /// What was supplied.
        value: String,
    },

    /// A currency code was not three uppercase letters.
    #[error("{field} is not an ISO 4217 code: {value}")]
    InvalidCurrency {
        /// The field it was supplied as.
        field: String,
        /// What was supplied.
        value: String,
    },

    /// A timestamp was not RFC 3339.
    #[error("{field} is not an RFC 3339 timestamp: {value}")]
    InvalidTimestamp {
        /// The field it was supplied as.
        field: String,
        /// What was supplied.
        value: String,
    },

    /// A date was not `YYYY-MM-DD`.
    #[error("{field} is not a YYYY-MM-DD date: {value}")]
    InvalidDate {
        /// The field it was supplied as.
        field: String,
        /// What was supplied.
        value: String,
    },

    /// A value outside the range its field allows.
    #[error("{field} is out of range: {value}")]
    OutOfRange {
        /// The field it was supplied as.
        field: String,
        /// What was supplied.
        value: String,
    },

    /// A converted value could not be rendered back into its wire form. Reachable only for
    /// instants outside the calendar RFC 3339 can spell.
    #[error("{field} cannot be represented in its wire format")]
    Unrepresentable {
        /// The field being rendered.
        field: String,
    },

    /// A row that converted cleanly but is not valid as a row.
    #[error("{row}: {reason}")]
    InvalidRow {
        /// Which row type.
        row: String,
        /// What the core said.
        reason: String,
    },

    /// A rule refused the input. The core's own error, stringified — the variants are
    /// documented in `mt-core` and there is no Dart code that can usefully match on them.
    #[error("{rule} failed: {reason}")]
    Rule {
        /// Which rule: `walk`, `budget`, `recurrence`, `categorise`.
        rule: String,
        /// What the core said.
        reason: String,
    },
}

impl BridgeError {
    /// A conversion failure for a named field.
    pub(crate) fn rule(rule: &str, reason: impl core::fmt::Display) -> Self {
        Self::Rule {
            rule: rule.to_owned(),
            reason: reason.to_string(),
        }
    }

    /// A row the core would not accept.
    pub(crate) fn invalid_row(row: &str, reason: impl core::fmt::Display) -> Self {
        Self::InvalidRow {
            row: row.to_owned(),
            reason: reason.to_string(),
        }
    }
}
