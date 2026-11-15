//! Accounts: the places money sits. Every query is scoped to one user.

use deadpool_postgres::Pool;
use mt_core::enums::AccountKind;
use mt_core::ids::{AccountId, DeviceId, UserId};
use mt_core::model::{Account, SyncMeta};
use mt_core::money::{Currency, Money};
use time::OffsetDateTime;
use tokio_postgres::Row;
use uuid::Uuid;

use crate::db;
use crate::error::ApiError;
use crate::repo::rows::Reader;

/// The table, for error messages.
const ENTITY: &str = "account";

/// The columns every read selects, with the domains cast to types `sqlx` decodes.
const COLUMNS: &str = "id, name, kind, currency::text AS currency,
     opening_balance_minor::bigint AS opening_balance_minor, sort_order, archived_at,
     created_at::timestamptz AS created_at, updated_at::timestamptz AS updated_at,
     deleted_at, device_id";

/// What a caller supplies to open an account. No id, no timestamps, no device: those are
/// the server's, because a caller that mints its own id can mint the same one twice.
#[derive(Debug, Clone)]
pub struct NewAccount {
    /// What the user calls it.
    pub name: String,
    /// Cash, mobile money, bank or card.
    pub kind: AccountKind,
    /// The one currency it is denominated in.
    pub currency: Currency,
    /// What it held before any transaction.
    pub opening_balance: Money,
}

/// Opens an account for this user.
///
/// # Errors
/// [`ApiError::Invalid`] if the opening balance is in another currency or the row does not
/// satisfy the model, or [`ApiError::Internal`] if the insert fails.
pub async fn create(
    pool: &Pool,
    user: UserId,
    device: DeviceId,
    new: &NewAccount,
) -> Result<Account, ApiError> {
    if new.opening_balance.currency() != new.currency {
        return Err(ApiError::Invalid {
            field: "opening_balance".to_owned(),
            reason: format!(
                "it is in {} and the account holds {}",
                new.opening_balance.currency().code(),
                new.currency.code()
            ),
        });
    }

    let now = OffsetDateTime::now_utc();
    let account = Account {
        id: AccountId::from(Uuid::now_v7()),
        name: new.name.clone(),
        kind: new.kind,
        currency: new.currency,
        opening_balance: new.opening_balance,
        archived_at: None,
        sort_order: 0,
        sync: SyncMeta {
            created_at: now,
            updated_at: now,
            deleted_at: None,
            device_id: device,
        },
    };
    // Validated by the core before it reaches the schema, so the caller is told which field
    // and why rather than that a check constraint failed.
    account.validate().map_err(|reason| ApiError::Invalid {
        field: "account".to_owned(),
        reason: reason.to_string(),
    })?;

    // Each domain parameter is cast through its base type: `$5::text::currency_code`
    // rather than `$5::currency_code`. Cast straight to the domain and Postgres infers the
    // parameter *as* the domain, which the client refuses to serialise because it has
    // never heard of that type. Through `text` the parameter is plain text, and the
    // domain's check still runs on the way in — which is what the domain was for.
    let client = db::client(pool).await?;
    let row = client
        .query_one(
            &format!(
                "INSERT INTO accounts (
                     user_id, id, name, kind, currency, opening_balance_minor, sort_order,
                     created_at, updated_at, device_id
                 ) VALUES (
                     $1, $2, $3, $4, $5::text::currency_code, $6,
                     (SELECT coalesce(max(sort_order) + 1, 0) FROM accounts WHERE user_id = $1),
                     $7::timestamptz::ledger_instant, $7::timestamptz::ledger_instant, $8
                 )
                 RETURNING {COLUMNS}"
            ),
            &[
                user.as_uuid(),
                account.id.as_uuid(),
                &account.name,
                &account.kind.as_str(),
                &account.currency.code(),
                &account.opening_balance.minor(),
                &now,
                device.as_uuid(),
            ],
        )
        .await
        .map_err(|error| ApiError::database("insert an account", &error))?;

    read(&row)
}

/// Every account this user has that is not deleted, in display order.
///
/// # Errors
/// [`ApiError::Internal`] if the query fails or a row will not read back.
pub async fn list(pool: &Pool, user: UserId) -> Result<Vec<Account>, ApiError> {
    let client = db::client(pool).await?;
    let rows = client
        .query(
            &format!(
                "SELECT {COLUMNS} FROM accounts
                  WHERE user_id = $1 AND deleted_at IS NULL
                  ORDER BY sort_order, id"
            ),
            &[user.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("list accounts", &error))?;

    rows.iter().map(read).collect()
}

/// One account of this user's.
///
/// # Errors
/// [`ApiError::NotFound`] if there is no such account **or it belongs to someone else** —
/// one answer for both, because distinguishing them confirms the row exists.
pub async fn get(pool: &Pool, user: UserId, id: AccountId) -> Result<Account, ApiError> {
    let client = db::client(pool).await?;
    let found = client
        .query_opt(
            &format!(
                "SELECT {COLUMNS} FROM accounts
                  WHERE user_id = $1 AND id = $2 AND deleted_at IS NULL"
            ),
            &[user.as_uuid(), id.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("read an account", &error))?;

    match found {
        Some(row) => read(&row),
        None => Err(ApiError::NotFound { entity: ENTITY }),
    }
}

/// Reads one row.
fn read(row: &Row) -> Result<Account, ApiError> {
    let reader = Reader::new(ENTITY, row)?;
    let currency = reader.currency("currency")?;
    Ok(Account {
        id: reader.id(),
        name: reader.text("name")?,
        kind: reader.variant("kind")?,
        currency,
        opening_balance: reader.money("opening_balance_minor", currency)?,
        archived_at: reader.optional_instant("archived_at")?,
        sort_order: reader.narrowed("sort_order")?,
        sync: SyncMeta {
            created_at: reader.instant("created_at")?,
            updated_at: reader.instant("updated_at")?,
            deleted_at: reader.optional_instant("deleted_at")?,
            device_id: reader.uuid("device_id")?,
        },
    })
}
