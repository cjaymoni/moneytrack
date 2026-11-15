//! The one handle that crosses, and the calls that go through it.
//!
//! Everywhere else in this crate, values cross and nothing else. Here an app opens a
//! [`Store`], holds it for as long as the process lives, and passes owned values across
//! every call it makes on it. What it never sees is a connection, a statement, a cursor or
//! a row: those stay in `mt-store`, which is the point of putting the database there.
//!
//! ## Why every call that touches the database is async
//!
//! SQLite is fast, a handset's ledger is small, and most of these calls would return before
//! a frame was due. Most is not all — a first sync, an import of a year of statements, a
//! query behind a cold screen — and a UI that blocks its main thread for the slow case is a
//! UI that janks for everyone to spare the author an `await`. So the work runs on a
//! blocking pool and the result comes back as `await` in Swift and `suspend` in Kotlin. The
//! few calls that only read a number held in memory stay synchronous, because making them
//! async would be noise.

use std::path::Path;
use std::sync::Arc;

use mt_core::ids::AccountId;
use mt_core::model::{Account, Transaction};
use mt_store::{NewAccount, NewTransaction, Store as DeviceStore, StoreError as DeviceError};

use crate::api::AccountBalanceDto;
use crate::dto::{AccountDto, TransactionDto};
use crate::enums::{AccountKind, BalanceBasis, Direction, TxSource, TxStatus};
use crate::error::BridgeError;
use crate::scalar::{self, MoneyDto};

/// What the store refuses, or could not do.
///
/// A mirror of `mt_store::StoreError`, for the same reason [`crate::enums`] mirrors the
/// core's enums: an app gets a Swift `enum` and a Kotlin sealed class it can switch on, and
/// a variant added below is a compile error on both sides rather than a silent default.
///
/// [`StoreError::Value`] is the one variant that flattens rather than mirrors. A value that
/// will not convert carries a [`BridgeError`], which names the field — and that name is
/// what a developer needs. An app has nothing to do with the distinction between a
/// malformed id and a malformed date except show it and log it, so it crosses as the
/// message rather than as a second error enum nested inside this one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum StoreError {
    /// A value handed to the store will not convert. The message names the field.
    #[error("{reason}")]
    Value {
        /// What was wrong, from [`BridgeError`].
        reason: String,
    },

    /// The database file would not open, or a statement would not run.
    #[error("{operation} failed: {reason}")]
    Database {
        /// What was being attempted.
        operation: String,
        /// What SQLite said.
        reason: String,
    },

    /// The key is not the length the cipher wants.
    #[error("the database key must be {expected} bytes, not {supplied}")]
    KeyLength {
        /// Bytes wanted.
        expected: u32,
        /// Bytes supplied.
        supplied: u32,
    },

    /// The file exists and will not decrypt under this key.
    #[error("the database could not be read: wrong key, or not a database")]
    Unreadable,

    /// The file was written by a newer build than this one.
    #[error("the database is at schema {found}, and this build knows {known}")]
    SchemaTooNew {
        /// What the file says.
        found: i64,
        /// What this build can run.
        known: i64,
    },

    /// A row that is in the database and will not read back into a value.
    #[error("{entity} {id}: column {column} does not read back: {reason}")]
    UnreadableRow {
        /// Which table.
        entity: String,
        /// Which row.
        id: String,
        /// Which column.
        column: String,
        /// What is wrong with it.
        reason: String,
    },

    /// A row that does not satisfy the model's own rules.
    #[error("{entity} is not valid: {reason}")]
    Invalid {
        /// Which table.
        entity: String,
        /// What the core said.
        reason: String,
    },

    /// A row the call needed is not there.
    #[error("no {entity} with id {id}")]
    NotFound {
        /// Which table.
        entity: String,
        /// The id looked for.
        id: String,
    },

    /// A rule refused what the rows add up to.
    #[error("{rule} failed: {reason}")]
    Rule {
        /// Which rule.
        rule: String,
        /// What the core said.
        reason: String,
    },

    /// The database lock was left poisoned by a thread that failed holding it.
    #[error("the database lock is poisoned: a thread failed while holding it")]
    LockPoisoned,

    /// The thread the work was handed to did not come back.
    ///
    /// Reachable only if a database call panics, which is a bug in this crate rather than
    /// anything an app did. Reported rather than retried: the connection's state is unknown
    /// at that point, and quietly running the call again could write the row twice.
    #[error("{operation} did not finish: the database thread failed")]
    Interrupted {
        /// What was being attempted.
        operation: String,
    },
}

