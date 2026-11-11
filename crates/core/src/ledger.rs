//! The balance walk: the one place a balance is ever worked out.
//!
//! Balances are derived, never stored. The walk sorts a set of rows by the instant they were
//! booked at and accumulates from the account's opening balance, which is why history and
//! forecast are the same list: a future-dated `pending` row is just a row with a later date,
//! and [`BalanceBasis`] decides whether the walk looks at it.
//!
//! Transfer legs *are* walked. Moving GHS 100 from MoMo to a bank account really does leave
//! one account and arrive in the other, so both balances must move. What a transfer must not
//! do is count as income and as spending, and that is handled in [`flows`], which drops both
//! legs.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::enums::{Direction, TxStatus};
use crate::ids::{AccountId, TxId};
use crate::model::{Account, Transaction};
use crate::money::{Currency, Money, MoneyError};
use time::OffsetDateTime;

/// A set of rows that cannot be walked as it stands.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LedgerError {
    /// A row belonging to another account was handed to a single-account walk. Skipping it
    /// silently would produce a balance that is wrong by exactly that row.
    #[error("transaction {transaction} belongs to account {found}, not {expected}")]
    WrongAccount {
        /// The offending row.
        transaction: TxId,
        /// The account being walked.
        expected: AccountId,
        /// The account the row names.
        found: AccountId,
    },

    /// A row was denominated in a currency the account does not hold.
    #[error("transaction {transaction} is in {found} but account {account} holds {expected}")]
    CurrencyMismatch {
        /// The offending row.
        transaction: TxId,
        /// The account being walked.
        account: AccountId,
        /// The account's currency.
        expected: Currency,
        /// The row's currency.
        found: Currency,
    },

    /// A row named an account that was not supplied.
    #[error("transaction {transaction} names unknown account {account}")]
    UnknownAccount {
        /// The offending row.
        transaction: TxId,
        /// The account it names.
        account: AccountId,
    },

    /// The running total left `i64` minor units. Validation bounds single amounts to
    /// [`crate::model::MAX_AMOUNT_MINOR`], so reaching this needs a corrupt store.
    #[error("the running balance overflowed at transaction {transaction}: {source}")]
    Arithmetic {
        /// The row being added when it overflowed.
        transaction: TxId,
        /// The arithmetic failure underneath.
        #[source]
        source: MoneyError,
    },

    /// A total across rows or accounts left `i64` minor units. No single row is at fault, so
    /// this names the currency being totalled rather than a transaction.
    #[error("the {currency} total overflowed i64 minor units: {source}")]
    TotalOverflow {
        /// The currency being totalled.
        currency: Currency,
        /// The arithmetic failure underneath.
        #[source]
        source: MoneyError,
    },
}

/// Which rows a balance is worked out from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BalanceBasis {
    /// Only rows known to have happened. "What is actually in the account."
    Confirmed,
    /// Confirmed and pending rows alike. "What the account will hold if everything expected
    /// happens" — the forecast, and the same walk that drew the history.
    Projected,
}

impl BalanceBasis {
    /// Whether a row of this status counts towards the basis.
    #[must_use]
    pub const fn admits(self, status: TxStatus) -> bool {
        match self {
            BalanceBasis::Confirmed => matches!(status, TxStatus::Confirmed),
            BalanceBasis::Projected => true,
        }
    }
}

/// One row of the walk: the transaction and the balance standing after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry<'a> {
    /// The row that moved the balance.
    pub transaction: &'a Transaction,
    /// The account balance once this row has been applied.
    pub balance_after: Money,
}

/// An account's balance over time, in booked order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Walk<'a> {
    /// The account walked.
    pub account_id: AccountId,
    /// The currency every figure here is in.
    pub currency: Currency,
    /// The balance before any row was applied.
    pub opening: Money,
    /// Every admitted row, sorted by `booked_at` then by `id`.
    pub entries: Vec<Entry<'a>>,
    /// The balance after the last row, or `opening` if there were none.
    pub closing: Money,
}

impl<'a> Walk<'a> {
    /// The balance standing at `instant`, counting every row booked at or before it.
    ///
    /// Entries are sorted, so this is a binary search rather than a scan: it runs per row on
    /// a chart axis and a linear search there would be quadratic.
    #[must_use]
    pub fn balance_as_of(&self, instant: OffsetDateTime) -> Money {
        let after = self
            .entries
            .partition_point(|entry| entry.transaction.booked_at <= instant);
        match after.checked_sub(1).and_then(|last| self.entries.get(last)) {
            Some(entry) => entry.balance_after,
            None => self.opening,
        }
    }

