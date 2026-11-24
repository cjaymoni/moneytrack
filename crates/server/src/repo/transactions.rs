//! Transactions: the rows everything else is derived from. Every query is scoped to one user.

use std::collections::HashMap;

use deadpool_postgres::{GenericClient, Pool};
use mt_core::enums::{Direction, TxSource, TxStatus};
use mt_core::ids::{AccountId, CategoryId, DeviceId, TxId, UserId};
use mt_core::model::{SyncMeta, Transaction};
use mt_core::money::Money;
use time::OffsetDateTime;
use tokio_postgres::Row;
use uuid::Uuid;

use crate::db;
use crate::error::ApiError;
use crate::repo::rows::Reader;
use crate::repo::{accounts, categories};

/// The table, for error messages.
const ENTITY: &str = "transaction";

/// The unique index on a bank's own reference, per account.
const EXTERNAL_ID_INDEX: &str = "transactions_external";

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
    // Read under this user's id for the same reason as the account: someone else's category
    // is not found, rather than a foreign-key refusal that would confirm it exists.
    if let Some(category) = new.category_id {
        categories::get(pool, user, category).await?;
    }
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

    let client = db::client(pool).await?;
    insert(&client, user, &transaction).await
}

/// Writes a validated transaction on a client the caller holds, so it can share a database
/// transaction with whatever else the caller is writing.
///
/// The account and category are not re-read here: the composite foreign keys refuse a row
/// naming either of another user's, and the caller has already read them under this user's
/// id to give a better answer than a constraint name.
///
/// # Errors
/// [`ApiError::Conflict`] if the account already holds a live row with the same bank
/// reference, or [`ApiError::Internal`].
pub(crate) async fn insert(
    client: &impl GenericClient,
    user: UserId,
    transaction: &Transaction,
) -> Result<Transaction, ApiError> {
    let category = transaction.category_id.map(|id| *id.as_uuid());
    // Domain parameters are cast through their base types; see the note in `repo::accounts`.
    let row = client
        .query_one(
            &format!(
                "INSERT INTO transactions (
                     user_id, id, account_id, direction, amount_minor, currency, booked_at,
                     category_id, label, counterparty, note, status, source, source_ref,
                     external_id, created_at, updated_at, device_id
                 ) VALUES (
                     $1, $2, $3, $4, $5::bigint::amount_minor, $6::text::currency_code,
                     $7::timestamptz::ledger_instant,
                     $8, $9, $10, $11, $12, $13, $14, $15,
                     $16::timestamptz::ledger_instant, $17::timestamptz::ledger_instant, $18
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
                &transaction.source_ref,
                &transaction.external_id,
                &transaction.sync.created_at,
                &transaction.sync.updated_at,
                transaction.sync.device_id.as_uuid(),
            ],
        )
        .await
        .map_err(|error| {
            if db::violates(&error, EXTERNAL_ID_INDEX) {
                return ApiError::Conflict {
                    reason: "a row with the same bank reference is already in this account"
                        .to_owned(),
                };
            }
            ApiError::database("insert a transaction", &error)
        })?;

    read(&row)
}

