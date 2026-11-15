//! Accounts: the places money sits.

use mt_core::enums::AccountKind;
use mt_core::ids::AccountId;
use mt_core::model::{Account, SyncMeta};
use mt_core::money::{Currency, Money};
use uuid::Uuid;

use crate::change::Table;
use crate::error::StoreError;
use crate::open::Store;
use crate::sql::{self, Reader};

/// The table, for error messages.
const ENTITY: &str = "account";

/// What a caller supplies to open an account.
///
/// Note what is absent: no id, no timestamps, no device id, no sort order. Those are the
/// store's, because a caller that mints its own id is a caller that can mint the same one
/// twice, and two apps would have invented the ordering rule twice.
#[derive(Debug, Clone, PartialEq, Eq)]
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

impl Store {
    /// Opens an account and returns the row as written.
    ///
    /// # Errors
    /// [`StoreError::Invalid`] if the opening balance is in another currency or the row
    /// does not satisfy the model, or [`StoreError::Database`] if the insert fails.
    pub fn create_account(&self, new: &NewAccount) -> Result<Account, StoreError> {
        if new.opening_balance.currency() != new.currency {
            return Err(StoreError::Invalid {
                entity: "account",
                reason: format!(
                    "the opening balance is in {} and the account holds {}",
                    new.opening_balance.currency().code(),
                    new.currency.code()
                ),
            });
        }

        let now = sql::to_stored_precision("account", self.now())?;
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
                device_id: self.device_id(),
            },
        };
        account
            .validate()
            .map_err(|reason| StoreError::invalid("account", &reason))?;

        let stamped = sql::millis_from_instant("account", now)?;
        {
            let connection = self.connection()?;
            // Appended to the end of the list. Read and written under the same lock, so
            // two accounts opened at once cannot land on the same position.
            let sort_order: i64 = connection
                .query_row(
                    "SELECT coalesce(max(sort_order) + 1, 0) FROM accounts",
                    [],
                    |row| row.get(0),
                )
                .map_err(|error| StoreError::database("read the account order", &error))?;

            connection
                .execute(
                    "INSERT INTO accounts (
                         id, name, kind, currency, opening_balance_minor,
                         sort_order, archived_at, created_at, updated_at, deleted_at, device_id
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?7, NULL, ?8)",
                    rusqlite::params![
                        account.id.to_string(),
                        account.name,
                        account.kind.as_str(),
                        account.currency.code(),
                        account.opening_balance.minor(),
                        sort_order,
                        stamped,
                        account.sync.device_id.to_string(),
                    ],
                )
                .map_err(|error| StoreError::database("insert an account", &error))?;
        }
        self.published(&[Table::Accounts])?;

        // Read back rather than returned from memory: the row a caller holds is then the
        // row the database holds, sort order and all.
        one(&*self.connection()?, account.id)
    }

    /// Every account that has not been deleted, in display order.
    ///
    /// Archived accounts are included — they still hold a balance, and hiding them is the
    /// screen's decision rather than the store's.
    ///
    /// # Errors
    /// [`StoreError::Database`] if the query fails, or [`StoreError::UnreadableRow`] for a
    /// row that will not read back.
    pub fn accounts(&self) -> Result<Vec<Account>, StoreError> {
        all(&*self.connection()?)
    }

    /// One account.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if there is no such account, or as [`Store::accounts`].
    pub fn account(&self, id: AccountId) -> Result<Account, StoreError> {
        one(&*self.connection()?, id)
    }
}

/// Every live account, in display order, on a connection the caller already holds.
pub(crate) fn all(connection: &rusqlite::Connection) -> Result<Vec<Account>, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT * FROM accounts
                 WHERE deleted_at IS NULL
                 ORDER BY sort_order, id",
        )
        .map_err(|error| StoreError::database("prepare the account list", &error))?;
    let rows = statement
        .query_map([], |row| Ok(read(row)))
        .map_err(|error| StoreError::database("list accounts", &error))?;

    let mut accounts = Vec::new();
    for row in rows {
        accounts.push(row.map_err(|error| StoreError::database("read an account", &error))??);
    }
    Ok(accounts)
}

/// One account, on a connection the caller already holds.
pub(crate) fn one(connection: &rusqlite::Connection, id: AccountId) -> Result<Account, StoreError> {
    let found = connection.query_row(
        "SELECT * FROM accounts WHERE id = ?1 AND deleted_at IS NULL",
        [id.to_string()],
        |row| Ok(read(row)),
    );
    match found {
        Ok(account) => account,
        Err(rusqlite::Error::QueryReturnedNoRows) => Err(StoreError::NotFound {
            entity: ENTITY,
            id: id.to_string(),
        }),
        Err(error) => Err(StoreError::database("read an account", &error)),
    }
}

/// Reads one row.
fn read(row: &rusqlite::Row<'_>) -> Result<Account, StoreError> {
    let reader = Reader::new(ENTITY, row)?;
    let currency = reader.currency("currency")?;
    Ok(Account {
        id: reader.id("id")?,
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
            device_id: reader.id("device_id")?,
        },
    })
}