    /// The first row that takes the balance below zero, if any.
    ///
    /// On a [`BalanceBasis::Projected`] walk this is the outlook's headline: the date the
    /// account runs out, and the row that does it.
    #[must_use]
    pub fn first_shortfall(&self) -> Option<&Entry<'a>> {
        self.entries
            .iter()
            .find(|entry| entry.balance_after.is_negative())
    }

    /// The rows booked within `from..=to`, both ends inclusive.
    #[must_use]
    pub fn between(&self, from: OffsetDateTime, to: OffsetDateTime) -> &[Entry<'a>] {
        let start = self
            .entries
            .partition_point(|entry| entry.transaction.booked_at < from);
        let end = self
            .entries
            .partition_point(|entry| entry.transaction.booked_at <= to);
        self.entries.get(start..end).unwrap_or_default()
    }
}

/// Walks one account's rows and returns the running balance after each.
///
/// Soft-deleted rows are skipped; rows whose status the basis does not admit are skipped.
/// Everything else must belong to `account` and be denominated in its currency — a row that
/// does not is an error rather than a silent omission, because an omission shows up later as
/// a balance that is wrong by an amount nobody can account for.
///
/// Ordering is `booked_at` then `id`. The id tiebreak is what makes the walk deterministic:
/// two rows booked at the same instant would otherwise produce a different intermediate
/// balance on each device, and the sync layer would see phantom disagreement.
///
/// # Errors
/// [`LedgerError`] naming the row at fault.
pub fn walk<'a>(
    account: &Account,
    transactions: &'a [Transaction],
    basis: BalanceBasis,
) -> Result<Walk<'a>, LedgerError> {
    let mut admitted: Vec<&'a Transaction> = Vec::new();
    for transaction in transactions {
        if transaction.sync.is_deleted() || !basis.admits(transaction.status) {
            continue;
        }
        if transaction.account_id != account.id {
            return Err(LedgerError::WrongAccount {
                transaction: transaction.id,
                expected: account.id,
                found: transaction.account_id,
            });
        }
        if transaction.amount.currency() != account.currency {
            return Err(LedgerError::CurrencyMismatch {
                transaction: transaction.id,
                account: account.id,
                expected: account.currency,
                found: transaction.amount.currency(),
            });
        }
        admitted.push(transaction);
    }

    admitted.sort_unstable_by(|left, right| {
        left.booked_at
            .cmp(&right.booked_at)
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut balance = account.opening_balance;
    let mut entries = Vec::with_capacity(admitted.len());
    for transaction in admitted {
        balance = apply(balance, transaction)?;
        entries.push(Entry {
            transaction,
            balance_after: balance,
        });
    }

    Ok(Walk {
        account_id: account.id,
        currency: account.currency,
        opening: account.opening_balance,
        entries,
        closing: balance,
    })
}

/// Adds one row to a running balance, reporting which row overflowed if one does.
fn apply(balance: Money, transaction: &Transaction) -> Result<Money, LedgerError> {
    let arithmetic = |source| LedgerError::Arithmetic {
        transaction: transaction.id,
        source,
    };
    let signed = transaction.signed_amount().map_err(arithmetic)?;
    balance.try_add(signed).map_err(arithmetic)
}

/// Closing balance per account, for a set of accounts and all their rows at once.
///
/// Rows are grouped by account first, so this is one pass over the transactions rather than
/// one pass per account.
///
/// Deleted accounts get no balance, and their rows are skipped rather than treated as
/// orphans — a soft delete leaves the rows behind by design, and erroring on them would make
/// the totals screen fail the moment someone removed an old wallet.
///
/// # Errors
/// [`LedgerError`] naming the row at fault, including [`LedgerError::UnknownAccount`] for a
/// row whose account was not supplied at all.
pub fn balances_by_account(
    accounts: &[Account],
    transactions: &[Transaction],
    basis: BalanceBasis,
) -> Result<BTreeMap<AccountId, Money>, LedgerError> {
    let live: HashMap<AccountId, &Account> = accounts
        .iter()
        .filter(|account| !account.sync.is_deleted())
        .map(|account| (account.id, account))
        .collect();
    let known: HashSet<AccountId> = accounts.iter().map(|account| account.id).collect();

    let mut rows: HashMap<AccountId, Vec<&Transaction>> = HashMap::new();
    for transaction in transactions {
        if transaction.sync.is_deleted() || !basis.admits(transaction.status) {
            continue;
        }
        if !known.contains(&transaction.account_id) {
            return Err(LedgerError::UnknownAccount {
                transaction: transaction.id,
                account: transaction.account_id,
            });
        }
        if !live.contains_key(&transaction.account_id) {
            continue;
        }
        rows.entry(transaction.account_id)
            .or_default()
            .push(transaction);
    }
    // Sorted, so that when two accounts each hold a bad row the error names the same one on
    // every run. The success path is already deterministic; an error that moves around is
    // the one you cannot reproduce from a bug report.
    let mut balances = BTreeMap::new();
    let by_id: BTreeMap<AccountId, &Account> = live.into_iter().collect();
    for (id, account) in by_id {
        let mut balance = account.opening_balance;
        for transaction in rows.get(&id).map(Vec::as_slice).unwrap_or_default() {
            if transaction.amount.currency() != account.currency {
                return Err(LedgerError::CurrencyMismatch {
                    transaction: transaction.id,
                    account: id,
                    expected: account.currency,
                    found: transaction.amount.currency(),
                });
            }
            balance = apply(balance, transaction)?;
        }
        balances.insert(id, balance);
    }
    Ok(balances)
}

/// Total held per currency across a set of accounts.
///
/// Deliberately returns one figure per currency rather than one converted total: a single
/// number spanning currencies is an estimate that moves when the rate does, and labelling it
/// as such is the UI's job, not this function's.
///
/// # Errors
/// As [`balances_by_account`].
pub fn totals_by_currency(
    accounts: &[Account],
    transactions: &[Transaction],
    basis: BalanceBasis,
) -> Result<BTreeMap<Currency, Money>, LedgerError> {
    let per_account = balances_by_account(accounts, transactions, basis)?;
    let currency_of: HashMap<AccountId, Currency> = accounts
        .iter()
        .map(|account| (account.id, account.currency))
        .collect();

    let mut totals: BTreeMap<Currency, Money> = BTreeMap::new();
    for (id, balance) in per_account {
        let currency = match currency_of.get(&id) {
            Some(currency) => *currency,
            None => continue,
        };
        let running = totals
            .entry(currency)
            .or_insert_with(|| Money::zero(currency));
        *running = running
            .try_add(balance)
            .map_err(|source| LedgerError::TotalOverflow { currency, source })?;
    }
    Ok(totals)
}

/// Money in and money out over a set of rows, with transfers dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flows {
    /// The currency all three figures are in.
    pub currency: Currency,
    /// Total that arrived, as a positive amount.
    pub income: Money,
    /// Total that left, as a positive amount.
    pub spending: Money,
    /// `income - spending`. Negative means more left than arrived.
    pub net: Money,
}