impl From<BridgeError> for StoreError {
    fn from(error: BridgeError) -> Self {
        Self::Value {
            reason: error.to_string(),
        }
    }
}

impl From<DeviceError> for StoreError {
    fn from(error: DeviceError) -> Self {
        match error {
            DeviceError::Database { operation, reason } => Self::Database { operation, reason },
            // Widened to u32 at the boundary: a `usize` is 32 bits on one Android ABI and
            // 64 on the others, and a type whose width depends on the handset is not a
            // thing to publish.
            DeviceError::KeyLength { expected, supplied } => Self::KeyLength {
                expected: u32::try_from(expected).unwrap_or(u32::MAX),
                supplied: u32::try_from(supplied).unwrap_or(u32::MAX),
            },
            DeviceError::Unreadable => Self::Unreadable,
            DeviceError::SchemaTooNew { found, known } => Self::SchemaTooNew { found, known },
            DeviceError::UnreadableRow {
                entity,
                id,
                column,
                reason,
            } => Self::UnreadableRow {
                entity: entity.to_owned(),
                id,
                column: column.to_owned(),
                reason,
            },
            DeviceError::Invalid { entity, reason } => Self::Invalid {
                entity: entity.to_owned(),
                reason,
            },
            DeviceError::NotFound { entity, id } => Self::NotFound {
                entity: entity.to_owned(),
                id,
            },
            DeviceError::Rule { rule, reason } => Self::Rule {
                rule: rule.to_owned(),
                reason,
            },
            DeviceError::LockPoisoned => Self::LockPoisoned,
        }
    }
}

/// A table a write touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ChangedTable {
    /// `accounts`.
    Accounts,
    /// `transactions`.
    Transactions,
    /// `categories`.
    Categories,
    /// `category_rules`.
    CategoryRules,
    /// `recurring`.
    Recurring,
    /// `budgets`.
    Budgets,
    /// `inbox_items`.
    InboxItems,
}

impl From<mt_store::Table> for ChangedTable {
    fn from(table: mt_store::Table) -> Self {
        match table {
            mt_store::Table::Accounts => Self::Accounts,
            mt_store::Table::Transactions => Self::Transactions,
            mt_store::Table::Categories => Self::Categories,
            mt_store::Table::CategoryRules => Self::CategoryRules,
            mt_store::Table::Recurring => Self::Recurring,
            mt_store::Table::Budgets => Self::Budgets,
            mt_store::Table::InboxItems => Self::InboxItems,
        }
    }
}

/// One committed write.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ChangeDto {
    /// Strictly increasing. A gap means a change arrived while nobody was listening.
    pub revision: u64,
    /// The tables it touched.
    pub tables: Vec<ChangedTable>,
}

/// Someone listening for writes.
///
/// This is what replaces drift's live queries: an app implements it, wraps it in an
/// `AsyncStream` or a `Flow`, and re-runs the queries behind the screens the change
/// touched. The call arrives on the thread that committed the write, so an implementation
/// does the smallest possible thing and hands off.
#[uniffi::export(with_foreign)]
pub trait ChangeObserver: Send + Sync {
    /// A write has committed, and a query run now will see it.
    fn changed(&self, change: ChangeDto);
}

/// Passes a store change out to a foreign observer.
struct Relay(Arc<dyn ChangeObserver>);

impl mt_store::ChangeObserver for Relay {
    fn changed(&self, change: &mt_store::Change) {
        self.0.changed(ChangeDto {
            revision: change.revision,
            tables: change.tables.iter().copied().map(Into::into).collect(),
        });
    }
}

/// What a caller supplies to open an account.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NewAccountDto {
    /// What the user calls it.
    pub name: String,
    /// Cash, mobile money, bank or card.
    pub kind: AccountKind,
    /// ISO 4217 code.
    pub currency: String,
    /// What it held before any transaction.
    pub opening_balance: MoneyDto,
}

