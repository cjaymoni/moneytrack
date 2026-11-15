//! Transactions: the row everything else is derived from.

use mt_core::enums::{Direction, TxSource, TxStatus};
use mt_core::ids::{AccountId, CategoryId, TxId};
use mt_core::model::{SyncMeta, Transaction};
use mt_core::money::Money;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::change::Table;
use crate::error::StoreError;
use crate::open::Store;
use crate::repo::accounts;
use crate::sql::{self, Reader};

/// The table, for error messages.
const ENTITY: &str = "transaction";

/// What a caller supplies to record a transaction.
///
/// `booked_at` is the caller's, not the clock's: a row typed in this evening for yesterday's
/// trotro fare is booked yesterday, and a future-dated pending row is the forecast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTransaction {
    /// The account it moves.
    pub account_id: AccountId,
    /// In or out, from that account's point of view.
    pub direction: Direction,
    /// Always positive; `direction` carries the sign. Must be in the account's currency.
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
    /// Where the row came from.
    pub source: TxSource,
}

impl Store {
    /// Writes a transaction and returns the row as written.
    ///
    /// The amount has to be in the account's own currency. The schema says so too, through
    /// the composite foreign key on `(account_id, currency)`, but a caller deserves to be
    /// told which two currencies disagreed rather than that a constraint failed.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if the account does not exist, [`StoreError::Invalid`] for
    /// a currency that does not match or a row the model refuses, or
    /// [`StoreError::Database`] if the insert fails.
    pub fn record_transaction(&self, new: &NewTransaction) -> Result<Transaction, StoreError> {
        // The account is read and the row written under one lock. Two would be a
        // check-then-act: the account could be archived, or its currency changed, between
        // the check passing and the insert landing.
        let connection = self.connection()?;
        let account = accounts::one(&connection, new.account_id)?;
        if new.amount.currency() != account.currency {
            return Err(StoreError::Invalid {
                entity: ENTITY,
                reason: format!(
                    "the amount is in {} and the account holds {}",
                    new.amount.currency().code(),
                    account.currency.code()
                ),
            });
        }

        let now = sql::to_stored_precision(ENTITY, self.now())?;
        let transaction = Transaction {
            id: TxId::from(Uuid::now_v7()),
            account_id: new.account_id,
            direction: new.direction,
            amount: new.amount,
            booked_at: sql::to_stored_precision(ENTITY, new.booked_at)?,
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
                device_id: self.device_id(),
            },
        };
        transaction
            .validate()
            .map_err(|reason| StoreError::invalid(ENTITY, &reason))?;

        let booked_at = sql::millis_from_instant(ENTITY, transaction.booked_at)?;
        let stamped = sql::millis_from_instant(ENTITY, now)?;
        connection
            .execute(
                "INSERT INTO transactions (
                         id, account_id, direction, amount_minor, currency, booked_at,
                         category_id, label, counterparty, note, status, source,
                         source_ref, external_id, recurring_id, transfer_id,
                         created_at, updated_at, deleted_at, device_id
                     ) VALUES (
                         ?1, ?2, ?3, ?4, ?5, ?6,
                         ?7, ?8, ?9, ?10, ?11, ?12,
                         NULL, NULL, NULL, NULL,
                         ?13, ?13, NULL, ?14
                     )",
                rusqlite::params![
                    transaction.id.to_string(),
                    transaction.account_id.to_string(),
                    transaction.direction.as_str(),
                    transaction.amount.minor(),
                    transaction.amount.currency().code(),
                    booked_at,
                    transaction.category_id.map(|id| id.to_string()),
                    transaction.label,
                    transaction.counterparty,
                    transaction.note,
                    transaction.status.as_str(),
                    transaction.source.as_str(),
                    stamped,
                    transaction.sync.device_id.to_string(),
                ],
            )
            .map_err(|error| StoreError::database("insert a transaction", &error))?;

        // Published after the lock is released, so an observer may query the store from
        // inside the notification without deadlocking against itself.
        drop(connection);
        self.published(&[Table::Transactions])?;

        Ok(transaction)
    }

    /// Every live row in one account, in the order the balance walk wants them.
    ///
    /// Pending rows included: the walk decides what a basis counts, and a query that
    /// dropped them here would make the outlook impossible to ask for.
    ///
    /// # Errors
    /// [`StoreError::Database`] if the query fails, or [`StoreError::UnreadableRow`] for a
    /// row that will not read back.
    pub fn transactions_for_account(
        &self,
        account_id: AccountId,
    ) -> Result<Vec<Transaction>, StoreError> {
        for_account(&*self.connection()?, account_id)
    }

    /// Every live row, across every account.
    ///
    /// # Errors
    /// As [`Store::transactions_for_account`].
    pub fn transactions(&self) -> Result<Vec<Transaction>, StoreError> {
        all(&*self.connection()?)
    }
}

/// Every live row in one account, on a connection the caller already holds.
pub(crate) fn for_account(
    connection: &rusqlite::Connection,
    account_id: AccountId,
) -> Result<Vec<Transaction>, StoreError> {
    query(
        connection,
        "SELECT * FROM transactions
         WHERE account_id = ?1 AND deleted_at IS NULL
         ORDER BY booked_at, id",
        rusqlite::params![account_id.to_string()],
    )
}

/// Every live row, on a connection the caller already holds.
pub(crate) fn all(connection: &rusqlite::Connection) -> Result<Vec<Transaction>, StoreError> {
    query(
        connection,
        "SELECT * FROM transactions
         WHERE deleted_at IS NULL
         ORDER BY booked_at, id",
        rusqlite::params![],
    )
}

/// One query, so the row mapping and the error wording are written once.
fn query(
    connection: &rusqlite::Connection,
    sql: &str,
    params: &[&dyn rusqlite::ToSql],
) -> Result<Vec<Transaction>, StoreError> {
    {
        let mut statement = connection
            .prepare(sql)
            .map_err(|error| StoreError::database("prepare a transaction query", &error))?;
        let rows = statement
            .query_map(params, |row| Ok(read(row)))
            .map_err(|error| StoreError::database("list transactions", &error))?;

        let mut transactions = Vec::new();
        for row in rows {
            transactions
                .push(row.map_err(|error| StoreError::database("read a transaction", &error))??);
        }
        Ok(transactions)
    }
}

/// Reads one row.
fn read(row: &rusqlite::Row<'_>) -> Result<Transaction, StoreError> {
    let reader = Reader::new(ENTITY, row)?;
    let currency = reader.currency("currency")?;
    Ok(Transaction {
        id: reader.id("id")?,
        account_id: reader.id("account_id")?,
        direction: reader.variant("direction")?,
        amount: reader.money("amount_minor", currency)?,
        booked_at: reader.instant("booked_at")?,
        category_id: reader.optional_id("category_id")?,
        label: reader.text("label")?,
        counterparty: reader.optional_text("counterparty")?,
        note: reader.optional_text("note")?,
        status: reader.variant("status")?,
        source: reader.variant("source")?,
        source_ref: reader.optional_text("source_ref")?,
        external_id: reader.optional_text("external_id")?,
        recurring_id: reader.optional_id("recurring_id")?,
        transfer_id: reader.optional_id("transfer_id")?,
        sync: SyncMeta {
            created_at: reader.instant("created_at")?,
            updated_at: reader.instant("updated_at")?,
            deleted_at: reader.optional_instant("deleted_at")?,
            device_id: reader.id("device_id")?,
        },
    })
}
