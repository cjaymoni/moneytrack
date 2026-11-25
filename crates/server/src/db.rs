//! The connection pool, and getting a database to the schema this build runs.
//!
//! `db/postgres/0001_init.sql` is the schema, the same file `docs/SCHEMA.md` describes and
//! `crates/core/tests/schema_agreement.rs` checks against the Rust types. It is compiled in
//! and applied to an empty database, and the revisions that follow it will be applied the
//! same way.

use core::str::FromStr;
use deadpool_postgres::{Config, Manager, ManagerConfig, Pool, RecyclingMethod, Runtime};
use tokio_postgres::NoTls;

use crate::error::ApiError;

/// How many connections the pool will open.
const MAX_CONNECTIONS: usize = 10;

/// The migrations, in order. Each is applied once and recorded by name.
const MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_init",
        include_str!("../../../db/postgres/0001_init.sql"),
    ),
    (
        "0002_import",
        include_str!("../../../db/postgres/0002_import.sql"),
    ),
];

/// Opens the pool.
///
/// # Errors
/// [`ApiError::Internal`] if the URL will not parse or the pool cannot be built.
pub fn connect(database_url: &str) -> Result<Pool, ApiError> {
    let pg =
        tokio_postgres::Config::from_str(database_url).map_err(|error| ApiError::Internal {
            operation: "read DATABASE_URL",
            // The URL itself carries a password and is never put in an error.
            reason: error.to_string(),
        })?;

    let mut config = Config::new();
    config.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    let manager = Manager::from_config(
        pg,
        // Plain TCP, which is honest for a loopback container and wrong for anything else.
        // A managed Postgres reached over a network needs TLS here, and that belongs with
        // the deployment decision rather than ahead of it — see §10 of ARCHITECTURE.md.
        NoTls,
        config.get_manager_config(),
    );
    Pool::builder(manager)
        .max_size(MAX_CONNECTIONS)
        .runtime(Runtime::Tokio1)
        .build()
        .map_err(|error| ApiError::Internal {
            operation: "build the connection pool",
            reason: error.to_string(),
        })
}

/// Brings a database up to the schema this build runs.
///
/// Each migration runs inside a transaction with its name recorded in the same transaction,
/// so a migration that fails half way leaves nothing behind and is retried in full. Already
/// applied migrations are skipped.
///
/// # Errors
/// [`ApiError::Internal`] if a migration will not run.
pub async fn migrate(pool: &Pool) -> Result<(), ApiError> {
    let mut client = pool.get().await.map_err(|error| ApiError::Internal {
        operation: "take a connection",
        reason: error.to_string(),
    })?;

    client
        .execute(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                 name       TEXT PRIMARY KEY,
                 applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
             )",
            &[],
        )
        .await
        .map_err(|error| ApiError::database("create the migration ledger", &error))?;

    for (name, sql) in MIGRATIONS {
        let applied = client
            .query_opt(
                "SELECT name FROM schema_migrations WHERE name = $1",
                &[name],
            )
            .await
            .map_err(|error| ApiError::database("read the migration ledger", &error))?;
        if applied.is_some() {
            continue;
        }

        let transaction = client
            .transaction()
            .await
            .map_err(|error| ApiError::database("begin a migration", &error))?;
        // The file carries its own BEGIN/COMMIT for running by hand through psql; inside a
        // transaction those are a no-op with a warning, which is the lesser evil against
        // keeping two copies of the schema.
        transaction
            .batch_execute(sql)
            .await
            .map_err(|error| ApiError::database("apply a migration", &error))?;
        transaction
            .execute("INSERT INTO schema_migrations (name) VALUES ($1)", &[name])
            .await
            .map_err(|error| ApiError::database("record a migration", &error))?;
        transaction
            .commit()
            .await
            .map_err(|error| ApiError::database("commit a migration", &error))?;

        tracing::info!(migration = name, "applied");
    }
    Ok(())
}

/// Takes a connection out of the pool.
///
/// # Errors
/// [`ApiError::Internal`] if none is available before the pool's timeout.
pub async fn client(pool: &Pool) -> Result<deadpool_postgres::Object, ApiError> {
    pool.get().await.map_err(|error| ApiError::Internal {
        operation: "take a connection",
        reason: error.to_string(),
    })
}

/// Whether a database error is a violation of the named constraint or unique index.
///
/// For the few places a constraint is an answer the caller can act on — a duplicate bank
/// reference, a fingerprint already seen — rather than a bug to report.
#[must_use]
pub fn violates(error: &tokio_postgres::Error, constraint: &str) -> bool {
    error
        .as_db_error()
        .is_some_and(|db| db.constraint() == Some(constraint))
}
