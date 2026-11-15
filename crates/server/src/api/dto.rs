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

use core::str::FromStr;
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
    /// What it holds before any transaction, in minor units.
    pub opening_balance_minor: i64,
}

/// What a browser sends to record a transaction.
#[derive(Debug, Deserialize)]
pub struct NewTransactionIn {
    /// The account it moves.
    pub account_id: String,
    /// `in` or `out`.
    pub direction: String,
    /// Always positive; `direction` carries the sign.
    pub amount_minor: i64,
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