impl NewAccountDto {
    /// Converts and validates.
    fn to_core(&self) -> Result<NewAccount, BridgeError> {
        Ok(NewAccount {
            name: self.name.clone(),
            kind: self.kind.into(),
            currency: scalar::currency("account.currency", &self.currency)?,
            opening_balance: self.opening_balance.parse("account.opening_balance")?,
        })
    }
}

/// What a caller supplies to record a transaction.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NewTransactionDto {
    /// The account it moves.
    pub account_id: String,
    /// In or out, from that account's point of view.
    pub direction: Direction,
    /// Always positive; `direction` carries the sign.
    pub amount: MoneyDto,
    /// When it happened, or is expected to. RFC 3339.
    pub booked_at: String,
    /// The category, absent for uncategorised.
    pub category_id: Option<String>,
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

impl NewTransactionDto {
    /// Converts and validates.
    fn to_core(&self) -> Result<NewTransaction, BridgeError> {
        Ok(NewTransaction {
            account_id: scalar::id("transaction.account_id", &self.account_id)?,
            direction: self.direction.into(),
            amount: self.amount.parse("transaction.amount")?,
            booked_at: scalar::instant("transaction.booked_at", &self.booked_at)?,
            category_id: scalar::optional_id("transaction.category_id", self.category_id.as_ref())?,
            label: self.label.clone(),
            counterparty: self.counterparty.clone(),
            note: self.note.clone(),
            status: self.status.into(),
            source: self.source.into(),
        })
    }
}

/// One person's ledger, open.
#[derive(Debug, uniffi::Object)]
pub struct Store {
    inner: Arc<DeviceStore>,
}

/// Opens or creates the database at `path`, encrypted under `key`.
///
/// The key is a raw 256-bit key, not a passphrase: the app generates it once, keeps it in
/// the Keychain or the Android Keystore behind device authentication, and hands over the
/// bytes. Rust never touches the secure enclave and never stores the key.
///
/// # Errors
/// [`StoreError::KeyLength`] for a key of the wrong size, [`StoreError::Unreadable`] if the
/// file will not decrypt under this key, [`StoreError::SchemaTooNew`] for a file from a
/// later build, or [`StoreError::Database`] if the file will not open.
#[uniffi::export(async_runtime = "tokio")]
pub async fn open_store(path: String, key: Vec<u8>) -> Result<Arc<Store>, StoreError> {
    let inner = blocking("open the store", move || {
        DeviceStore::open(Path::new(&path), &key).map(Arc::new)
    })
    .await?;
    Ok(Arc::new(Store { inner }))
}

#[uniffi::export(async_runtime = "tokio")]
impl Store {
    /// This installation's id, as carried on every row it writes.
    ///
    /// Synchronous: it was read once when the store opened and has not changed since.
    #[must_use]
    pub fn device_id(&self) -> String {
        self.inner.device_id().to_string()
    }