/// Totals income and spending over whatever rows are handed in.
///
/// Both legs of a transfer are dropped: moving your own money between your own accounts is
/// neither earning nor spending, and counting it as both is the single most common way a
/// personal-finance report ends up wrong. Soft-deleted rows and rows the basis does not
/// admit are dropped too, as is anything not denominated in `currency` — a cedi total and a
/// dollar total are separate questions.
///
/// The caller supplies the window by filtering the iterator, so there is one definition of
/// "in this period" per feature rather than one hidden in here.
///
/// # Errors
/// [`LedgerError::Arithmetic`] if a total overflows.
pub fn flows<'a, I>(
    currency: Currency,
    transactions: I,
    basis: BalanceBasis,
) -> Result<Flows, LedgerError>
where
    I: IntoIterator<Item = &'a Transaction>,
{
    let mut income = Money::zero(currency);
    let mut spending = Money::zero(currency);

    for transaction in transactions {
        if !counts_as_flow(transaction, currency, basis) {
            continue;
        }
        let arithmetic = |source| LedgerError::Arithmetic {
            transaction: transaction.id,
            source,
        };
        match transaction.direction {
            Direction::In => income = income.try_add(transaction.amount).map_err(arithmetic)?,
            Direction::Out => {
                spending = spending.try_add(transaction.amount).map_err(arithmetic)?;
            }
        }
    }

    let net = income
        .try_sub(spending)
        .map_err(|source| LedgerError::TotalOverflow { currency, source })?;
    Ok(Flows {
        currency,
        income,
        spending,
        net,
    })
}

/// Whether a row is earning or spending, rather than a transfer, a tombstone, or a row this
/// basis does not look at.
#[must_use]
pub fn counts_as_flow(transaction: &Transaction, currency: Currency, basis: BalanceBasis) -> bool {
    !transaction.sync.is_deleted()
        && !transaction.is_transfer()
        && basis.admits(transaction.status)
        && transaction.amount.currency() == currency
}
