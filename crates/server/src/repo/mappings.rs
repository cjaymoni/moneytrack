//! The CSV column mapping kept for each account, so a person sets it up once.

use deadpool_postgres::Pool;
use mt_core::ids::{AccountId, UserId};
use mt_core::ingest::csv::Mapping;
use time::OffsetDateTime;

use crate::db;
use crate::error::ApiError;

/// The mapping this user last imported a CSV statement into this account with, if any.
///
/// A stored mapping this build cannot read or use is treated as absent and logged, not
/// refused: it was only ever a default for the form, and the person can set it again.
///
/// # Errors
/// [`ApiError::Internal`] if the query fails.
pub async fn get(
    pool: &Pool,
    user: UserId,
    account: AccountId,
) -> Result<Option<Mapping>, ApiError> {
    let client = db::client(pool).await?;
    let row = client
        .query_opt(
            "SELECT mapping::text AS mapping FROM import_mappings
              WHERE user_id = $1 AND account_id = $2",
            &[user.as_uuid(), account.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("read an import mapping", &error))?;
    let Some(row) = row else {
        return Ok(None);
    };
    let text: String = row.try_get("mapping").map_err(|error| ApiError::Internal {
        operation: "read an import mapping",
        reason: error.to_string(),
    })?;
    let mapping = serde_json::from_str::<Mapping>(&text)
        .ok()
        .filter(|mapping| mapping.validate().is_ok());
    if mapping.is_none() {
        tracing::warn!(%account, "a stored import mapping is not usable by this build; ignoring it");
    }
    Ok(mapping)
}

/// Keeps a mapping as this account's default.
///
/// # Errors
/// [`ApiError::Internal`] if it will not serialise or the write fails. The account is the
/// caller's to have checked; the composite foreign key refuses another user's.
pub async fn put(
    pool: &Pool,
    user: UserId,
    account: AccountId,
    mapping: &Mapping,
) -> Result<(), ApiError> {
    let text = serde_json::to_string(mapping).map_err(|error| ApiError::Internal {
        operation: "store an import mapping",
        reason: error.to_string(),
    })?;
    let now = OffsetDateTime::now_utc();
    let client = db::client(pool).await?;
    client
        .execute(
            "INSERT INTO import_mappings (user_id, account_id, mapping, updated_at)
             VALUES ($1, $2, $3::text::jsonb, $4::timestamptz::ledger_instant)
             ON CONFLICT (user_id, account_id)
             DO UPDATE SET mapping = EXCLUDED.mapping, updated_at = EXCLUDED.updated_at",
            &[user.as_uuid(), account.as_uuid(), &text, &now],
        )
        .await
        .map_err(|error| ApiError::database("store an import mapping", &error))?;
    Ok(())
}
