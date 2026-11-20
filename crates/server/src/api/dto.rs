//! What crosses the wire, and how each value is spelled.
//!
//! The spellings are the ones the sync protocol and the FFI already commit to, which
//! `crates/core/tests/wire.rs` fixes: ids and instants as text, enums in their own
//! lowercase words, money as minor units plus an ISO code.
//!
//! Money carries a third field, and it is the one that keeps a rule out of the client. A
//! `display` string is rendered here, by `mt-core`, because where the decimal point goes is
//! a property of the currency — two places for the cedi, none for the CFA franc. A browser
//! that divides by a hundred is right until the first West African row arrives.
//!
//! The same holds in the other direction. A person types `12.50`, and turning that into
//! minor units is the same rule read backwards, so an amount may arrive as the decimal text
//! the person typed and be read here by [`Money::parse`]. A browser that multiplied by a
//! hundred would be wrong for the same franc.

use core::str::FromStr;
use mt_core::ledger::Walk;
use mt_core::model::{Account, Transaction};
use mt_core::money::{Currency, Money};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::ApiError;
use crate::repo::users::User;

/// An amount, as the browser receives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MoneyOut {
    /// Signed amount in minor units. Never a decimal, never a double.
    pub minor_units: i64,
    /// ISO 4217 code.
    pub currency: String,
    /// The amount at the currency's own number of decimal places, rendered by `mt-core`.
    pub display: String,
}

impl From<Money> for MoneyOut {
    fn from(money: Money) -> Self {
        Self {
            minor_units: money.minor(),
            currency: money.currency().code().to_owned(),
            display: money.to_string(),
        }
    }
}

/// A person.
#[derive(Debug, Clone, Serialize)]
pub struct UserOut {
    /// Row id.
    pub id: String,
    /// The address they sign in with.
    pub email: String,
    /// When they registered, RFC 3339.
    pub created_at: String,
}

impl TryFrom<User> for UserOut {
    type Error = ApiError;

    fn try_from(user: User) -> Result<Self, Self::Error> {
        Ok(Self {
            id: user.id.to_string(),
            email: user.email,
            created_at: instant("created_at", user.created_at)?,
        })
    }
}

/// An account and what it holds.
#[derive(Debug, Clone, Serialize)]
pub struct AccountOut {
    /// Row id.
    pub id: String,
    /// What the user calls it.
    pub name: String,
    /// `cash`, `momo`, `bank` or `card`.
    pub kind: String,
    /// ISO 4217 code. Every row in this account is denominated in it.
    pub currency: String,
    /// The balance before any transaction.
    pub opening_balance: MoneyOut,
    /// What it holds counting only rows known to have happened.
    pub confirmed: MoneyOut,
    /// What it holds counting pending rows too — the forecast.
    pub projected: MoneyOut,
}

/// A transaction.
#[derive(Debug, Clone, Serialize)]
pub struct TransactionOut {
    /// Row id.
    pub id: String,
    /// The account it moves.
    pub account_id: String,
    /// `in` or `out`, from that account's point of view.
    pub direction: String,
    /// The amount, always positive; `direction` carries the sign.
    pub amount: MoneyOut,
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
    /// `pending` or `confirmed`.
    pub status: String,
    /// Where the row came from.
    pub source: String,
}

impl TryFrom<&Transaction> for TransactionOut {
    type Error = ApiError;

    fn try_from(transaction: &Transaction) -> Result<Self, Self::Error> {
        Ok(Self {
            id: transaction.id.to_string(),
            account_id: transaction.account_id.to_string(),
            direction: transaction.direction.as_str().to_owned(),
            amount: transaction.amount.into(),
            booked_at: instant("booked_at", transaction.booked_at)?,
            category_id: transaction.category_id.map(|id| id.to_string()),
            label: transaction.label.clone(),
            counterparty: transaction.counterparty.clone(),
            note: transaction.note.clone(),
            status: transaction.status.as_str().to_owned(),
            source: transaction.source.as_str().to_owned(),
        })
    }
}

/// A currency the server knows how to scale, for a picker.
#[derive(Debug, Clone, Serialize)]
pub struct CurrencyOut {
    /// ISO 4217 code.
    pub code: String,
    /// The English name, so a person can find the code.
    pub name: String,
}

impl From<Currency> for CurrencyOut {
    fn from(currency: Currency) -> Self {
        Self {
            code: currency.code().to_owned(),
            name: currency.name().to_owned(),
        }
    }
}

/// One row of an account's ledger and what the account held once it was applied.
#[derive(Debug, Clone, Serialize)]
pub struct LedgerEntryOut {
    /// The row.
    pub transaction: TransactionOut,
    /// The running balance after it, walked by `mt-core`.
    pub balance_after: MoneyOut,
}

/// An account's ledger: its rows in booked order with the running balance beside each.
#[derive(Debug, Clone, Serialize)]
pub struct LedgerOut {
    /// The account, with both balances.
    pub account: AccountOut,
    /// `confirmed` or `projected`: which rows the walk admitted.
    pub basis: String,
    /// What the account held before any row.
    pub opening: MoneyOut,
    /// Every admitted row, oldest first.
    pub entries: Vec<LedgerEntryOut>,
    /// What it holds after the last row.
    pub closing: MoneyOut,
}