    /// The revision of the last committed write, or 0 before any.
    ///
    /// Synchronous: it is a counter in memory. A screen that has just been told about a
    /// change can compare it against the one it last rendered.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.inner.revision()
    }

    /// Registers an observer and returns the handle that removes it.
    ///
    /// # Errors
    /// [`StoreError::LockPoisoned`].
    pub fn subscribe(&self, observer: Arc<dyn ChangeObserver>) -> Result<u64, StoreError> {
        self.inner
            .subscribe(Arc::new(Relay(observer)))
            .map(|id| id.as_u64())
            .map_err(Into::into)
    }

    /// Removes an observer. Removing one that is already gone is not an error.
    ///
    /// # Errors
    /// [`StoreError::LockPoisoned`].
    pub fn unsubscribe(&self, subscription: u64) -> Result<(), StoreError> {
        self.inner
            .unsubscribe(mt_store::SubscriptionId::from_u64(subscription))
            .map_err(Into::into)
    }

    /// Opens an account and returns the row as written.
    ///
    /// # Errors
    /// [`StoreError::Value`] for a value that will not convert, [`StoreError::Invalid`] for
    /// a row the model refuses, or [`StoreError::Database`] if the insert fails.
    pub async fn create_account(&self, account: NewAccountDto) -> Result<AccountDto, StoreError> {
        let new = account.to_core()?;
        let store = Arc::clone(&self.inner);
        let written = blocking("create an account", move || store.create_account(&new)).await?;
        account_out(&written)
    }

    /// Every account that has not been deleted, in display order.
    ///
    /// # Errors
    /// [`StoreError::Database`] if the query fails, or [`StoreError::UnreadableRow`] for a
    /// row that will not read back.
    pub async fn accounts(&self) -> Result<Vec<AccountDto>, StoreError> {
        let store = Arc::clone(&self.inner);
        let rows = blocking("list accounts", move || store.accounts()).await?;
        rows.iter().map(account_out).collect()
    }

    /// Writes a transaction and returns the row as written.
    ///
    /// # Errors
    /// [`StoreError::Value`] for a value that will not convert, [`StoreError::NotFound`] if
    /// the account does not exist, [`StoreError::Invalid`] for a currency that does not
    /// match the account or a row the model refuses, or [`StoreError::Database`].
    pub async fn record_transaction(
        &self,
        transaction: NewTransactionDto,
    ) -> Result<TransactionDto, StoreError> {
        let new = transaction.to_core()?;
        let store = Arc::clone(&self.inner);
        let written = blocking("record a transaction", move || {
            store.record_transaction(&new)
        })
        .await?;
        transaction_out(&written)
    }

    /// Every live row in one account, oldest first.
    ///
    /// Pending rows included: the basis a balance is asked on decides what counts, and a
    /// query that dropped them here would make the outlook impossible to ask for.
    ///
    /// # Errors
    /// As [`Store::accounts`], plus [`StoreError::Value`] for an unreadable id.
    pub async fn transactions_for_account(
        &self,
        account_id: String,
    ) -> Result<Vec<TransactionDto>, StoreError> {
        let id: AccountId = scalar::id("account_id", &account_id)?;
        let store = Arc::clone(&self.inner);
        let rows = blocking("list transactions", move || {
            store.transactions_for_account(id)
        })
        .await?;
        rows.iter().map(transaction_out).collect()
    }

    /// What one account holds.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if there is no such account, [`StoreError::Rule`] if the
    /// rows will not walk, or [`StoreError::Database`].
    pub async fn account_balance(
        &self,
        account_id: String,
        basis: BalanceBasis,
    ) -> Result<MoneyDto, StoreError> {
        let id: AccountId = scalar::id("account_id", &account_id)?;
        let store = Arc::clone(&self.inner);
        let balance = blocking("walk a balance", move || {
            store.account_balance(id, basis.into())
        })
        .await?;
        Ok(balance.into())
    }

    /// What every account holds, in one pass over the rows.
    ///
    /// # Errors
    /// As [`Store::account_balance`].
    pub async fn account_balances(
        &self,
        basis: BalanceBasis,
    ) -> Result<Vec<AccountBalanceDto>, StoreError> {
        let store = Arc::clone(&self.inner);
        let balances = blocking("walk every balance", move || {
            store.account_balances(basis.into())
        })
        .await?;
        Ok(balances
            .into_iter()
            .map(|(id, balance)| AccountBalanceDto {
                account_id: id.to_string(),
                balance: balance.into(),
            })
            .collect())
    }
}

/// Runs a database call on the blocking pool.
///
/// `async_runtime = "tokio"` above is what puts a Tokio context around the future the
/// foreign side is polling, which is what makes this legal from a call that started in
/// Swift or Kotlin.
async fn blocking<T, F>(operation: &'static str, work: F) -> Result<T, StoreError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, DeviceError> + Send + 'static,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(result) => result.map_err(Into::into),
        Err(_) => Err(StoreError::Interrupted {
            operation: operation.to_owned(),
        }),
    }
}

/// Renders an account row outward.
fn account_out(account: &Account) -> Result<AccountDto, StoreError> {
    AccountDto::from_core(account).map_err(Into::into)
}

/// Renders a transaction row outward.
fn transaction_out(transaction: &Transaction) -> Result<TransactionDto, StoreError> {
    TransactionDto::from_core(transaction).map_err(Into::into)
}
