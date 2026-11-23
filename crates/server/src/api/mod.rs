//! The handlers.
//!
//! Thin on purpose: read rows through [`crate::repo`], hand them to `mt-core`, render the
//! answer through [`dto`]. A handler that computes something is a handler in the wrong
//! layer — and, since the apps compute the same things locally, a second implementation of
//! a rule waiting to disagree with the first.

pub mod auth;
pub mod budgets;
pub mod dto;
pub mod ledger;

use axum::Json;

/// Whether the server is up. Says nothing about the database on purpose: a health check that
/// queries Postgres turns a slow database into a server the load balancer kills.
pub async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}
