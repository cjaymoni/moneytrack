//! `mt-store` — the device database.
//!
//! One person's ledger, in SQLite, encrypted at rest. This crate owns the schema, the
//! migrations, the repositories and the queries the screens run; `mt-core` below it owns
//! every decision about what the rows add up to, and knows nothing about any of this.
//!
//! ## Why this is in Rust
//!
//! Under Flutter it was not: drift held the schema and the queries, and one implementation
//! served both platforms because there was one app. Two native apps have no such thing, and
//! writing the schema, the migrations, the screen queries and the sync cycle once in Room
//! and once in GRDB is the duplication `mt-core` exists to prevent, one layer down. See
//! `docs/decisions/0001-native-uis.md`.
//!
//! ## The boundary this keeps
//!
//! Nothing `rusqlite` owns leaves this crate: no connection, no statement, no row, no error
//! code. Values go in and values come out, so the published surface above holds exactly one
//! handle — the [`Store`] — and everything else crossing into Swift and Kotlin is a value.
//!
//! ## What is here
//!
//! - [`open`] — opening, the key, the pragmas, and the handle itself.
//! - [`migrate`] — `db/sqlite/0001_init.sql`, compiled in and run.
//! - [`repo`] — one module per table, rows in and out as core values.
//! - [`query`] — the reads behind the screens.
//! - [`change`] — the revision counter and the change bus, which is what replaces drift's
//!   reactive queries.
//! - [`clock`] — the one impurity, behind a trait so tests can fix it.

// Restated although the workspace already forbids it. Nothing here touches a pointer or an
// FFI boundary: `rusqlite` does that behind its own API, which is one of the reasons to use
// it rather than `libsqlite3-sys` directly.
#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod change;
pub mod clock;
pub mod error;
mod migrate;
pub mod open;
pub mod query;
pub mod repo;
mod sql;

pub use change::{Change, ChangeObserver, SubscriptionId, Table};
pub use clock::{Clock, SystemClock};
pub use error::StoreError;
pub use migrate::SCHEMA_VERSION;
pub use open::{KEY_LEN, Store};
pub use repo::accounts::NewAccount;
pub use repo::transactions::NewTransaction;
