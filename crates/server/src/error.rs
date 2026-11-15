//! What the API refuses, and how it says so.
//!
//! Every failure leaves here as a JSON body with a stable `code` and a message. The code is
//! what a client switches on; the message is for a person reading a log or a toast. Two
//! rules hold throughout.
//!
//! **A refusal never says more than the caller is entitled to know.** A sign-in that fails
//! says the same thing whether the address exists or not, and a row belonging to another
//! user is not found rather than forbidden — "forbidden" would confirm it exists.
//!
//! **A refusal never carries an amount, a label or a counterparty.** Errors end up in logs
//! and crash reports, and someone's rent is not a thing to put in one.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// Something the API would not do, or could not.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// No session, or one that has expired.
    #[error("not signed in")]
    Unauthenticated,

    /// Signed in, and the credentials presented do not match.
    ///
    /// Deliberately one variant for "no such address" and "wrong password": telling those
    /// apart turns the sign-in form into a way to find out who has an account.
    #[error("that email and password do not match")]
    BadCredentials,

    /// A request that changes something arrived without a valid CSRF token.
    #[error("missing or invalid CSRF token")]
    BadCsrf,

    /// A row the request needed does not exist, or belongs to someone else.
    #[error("no {entity} with that id")]
    NotFound {
        /// Which kind of row.
        entity: &'static str,
    },

    /// The request body is not usable. Names the field.
    #[error("{field} is not usable: {reason}")]
    Invalid {
        /// The field at fault.
        field: String,
        /// Why it was refused, never quoting a value that could be someone's money.
        reason: String,
    },

    /// A unique constraint the caller can do something about — an address already
    /// registered, most of all.
    #[error("{reason}")]
    Conflict {
        /// What already exists.
        reason: String,
    },

    /// A rule refused what the rows add up to.
    #[error("{rule} failed: {reason}")]
    Rule {
        /// Which rule.
        rule: &'static str,
        /// What the core said.
        reason: String,
    },

    /// Anything the caller cannot fix: the database is down, a query is wrong, a row will
    /// not read back. Logged in full; reported as five words.
    #[error("the server could not complete that")]
    Internal {
        /// What was being attempted, for the log.
        operation: &'static str,
        /// What went wrong, for the log. Never sent to the client.
        reason: String,
    },
}

impl ApiError {
    /// Wraps a database failure with the operation that hit it.
    ///
    /// The driver's own `Display` is the single word "db error" for anything the server
    /// sent, which is no use at three in the morning. Where Postgres said something — and
    /// for a failing migration or a violated constraint it always does — its message, its
    /// SQLSTATE and its detail are read out instead.
    pub(crate) fn database(operation: &'static str, error: &tokio_postgres::Error) -> Self {
        let reason = match error.as_db_error() {
            Some(db) => {
                let mut reason = format!("{}: {}", db.code().code(), db.message());
                if let Some(detail) = db.detail() {
                    reason.push_str(&format!(" ({detail})"));
                }
                if let Some(hint) = db.hint() {
                    reason.push_str(&format!(" [{hint}]"));
                }
                reason
            }
            None => error.to_string(),
        };
        Self::Internal { operation, reason }
    }

    /// The HTTP status this maps onto.
    const fn status(&self) -> StatusCode {
        match self {
            Self::Unauthenticated | Self::BadCredentials => StatusCode::UNAUTHORIZED,
            Self::BadCsrf => StatusCode::FORBIDDEN,
            Self::NotFound { .. } => StatusCode::NOT_FOUND,
            Self::Invalid { .. } | Self::Rule { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Conflict { .. } => StatusCode::CONFLICT,
            Self::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// The stable code a client switches on. Adding a variant adds one here; changing one of
    /// these strings is a breaking change to the API.
    const fn code(&self) -> &'static str {
        match self {
            Self::Unauthenticated => "unauthenticated",
            Self::BadCredentials => "bad_credentials",
            Self::BadCsrf => "bad_csrf",
            Self::NotFound { .. } => "not_found",
            Self::Invalid { .. } => "invalid",
            Self::Conflict { .. } => "conflict",
            Self::Rule { .. } => "rule_refused",
            Self::Internal { .. } => "internal",
        }
    }
}

/// The body every refusal carries.
#[derive(Debug, Serialize)]
struct Body {
    code: &'static str,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        // The internal variant is the one that knows things the client may not: it is
        // logged with its operation and its reason, and answered with neither.
        let message = match &self {
            Self::Internal { operation, reason } => {
                tracing::error!(operation, reason, "request failed");
                self.to_string()
            }
            other => other.to_string(),
        };
        (
            self.status(),
            Json(Body {
                code: self.code(),
                message,
            }),
        )
            .into_response()
    }
}

/// What a handler returns.
pub type ApiResult<T> = Result<T, ApiError>;
