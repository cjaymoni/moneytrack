//! People, and the device rows their sessions hang off.

use deadpool_postgres::Pool;
use mt_core::ids::{DeviceId, UserId};
use time::OffsetDateTime;
use tokio_postgres::error::SqlState;
use uuid::Uuid;

use crate::db;
use crate::error::ApiError;

/// A person, as the server holds them. No password hash crosses out of this module except
/// to [`crate::password::verify`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    /// Row id.
    pub id: UserId,
    /// The address they sign in with.
    pub email: String,
    /// When the account was created.
    pub created_at: OffsetDateTime,
}

/// Registers a person.
///
/// # Errors
/// [`ApiError::Conflict`] if the address is already registered, or [`ApiError::Internal`].
pub async fn create(pool: &Pool, email: &str, password_hash: &str) -> Result<User, ApiError> {
    let client = db::client(pool).await?;
    let created = client
        .query_one(
            "INSERT INTO users (id, email, password_hash)
             VALUES ($1, $2, $3)
             RETURNING id, email::text, created_at",
            &[&Uuid::now_v7(), &email, &password_hash],
        )
        .await;

    match created {
        Ok(row) => read(&row),
        // The partial unique index on live addresses is what makes this reachable. Reported
        // as a conflict rather than an internal error, because the caller can act on it.
        Err(error) if error.code() == Some(&SqlState::UNIQUE_VIOLATION) => {
            Err(ApiError::Conflict {
                reason: "that email is already registered".to_owned(),
            })
        }
        Err(error) => Err(ApiError::database("register a user", &error)),
    }
}

/// Finds a person by the address they sign in with, with their stored hash.
///
/// # Errors
/// [`ApiError::Internal`] if the query fails.
pub async fn by_email(pool: &Pool, email: &str) -> Result<Option<(User, String)>, ApiError> {
    let client = db::client(pool).await?;
    let found = client
        .query_opt(
            "SELECT id, email::text, created_at, password_hash
               FROM users
              WHERE email = $1::citext AND deleted_at IS NULL",
            &[&email],
        )
        .await
        .map_err(|error| ApiError::database("find a user", &error))?;

    match found {
        Some(row) => {
            let hash: String = row
                .try_get("password_hash")
                .map_err(|error| ApiError::database("read a stored hash", &error))?;
            Ok(Some((read(&row)?, hash)))
        }
        None => Ok(None),
    }
}

/// Reads the signed-in person.
///
/// # Errors
/// [`ApiError::Unauthenticated`] if the row is gone — a session outliving its user by a
/// moment is possible, and answering as though they were still there is not.
pub async fn by_id(pool: &Pool, id: UserId) -> Result<User, ApiError> {
    let client = db::client(pool).await?;
    let found = client
        .query_opt(
            "SELECT id, email::text, created_at FROM users
              WHERE id = $1 AND deleted_at IS NULL",
            &[id.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("read a user", &error))?;

    match found {
        Some(row) => read(&row),
        None => Err(ApiError::Unauthenticated),
    }
}

/// Finds this user's browser device, creating it the first time.
///
/// Every row the web client writes is attributed to it, which is what lets sync tell later
/// which client wrote what — and what lets "sign out my other devices" mean something.
///
/// # Errors
/// [`ApiError::Internal`] if the query or the insert fails.
pub async fn web_device(pool: &Pool, user: UserId) -> Result<DeviceId, ApiError> {
    let client = db::client(pool).await?;
    let existing = client
        .query_opt(
            "SELECT id FROM devices
              WHERE user_id = $1 AND platform = 'web' AND revoked_at IS NULL
              ORDER BY created_at
              LIMIT 1",
            &[user.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("find the web device", &error))?;

    if let Some(row) = existing {
        let id: Uuid = row
            .try_get("id")
            .map_err(|error| ApiError::database("read the web device", &error))?;
        return Ok(DeviceId::from(id));
    }

    let id = Uuid::now_v7();
    client
        .execute(
            "INSERT INTO devices (user_id, id, name, platform)
             VALUES ($1, $2, 'Browser', 'web')",
            &[user.as_uuid(), &id],
        )
        .await
        .map_err(|error| ApiError::database("record the web device", &error))?;
    Ok(DeviceId::from(id))
}

/// Reads a user row.
fn read(row: &tokio_postgres::Row) -> Result<User, ApiError> {
    let id: Uuid = row
        .try_get("id")
        .map_err(|error| ApiError::database("read a user", &error))?;
    let email: String = row
        .try_get("email")
        .map_err(|error| ApiError::database("read a user", &error))?;
    let created_at: OffsetDateTime = row
        .try_get("created_at")
        .map_err(|error| ApiError::database("read a user", &error))?;
    Ok(User {
        id: UserId::from(id),
        email,
        created_at,
    })
}
