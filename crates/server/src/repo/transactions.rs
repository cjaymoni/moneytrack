//! Transactions: the rows everything else is derived from. Every query is scoped to one user.

use deadpool_postgres::Pool;
use mt_core::enums::{Direction, TxSource, TxStatus};
use mt_core::ids::{AccountId, CategoryId, DeviceId, TxId, UserId};
use mt_core::model::{SyncMeta, Transaction};
use mt_core::money::Money;
use time::OffsetDateTime;
use tokio_postgres::Row;
use uuid::Uuid;

use crate::db;
use crate::error::ApiError;
use crate::repo::accounts;
use crate::repo::rows::Reader;

/// The table, for error messages.
const ENTITY: &str = "transaction";

/// The columns every read selects, with the domains cast to types `sqlx` decodes.
const COLUMNS: &str = "id, account_id, direction, amount_minor::bigint AS amount_minor,
     currency::text AS currency, booked_at::timestamptz AS booked_at, category_id, label,
     counterparty, note, status, source, source_ref, external_id, recurring_id, transfer_id,
     created_at::timestamptz AS created_at, updated_at::timestamptz AS updated_at,
     deleted_at, device_id";

/// What a caller supplies to record a transaction.
#[derive(Debug, Clone)]
pub struct NewTransaction {
    /// The account it moves.
    pub account_id: AccountId,
    /// In or out, from that account's point of view.
    pub direction: Direction,
    /// Always positive; `direction` carries the sign. Must be in the account's currency.
    pub amount: Money,
    /// When it happened, or is expected to. May be in the future: that is the forecast.
    pub booked_at: OffsetDateTime,
    /// The category, absent for uncategorised.
    pub category_id: Option<CategoryId>,
    /// What it was.
    pub label: String,
    /// Merchant, sender or recipient.
    pub counterparty: Option<String>,
    /// Free text.
    pub note: Option<String>,
    /// Pending or confirmed.
    pub status: TxStatus,
    /// Where the row came from.
    pub source: TxSource,
}

/// Records a transaction for this user.
///
/// The account is read first, under this user's id: a row naming an account that is not
/// theirs is not found, rather than an insert that the composite foreign key would have
/// refused with a message about constraints.
///
/// # Errors
/// [`ApiError::NotFound`] if the account is not this user's, [`ApiError::Invalid`] for a
/// currency that does not match or a row the model refuses, or [`ApiError::Internal`].
pub async fn create(
    pool: &Pool,
    user: UserId,
    device: DeviceId,
    new: &NewTransaction,
) -> Result<Transaction, ApiError> {
    let account = accounts::get(pool, user, new.account_id).await?;
    if new.amount.currency() != account.currency {
        return Err(ApiError::Invalid {
            field: "amount".to_owned(),
            reason: format!(
                "it is in {} and the account holds {}",
                new.amount.currency().code(),
                account.currency.code()
            ),
        });
    }

    let now = OffsetDateTime::now_utc();
    let transaction = Transaction {
        id: TxId::from(Uuid::now_v7()),
        account_id: new.account_id,
        direction: new.direction,
        amount: new.amount,
        booked_at: new.booked_at,
        category_id: new.category_id,
        label: new.label.clone(),
        counterparty: new.counterparty.clone(),
        note: new.note.clone(),
        status: new.status,
        source: new.source,
        source_ref: None,
        external_id: None,
        recurring_id: None,
        transfer_id: None,
        sync: SyncMeta {
            created_at: now,
            updated_at: now,
            deleted_at: None,
            device_id: device,
        },
    };
    transaction.validate().map_err(|reason| ApiError::Invalid {
        field: "transaction".to_owned(),
        reason: reason.to_string(),
    })?;

    let category = transaction.category_id.map(|id| *id.as_uuid());
    // Domain parameters are cast through their base types; see the note in `repo::accounts`.
    let client = db::client(pool).await?;
    let row = client
        .query_one(
            &format!(
                "INSERT INTO transactions (
                     user_id, id, account_id, direction, amount_minor, currency, booked_at,
                     category_id, label, counterparty, note, status, source,
                     created_at, updated_at, device_id
                 ) VALUES (
                     $1, $2, $3, $4, $5::bigint::amount_minor, $6::text::currency_code,
                     $7::timestamptz::ledger_instant,
                     $8, $9, $10, $11, $12, $13,
                     $14::timestamptz::ledger_instant, $14::timestamptz::ledger_instant, $15
                 )
                 RETURNING {COLUMNS}"
            ),
            &[
                user.as_uuid(),
                transaction.id.as_uuid(),
                transaction.account_id.as_uuid(),
                &transaction.direction.as_str(),
                &transaction.amount.minor(),
                &transaction.amount.currency().code(),
                &transaction.booked_at,
                &category,
                &transaction.label,
                &transaction.counterparty,
                &transaction.note,
                &transaction.status.as_str(),
                &transaction.source.as_str(),
                &now,
                device.as_uuid(),
            ],
        )
        .await
        .map_err(|error| ApiError::database("insert a transaction", &error))?;

    read(&row)
}

