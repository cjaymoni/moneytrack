//! What the server is told at start, and nothing it is told at compile time.
//!
//! Every value here comes from the environment. No secret is in this repository, no default
//! stands in for one, and the server refuses to start rather than run with a placeholder: a
//! database URL that silently fell back to localhost is a server that looks healthy and
//! holds nobody's data.

use std::time::Duration;

/// How long a session lasts before it has to be signed in again.
const SESSION_DAYS: u64 = 30;

/// Everything the server needs to run.
#[derive(Debug, Clone)]
pub struct Config {
    /// Postgres connection string.
    pub database_url: String,
    /// Where to listen, as `host:port`.
    pub bind: String,
    /// Whether session cookies carry `Secure`.
    ///
    /// True everywhere a browser will ever see this server. False is for local development
    /// over plain HTTP, where a `Secure` cookie is simply never sent and nothing works.
    pub secure_cookies: bool,
    /// How long a session lives.
    pub session_ttl: Duration,
}

/// Why the server would not start.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A required variable is absent or empty.
    #[error("{name} is not set; see .env.example")]
    Missing {
        /// The variable's name.
        name: &'static str,
    },
    /// A variable is present and unusable.
    #[error("{name} is not {expected}")]
    Invalid {
        /// The variable's name.
        name: &'static str,
        /// What it should have been.
        expected: &'static str,
    },
}

impl Config {
    /// Reads the environment.
    ///
    /// # Errors
    /// [`ConfigError`] naming the variable at fault. Never names its value: these are
    /// secrets, and an error message is a thing that gets pasted into a chat window.
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            database_url: required("DATABASE_URL")?,
            bind: optional("BIND", "127.0.0.1:8080"),
            secure_cookies: flag("SECURE_COOKIES", true)?,
            session_ttl: Duration::from_secs(SESSION_DAYS * 24 * 60 * 60),
        })
    }
}

/// A variable that must be there and must not be blank.
fn required(name: &'static str) -> Result<String, ConfigError> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        _ => Err(ConfigError::Missing { name }),
    }
}

/// A variable with a safe default. Only for values that are not secret.
fn optional(name: &str, fallback: &str) -> String {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => value,
        _ => fallback.to_owned(),
    }
}

/// A boolean variable, spelled `true` or `false`.
///
/// Defaults to the safe answer rather than the convenient one: a missing `SECURE_COOKIES`
/// means secure cookies, so forgetting it in production cannot quietly send a session over
/// plain HTTP.
fn flag(name: &'static str, fallback: bool) -> Result<bool, ConfigError> {
    match std::env::var(name) {
        Err(_) => Ok(fallback),
        Ok(value) => match value.trim() {
            "" => Ok(fallback),
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(ConfigError::Invalid {
                name,
                expected: "`true` or `false`",
            }),
        },
    }
}
