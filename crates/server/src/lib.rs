//! `mt-server` — the API the browser talks to.
//!
//! Axum over Postgres, with `mt-core` linked in. Every number this server returns comes from
//! the same crate a handset runs locally, which is the whole reason the rules live in Rust:
//! a client and a server disagreeing about someone's balance is the bug class that destroys
//! trust in a finance app.
//!
//! ## What is here
//!
//! - [`api`] — the handlers. Thin: read rows, hand them to the core, render the answer.
//! - [`repo`] — every query, each one scoped to a user. No accessor takes less than a
//!   `UserId`, and a test fails the build if one appears.
//! - [`session`] — opaque session tokens in an `HttpOnly` cookie, stored as hashes.
//! - [`csrf`] — the token that covers what `SameSite=Lax` does not.
//! - [`password`] — Argon2id, in one place.
//! - [`limit`] — how fast anyone may try to sign in.
//! - [`db`] — the pool and the migrations.
//! - [`config`] — what the server is told at start, all of it from the environment.
//!
//! ## What is not here
//!
//! No rule. Nothing in this crate decides what counts as spending, how a transfer nets, or
//! where a currency's decimal point goes.

#![forbid(unsafe_code)]

pub mod api;
pub mod config;
pub mod csrf;
pub mod db;
pub mod error;
pub mod limit;
pub mod password;
pub mod repo;
pub mod session;

use axum::Router;
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use deadpool_postgres::Pool;
use std::sync::Arc;
use std::time::Duration;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::config::Config;
use crate::limit::Attempts;

/// The largest request body the server will read. Generous for a transaction, nowhere near
/// enough to be interesting as a way to use up memory. Import will raise it on its own
/// route when it exists.
const MAX_BODY: usize = 64 * 1024;

/// How long a request may take before it is cut off.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// What every handler is given.
#[derive(Clone)]
pub struct AppState {
    /// The connection pool.
    pub pool: Pool,
    /// What the server was told at start.
    pub config: Arc<Config>,
    /// Sign-in attempts, per address.
    pub attempts: Arc<Attempts>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // By hand, and without the config: it holds the database URL, which carries a
        // password, and a struct that prints itself ends up in a log eventually.
        f.debug_struct("AppState").finish_non_exhaustive()
    }
}

/// Assembles the router.
///
/// Everything is under `/api`, which is also what the session and CSRF cookies are scoped
/// to: a static client or a marketing page served from the same domain never receives them.
pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(api::health))
        .route("/csrf", get(csrf::token))
        .route("/auth/register", post(api::auth::register))
        .route("/auth/login", post(api::auth::login))
        .route("/auth/logout", post(api::auth::logout))
        .route("/me", get(api::auth::me))
        .route(
            "/accounts",
            get(api::ledger::list_accounts).post(api::ledger::create_account),
        )
        .route(
            "/transactions",
            get(api::ledger::list_transactions).post(api::ledger::create_transaction),
        )
        .route(
            "/transactions/{id}",
            patch(api::ledger::edit_transaction).delete(api::ledger::delete_transaction),
        )
        .route(
            "/transactions/{id}/restore",
            post(api::ledger::restore_transaction),
        )
        .route(
            "/transactions/{id}/category",
            post(api::ledger::set_category),
        )
        .route(
            "/categories",
            get(api::ledger::list_categories).post(api::ledger::create_category),
        )
        .route("/ledger", get(api::ledger::account_ledger))
        .route("/totals", get(api::ledger::currency_totals))
        .route("/breakdown", get(api::ledger::breakdown))
        .route("/currencies", get(api::ledger::currencies))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            csrf::guard,
        ))
        .with_state(state);

    Router::new()
        .nest("/api", api)
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            REQUEST_TIMEOUT,
        ))
        .layer(RequestBodyLimitLayer::new(MAX_BODY))
}