/// Every live row in one of this user's accounts, oldest first.
///
/// Pending rows included: the basis a balance is asked on decides what counts, and a query
/// that dropped them here would make the outlook impossible to ask for.
///
/// # Errors
/// [`ApiError::NotFound`] if the account is not this user's, or [`ApiError::Internal`].
pub async fn for_account(
    pool: &Pool,
    user: UserId,
    account_id: AccountId,
) -> Result<Vec<Transaction>, ApiError> {
    // Checked rather than filtered: an account that is not this user's is not found, and an
    // empty list would have said "you have no transactions" about someone else's account.
    accounts::get(pool, user, account_id).await?;

    let client = db::client(pool).await?;
    let rows = client
        .query(
            &format!(
                "SELECT {COLUMNS} FROM transactions
                  WHERE user_id = $1 AND account_id = $2 AND deleted_at IS NULL
                  ORDER BY booked_at, id"
            ),
            &[user.as_uuid(), account_id.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("list transactions", &error))?;

    rows.iter().map(read).collect()
}

/// Every live row this user has, across every account.
///
/// # Errors
/// [`ApiError::Internal`] if the query fails or a row will not read back.
pub async fn all(pool: &Pool, user: UserId) -> Result<Vec<Transaction>, ApiError> {
    let client = db::client(pool).await?;
    let rows = client
        .query(
            &format!(
                "SELECT {COLUMNS} FROM transactions
                  WHERE user_id = $1 AND deleted_at IS NULL
                  ORDER BY booked_at, id"
            ),
            &[user.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("list transactions", &error))?;

    rows.iter().map(read).collect()
}

/// Reads one row.
fn read(row: &Row) -> Result<Transaction, ApiError> {
    let reader = Reader::new(ENTITY, row)?;
    let currency = reader.currency("currency")?;
    Ok(Transaction {
        id: reader.id(),
        account_id: reader.uuid("account_id")?,
        direction: reader.variant("direction")?,
        amount: reader.money("amount_minor", currency)?,
        booked_at: reader.instant("booked_at")?,
        category_id: reader.optional_uuid("category_id")?,
        label: reader.text("label")?,
        counterparty: reader.optional_text("counterparty")?,
        note: reader.optional_text("note")?,
        status: reader.variant("status")?,
        source: reader.variant("source")?,
        source_ref: reader.optional_text("source_ref")?,
        external_id: reader.optional_text("external_id")?,
        recurring_id: reader.optional_uuid("recurring_id")?,
        transfer_id: reader.optional_uuid("transfer_id")?,
        sync: SyncMeta {
            created_at: reader.instant("created_at")?,
            updated_at: reader.instant("updated_at")?,
            deleted_at: reader.optional_instant("deleted_at")?,
            device_id: reader.uuid("device_id")?,
        },
    })
}