/// Writes several validated transactions in one statement, on the caller's client, and
/// returns them in the order given.
///
/// As [`insert`], the caller has already read each account and category under this user's
/// id and settled which rows keep a bank reference; the composite foreign keys and the
/// reference index are the backstop.
///
/// # Errors
/// [`ApiError::Conflict`] if a bank reference was written to one of the accounts by another
/// request since the caller checked, or [`ApiError::Internal`].
pub(crate) async fn insert_many(
    client: &impl GenericClient,
    user: UserId,
    transactions: &[Transaction],
) -> Result<Vec<Transaction>, ApiError> {
    if transactions.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<Uuid> = transactions.iter().map(|t| *t.id.as_uuid()).collect();
    let accounts: Vec<Uuid> = transactions
        .iter()
        .map(|t| *t.account_id.as_uuid())
        .collect();
    let directions: Vec<&str> = transactions.iter().map(|t| t.direction.as_str()).collect();
    let amounts: Vec<i64> = transactions.iter().map(|t| t.amount.minor()).collect();
    let currencies: Vec<String> = transactions
        .iter()
        .map(|t| t.amount.currency().code().to_owned())
        .collect();
    let booked: Vec<OffsetDateTime> = transactions.iter().map(|t| t.booked_at).collect();
    let categories: Vec<Option<Uuid>> = transactions
        .iter()
        .map(|t| t.category_id.map(|id| *id.as_uuid()))
        .collect();
    let labels: Vec<&str> = transactions.iter().map(|t| t.label.as_str()).collect();
    let counterparties: Vec<Option<&str>> = transactions
        .iter()
        .map(|t| t.counterparty.as_deref())
        .collect();
    let notes: Vec<Option<&str>> = transactions.iter().map(|t| t.note.as_deref()).collect();
    let statuses: Vec<&str> = transactions.iter().map(|t| t.status.as_str()).collect();
    let sources: Vec<&str> = transactions.iter().map(|t| t.source.as_str()).collect();
    let source_refs: Vec<Option<&str>> = transactions
        .iter()
        .map(|t| t.source_ref.as_deref())
        .collect();
    let external_ids: Vec<Option<&str>> = transactions
        .iter()
        .map(|t| t.external_id.as_deref())
        .collect();
    let created: Vec<OffsetDateTime> = transactions.iter().map(|t| t.sync.created_at).collect();
    let updated: Vec<OffsetDateTime> = transactions.iter().map(|t| t.sync.updated_at).collect();
    let devices: Vec<Uuid> = transactions
        .iter()
        .map(|t| *t.sync.device_id.as_uuid())
        .collect();

    // Domain parameters are cast through their base types; see the note in `repo::accounts`.
    let rows = client
        .query(
            &format!(
                "INSERT INTO transactions (
                     user_id, id, account_id, direction, amount_minor, currency, booked_at,
                     category_id, label, counterparty, note, status, source, source_ref,
                     external_id, created_at, updated_at, device_id
                 )
                 SELECT $1, row.id, row.account_id, row.direction, row.amount::amount_minor,
                        row.currency::currency_code, row.booked_at::ledger_instant,
                        row.category_id, row.label, row.counterparty, row.note, row.status,
                        row.source, row.source_ref, row.external_id,
                        row.created_at::ledger_instant, row.updated_at::ledger_instant,
                        row.device_id
                   FROM UNNEST($2::uuid[], $3::uuid[], $4::text[], $5::bigint[], $6::text[],
                               $7::timestamptz[], $8::uuid[], $9::text[], $10::text[],
                               $11::text[], $12::text[], $13::text[], $14::text[], $15::text[],
                               $16::timestamptz[], $17::timestamptz[], $18::uuid[])
                        AS row(id, account_id, direction, amount, currency, booked_at,
                               category_id, label, counterparty, note, status, source,
                               source_ref, external_id, created_at, updated_at, device_id)
                 RETURNING {COLUMNS}"
            ),
            &[
                user.as_uuid(),
                &ids,
                &accounts,
                &directions,
                &amounts,
                &currencies,
                &booked,
                &categories,
                &labels,
                &counterparties,
                &notes,
                &statuses,
                &sources,
                &source_refs,
                &external_ids,
                &created,
                &updated,
                &devices,
            ],
        )
        .await
        .map_err(|error| {
            if db::violates(&error, EXTERNAL_ID_INDEX) {
                return ApiError::Conflict {
                    reason: "a payment with the same bank reference was added to one of these \
                             accounts a moment ago; reload and try again"
                        .to_owned(),
                };
            }
            ApiError::database("insert transactions", &error)
        })?;

    // RETURNING promises no order, so the rows are put back in the order they were given.
    let mut by_id: HashMap<TxId, Transaction> = rows
        .iter()
        .map(|row| read(row).map(|saved| (saved.id, saved)))
        .collect::<Result<_, _>>()?;
    transactions
        .iter()
        .map(|wanted| {
            by_id.remove(&wanted.id).ok_or_else(|| ApiError::Internal {
                operation: "insert transactions",
                reason: format!("transaction {} was not returned by its insert", wanted.id),
            })
        })
        .collect()
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