/// Renders a walk.
///
/// # Errors
/// [`ApiError::Internal`] if an instant will not render.
pub fn ledger_out(
    account: AccountOut,
    basis: &str,
    walk: &Walk<'_>,
) -> Result<LedgerOut, ApiError> {
    let entries = walk
        .entries
        .iter()
        .map(|entry| {
            Ok(LedgerEntryOut {
                transaction: TransactionOut::try_from(entry.transaction)?,
                balance_after: entry.balance_after.into(),
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    Ok(LedgerOut {
        account,
        basis: basis.to_owned(),
        opening: walk.opening.into(),
        entries,
        closing: walk.closing.into(),
    })
}

/// Renders an account with the two balances the screens want.
pub fn account_out(account: &Account, confirmed: Money, projected: Money) -> AccountOut {
    AccountOut {
        id: account.id.to_string(),
        name: account.name.clone(),
        kind: account.kind.as_str().to_owned(),
        currency: account.currency.code().to_owned(),
        opening_balance: account.opening_balance.into(),
        confirmed: confirmed.into(),
        projected: projected.into(),
    }
}

/// What a browser sends to register or sign in.
#[derive(Debug, Deserialize)]
pub struct Credentials {
    /// The address.
    pub email: String,
    /// The password, which is never logged, never echoed and never stored.
    pub password: String,
}

/// What a browser sends to open an account.
#[derive(Debug, Deserialize)]
pub struct NewAccountIn {
    /// What to call it.
    pub name: String,
    /// `cash`, `momo`, `bank` or `card`.
    pub kind: String,
    /// ISO 4217 code.
    pub currency: String,
    /// What it holds before any transaction, in minor units. This or `opening_balance`.
    pub opening_balance_minor: Option<i64>,
    /// The same, as the decimal text a person typed, read by `mt-core` in the account's
    /// currency. This or `opening_balance_minor`; neither means zero.
    pub opening_balance: Option<String>,
}

/// What a browser sends to record a transaction.
#[derive(Debug, Deserialize)]
pub struct NewTransactionIn {
    /// The account it moves.
    pub account_id: String,
    /// `in` or `out`.
    pub direction: String,
    /// Always positive; `direction` carries the sign. This or `amount`.
    pub amount_minor: Option<i64>,
    /// The same, as the decimal text a person typed, read by `mt-core` in the account's
    /// currency. This or `amount_minor`.
    pub amount: Option<String>,
    /// When it happened, RFC 3339. Absent means now.
    pub booked_at: Option<String>,
    /// What it was.
    pub label: String,
    /// Merchant, sender or recipient.
    pub counterparty: Option<String>,
    /// Free text.
    pub note: Option<String>,
    /// `pending` or `confirmed`. Absent means confirmed.
    pub status: Option<String>,
}

/// Reads one of the closed sets, naming the field when it is not one of them.
///
/// # Errors
/// [`ApiError::Invalid`] naming the field and what was wanted.
pub fn variant<T>(field: &str, value: &str) -> Result<T, ApiError>
where
    T: FromStr,
    T::Err: core::fmt::Display,
{
    T::from_str(value).map_err(|error| ApiError::Invalid {
        field: field.to_owned(),
        reason: error.to_string(),
    })
}

/// Reads an amount offered either as minor units or as decimal text, never both.
///
/// The text goes through [`Money::parse`], which refuses more decimal places than the
/// currency has rather than rounding — a person who typed `5.005` cedis meant something,
/// and it is not the server's place to guess what.
///
/// # Errors
/// [`ApiError::Invalid`] naming `field` if both or neither are given, or the text will not
/// parse. The reason never quotes the text: it is someone's money.
pub fn amount(
    field: &str,
    minor: Option<i64>,
    text: Option<&str>,
    currency: Currency,
) -> Result<Option<Money>, ApiError> {
    let refuse = |reason: String| ApiError::Invalid {
        field: field.to_owned(),
        reason,
    };
    match (minor, text) {
        (Some(_), Some(_)) => Err(refuse(
            "give it in minor units or as decimal text, not both".to_owned(),
        )),
        (Some(minor), None) => Ok(Some(Money::new(minor, currency))),
        (None, Some(text)) => match Money::parse(text.trim(), currency) {
            Ok(money) => Ok(Some(money)),
            Err(mt_core::money::MoneyError::Parse { reason, .. }) => {
                Err(refuse(format!("{reason} ({currency})")))
            }
            Err(other) => Err(refuse(other.to_string())),
        },
        (None, None) => Ok(None),
    }
}

/// Reads an ISO 4217 code.
///
/// # Errors
/// [`ApiError::Invalid`] if the registry does not know it.
pub fn currency(field: &str, value: &str) -> Result<Currency, ApiError> {
    Currency::parse(value).map_err(|error| ApiError::Invalid {
        field: field.to_owned(),
        reason: error.to_string(),
    })
}

/// Reads a typed id from its canonical hyphenated form.
///
/// # Errors
/// [`ApiError::Invalid`] if the text is not a UUID.
pub fn id<T: From<uuid::Uuid>>(field: &str, value: &str) -> Result<T, ApiError> {
    uuid::Uuid::parse_str(value)
        .map(T::from)
        .map_err(|_| ApiError::Invalid {
            field: field.to_owned(),
            reason: "it is not a UUID".to_owned(),
        })
}

/// Reads an RFC 3339 instant.
///
/// # Errors
/// [`ApiError::Invalid`] if it does not parse.
pub fn parse_instant(field: &str, value: &str) -> Result<OffsetDateTime, ApiError> {
    OffsetDateTime::parse(value, &Rfc3339).map_err(|_| ApiError::Invalid {
        field: field.to_owned(),
        reason: "it is not an RFC 3339 timestamp".to_owned(),
    })
}

/// Renders an instant back to RFC 3339.
///
/// # Errors
/// [`ApiError::Internal`] for an instant outside the years RFC 3339 can spell, which a
/// validated row cannot reach.
pub fn instant(field: &'static str, value: OffsetDateTime) -> Result<String, ApiError> {
    value.format(&Rfc3339).map_err(|error| ApiError::Internal {
        operation: "render an instant",
        reason: format!("{field}: {error}"),
    })
}