/// One live row of this user's.
///
/// # Errors
/// [`ApiError::NotFound`] if there is no such row or it belongs to someone else.
pub async fn get(pool: &Pool, user: UserId, id: TxId) -> Result<Transaction, ApiError> {
    let client = db::client(pool).await?;
    let found = client
        .query_opt(
            &format!(
                "SELECT {COLUMNS} FROM transactions
                  WHERE user_id = $1 AND id = $2 AND deleted_at IS NULL"
            ),
            &[user.as_uuid(), id.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("read a transaction", &error))?;

    match found {
        Some(row) => read(&row),
        None => Err(ApiError::NotFound { entity: ENTITY }),
    }
}

/// Files a row under a category, or under none.
///
/// The row's `updated_at` and `device_id` move with it, because sync decides which of two
/// edits wins by when they were made and on which device.
///
/// # Errors
/// [`ApiError::NotFound`] if the row or the category is not this user's,
/// [`ApiError::Invalid`] for a transfer leg, or [`ApiError::Internal`].
pub async fn set_category(
    pool: &Pool,
    user: UserId,
    device: DeviceId,
    id: TxId,
    category: Option<CategoryId>,
) -> Result<Transaction, ApiError> {
    let existing = get(pool, user, id).await?;
    if let Some(category) = category {
        categories::get(pool, user, category).await?;
        // A transfer is netted, not spent: filing one leg under a category would count
        // moving money between your own accounts as spending. The schema refuses it too.
        if existing.transfer_id.is_some() {
            return Err(ApiError::Invalid {
                field: "category_id".to_owned(),
                reason: "a transfer between your own accounts is not filed under a category"
                    .to_owned(),
            });
        }
    }

    let now = OffsetDateTime::now_utc();
    let category = category.map(|id| *id.as_uuid());
    let client = db::client(pool).await?;
    let row = client
        .query_one(
            &format!(
                "UPDATE transactions
                    SET category_id = $3,
                        updated_at = greatest($4::timestamptz, created_at::timestamptz)
                                     ::ledger_instant,
                        device_id = $5
                  WHERE user_id = $1 AND id = $2 AND deleted_at IS NULL
                  RETURNING {COLUMNS}"
            ),
            &[
                user.as_uuid(),
                id.as_uuid(),
                &category,
                &now,
                device.as_uuid(),
            ],
        )
        .await
        .map_err(|error| ApiError::database("file a transaction", &error))?;

    read(&row)
}

/// What a caller may change about a row. Everything a person typed; not the account it is
/// in, where it came from, or anything sync owns.
///
/// The account is fixed because moving a row between accounts can change its currency, and
/// an amount typed in cedis does not become dollars by being moved. A wrong account is a
/// delete and a new row.
#[derive(Debug, Clone)]
pub struct Edit {
    /// In or out.
    pub direction: Direction,
    /// Always positive. Must be in the account's currency.
    pub amount: Money,
    /// When it happened, or is expected to.
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
}

/// Refuses a change to one leg of a transfer.
///
/// The two legs must stay equal and opposite, and `mt-core` checks that over the pair. Changing
/// one alone would leave a transfer that moves money out of existence, so it is refused here
/// until transfers have an editor that edits both.
fn not_a_transfer(row: &Transaction) -> Result<(), ApiError> {
    if row.is_transfer() {
        return Err(ApiError::Invalid {
            field: "transaction".to_owned(),
            reason: "it is one leg of a transfer, and the two legs change together".to_owned(),
        });
    }
    Ok(())
}

/// Changes a row.
///
/// The new values are checked by the model before they reach the schema, as a new row's are.
/// `updated_at` and `device_id` move with the change, because sync decides between competing
/// edits by both.
///
/// # Errors
/// [`ApiError::NotFound`] if the row or the category is not this user's,
/// [`ApiError::Invalid`] for a transfer leg, a currency that does not match or values the
/// model refuses, or [`ApiError::Internal`].
pub async fn update(
    pool: &Pool,
    user: UserId,
    device: DeviceId,
    id: TxId,
    edit: &Edit,
) -> Result<Transaction, ApiError> {
    let existing = get(pool, user, id).await?;
    not_a_transfer(&existing)?;
    if edit.amount.currency() != existing.amount.currency() {
        return Err(ApiError::Invalid {
            field: "amount".to_owned(),
            reason: format!(
                "it is in {} and the account holds {}",
                edit.amount.currency().code(),
                existing.amount.currency().code()
            ),
        });
    }
    if let Some(category) = edit.category_id {
        categories::get(pool, user, category).await?;
    }

    let now = OffsetDateTime::now_utc();
    let changed = Transaction {
        direction: edit.direction,
        amount: edit.amount,
        booked_at: edit.booked_at,
        category_id: edit.category_id,
        label: edit.label.clone(),
        counterparty: edit.counterparty.clone(),
        note: edit.note.clone(),
        status: edit.status,
        sync: SyncMeta {
            updated_at: now.max(existing.sync.created_at),
            device_id: device,
            ..existing.sync
        },
        ..existing
    };
    changed.validate().map_err(|reason| ApiError::Invalid {
        field: "transaction".to_owned(),
        reason: reason.to_string(),
    })?;

    let category = changed.category_id.map(|id| *id.as_uuid());
    let client = db::client(pool).await?;
    let row = client
        .query_opt(
            &format!(
                "UPDATE transactions
                    SET direction = $3,
                        amount_minor = $4::bigint::amount_minor,
                        booked_at = $5::timestamptz::ledger_instant,
                        category_id = $6,
                        label = $7,
                        counterparty = $8,
                        note = $9,
                        status = $10,
                        updated_at = $11::timestamptz::ledger_instant,
                        device_id = $12
                  WHERE user_id = $1 AND id = $2 AND deleted_at IS NULL
                  RETURNING {COLUMNS}"
            ),
            &[
                user.as_uuid(),
                id.as_uuid(),
                &changed.direction.as_str(),
                &changed.amount.minor(),
                &changed.booked_at,
                &category,
                &changed.label,
                &changed.counterparty,
                &changed.note,
                &changed.status.as_str(),
                &changed.sync.updated_at,
                device.as_uuid(),
            ],
        )
        .await
        .map_err(|error| {
            // Two occurrences of one series cannot share a day: the schema's index on
            // (recurring_id, booked_at) says so, and moving one onto another's date is the
            // one way a person can run into it.
            if error.code() == Some(&tokio_postgres::error::SqlState::UNIQUE_VIOLATION) {
                ApiError::Conflict {
                    reason: "another row of the same repeating series is already on that day"
                        .to_owned(),
                }
            } else {
                ApiError::database("change a transaction", &error)
            }
        })?;

    // Deleted between the read and the write, by another tab or another device.
    match row {
        Some(row) => read(&row),
        None => Err(ApiError::NotFound { entity: ENTITY }),
    }
}

/// Deletes a row, leaving a tombstone.
///
/// Never a `DELETE`: sync has to tell every other device the row is gone, and a row that no
/// longer exists cannot say so. The balance walk, the breakdown and every listing already
/// skip a row with `deleted_at` set.
///
/// # Errors
/// [`ApiError::NotFound`] if there is no such live row of this user's,
/// [`ApiError::Invalid`] for a transfer leg, or [`ApiError::Internal`].
pub async fn delete(pool: &Pool, user: UserId, device: DeviceId, id: TxId) -> Result<(), ApiError> {
    let existing = get(pool, user, id).await?;
    not_a_transfer(&existing)?;

    let now = OffsetDateTime::now_utc().max(existing.sync.created_at);
    let client = db::client(pool).await?;
    let changed = client
        .execute(
            "UPDATE transactions
                SET deleted_at = $3,
                    updated_at = $3::timestamptz::ledger_instant,
                    device_id = $4
              WHERE user_id = $1 AND id = $2 AND deleted_at IS NULL",
            &[user.as_uuid(), id.as_uuid(), &now, device.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("delete a transaction", &error))?;

    if changed == 0 {
        return Err(ApiError::NotFound { entity: ENTITY });
    }
    Ok(())
}

/// Brings a deleted row back, as it was.
///
/// The undo behind "Deleted. Undo". A tombstone keeps every column, so nothing has to be
/// remembered by the client to restore it.
///
/// # Errors
/// [`ApiError::NotFound`] if there is no deleted row of this user's with that id — a live
/// row included, which has nothing to restore — or [`ApiError::Internal`].
pub async fn restore(
    pool: &Pool,
    user: UserId,
    device: DeviceId,
    id: TxId,
) -> Result<Transaction, ApiError> {
    let now = OffsetDateTime::now_utc();
    let client = db::client(pool).await?;
    let row = client
        .query_opt(
            &format!(
                "UPDATE transactions
                    SET deleted_at = NULL,
                        updated_at = greatest($3::timestamptz, created_at::timestamptz)
                                     ::ledger_instant,
                        device_id = $4
                  WHERE user_id = $1 AND id = $2 AND deleted_at IS NOT NULL
                  RETURNING {COLUMNS}"
            ),
            &[user.as_uuid(), id.as_uuid(), &now, device.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("restore a transaction", &error))?;

    match row {
        Some(row) => read(&row),
        None => Err(ApiError::NotFound { entity: ENTITY }),
    }
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

/// Every live row this user has booked inside a window, across every account.
///
/// For the work that needs the ledger around a set of dates rather than all of it: the
/// duplicate check reads the days around the lines it is checking, and categorising an
/// import reads recent history.
///
/// # Errors
/// [`ApiError::Internal`] if the query fails or a row will not read back.
pub async fn between(
    pool: &Pool,
    user: UserId,
    from: OffsetDateTime,
    to: OffsetDateTime,
) -> Result<Vec<Transaction>, ApiError> {
    let client = db::client(pool).await?;
    let rows = client
        .query(
            &format!(
                "SELECT {COLUMNS} FROM transactions
                  WHERE user_id = $1 AND deleted_at IS NULL
                    AND booked_at >= $2::timestamptz AND booked_at <= $3::timestamptz
                  ORDER BY booked_at, id"
            ),
            &[user.as_uuid(), &from, &to],
        )
        .await
        .map_err(|error| ApiError::database("list transactions in a window", &error))?;

    rows.iter().map(read).collect()
}

/// One live row of this user's, locked until the caller's transaction ends.
///
/// # Errors
/// [`ApiError::NotFound`] if there is no such live row of this user's.
pub(crate) async fn lock(
    client: &impl GenericClient,
    user: UserId,
    id: TxId,
) -> Result<Transaction, ApiError> {
    let found = client
        .query_opt(
            &format!(
                "SELECT {COLUMNS} FROM transactions
                  WHERE user_id = $1 AND id = $2 AND deleted_at IS NULL
                  FOR UPDATE"
            ),
            &[user.as_uuid(), id.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("lock a transaction", &error))?;
    match found {
        Some(row) => read(&row),
        None => Err(ApiError::NotFound { entity: ENTITY }),
    }
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
