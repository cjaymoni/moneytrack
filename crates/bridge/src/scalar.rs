//! The four scalars that cross the bridge, and how they are spelled.
//!
//! Ids, money, instants and dates all have a compact native form in `mt-core` and a textual
//! form on the sync wire. The bridge uses the wire form, deliberately: `tests/wire.rs` in
//! the core already fixes those spellings as a protocol commitment, the SQLite schema stores
//! them that way, and a Dart layer that reads a row out of drift can hand it straight back
//! without a second encoding to keep in agreement.
//!
//! Money is the exception that proves it. It crosses as minor units and a currency code,
//! never as a decimal string and never as a double, because the one thing this project has
//! promised from the first commit is that no amount is ever a float.

use mt_core::money::{Currency, Money};
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::error::BridgeError;

/// The wire spelling of a date: `2026-11-11`.
const DATE_FORMAT: &[time::format_description::FormatItem<'static>] =
    format_description!("[year]-[month]-[day]");

/// An amount of money as it crosses the bridge.
///
/// Integer minor units plus the ISO 4217 code they are denominated in — the same pair
/// `mt_core::money::Money` holds, with the currency widened to a string because Dart has no
/// three-byte array type worth the trouble.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoneyDto {
    /// Signed amount in minor units. Never a decimal, never a double.
    pub minor_units: i64,
    /// ISO 4217 code, three uppercase letters.
    pub currency: String,
}

impl MoneyDto {
    /// Reads the pair into a core [`Money`].
    ///
    /// # Errors
    /// [`BridgeError::InvalidCurrency`] if the code is not three uppercase letters.
    pub fn parse(&self, field: &str) -> Result<Money, BridgeError> {
        Ok(Money::new(
            self.minor_units,
            currency(field, &self.currency)?,
        ))
    }
}

impl From<Money> for MoneyDto {
    fn from(money: Money) -> Self {
        Self {
            minor_units: money.minor(),
            currency: money.currency().code().to_owned(),
        }
    }
}

/// Reads an ISO 4217 code.
///
/// # Errors
/// [`BridgeError::InvalidCurrency`] if it is not three uppercase letters.
pub fn currency(field: &str, value: &str) -> Result<Currency, BridgeError> {
    Currency::parse(value).map_err(|_| BridgeError::InvalidCurrency {
        field: field.to_owned(),
        value: value.to_owned(),
    })
}

/// Reads a typed id from its canonical hyphenated form.
///
/// # Errors
/// [`BridgeError::InvalidId`] if the text is not a UUID.
pub fn id<T: From<Uuid>>(field: &str, value: &str) -> Result<T, BridgeError> {
    Uuid::parse_str(value)
        .map(T::from)
        .map_err(|_| BridgeError::InvalidId {
            field: field.to_owned(),
            value: value.to_owned(),
        })
}

/// Reads an optional id. An absent id is `None`; a present but malformed one is an error,
/// not a quiet `None` — a row pointing at a category nobody can parse is a bug to surface.
///
/// # Errors
/// As [`id`].
pub fn optional_id<T: From<Uuid>>(
    field: &str,
    value: Option<&String>,
) -> Result<Option<T>, BridgeError> {
    match value {
        Some(text) => id(field, text).map(Some),
        None => Ok(None),
    }
}

/// Reads an RFC 3339 instant.
///
/// # Errors
/// [`BridgeError::InvalidTimestamp`] if it does not parse.
pub fn instant(field: &str, value: &str) -> Result<OffsetDateTime, BridgeError> {
    OffsetDateTime::parse(value, &Rfc3339).map_err(|_| BridgeError::InvalidTimestamp {
        field: field.to_owned(),
        value: value.to_owned(),
    })
}

/// Reads an optional RFC 3339 instant.
///
/// # Errors
/// As [`instant`].
pub fn optional_instant(
    field: &str,
    value: Option<&String>,
) -> Result<Option<OffsetDateTime>, BridgeError> {
    match value {
        Some(text) => instant(field, text).map(Some),
        None => Ok(None),
    }
}

/// Reads a `YYYY-MM-DD` date.
///
/// # Errors
/// [`BridgeError::InvalidDate`] if it does not parse.
pub fn date(field: &str, value: &str) -> Result<Date, BridgeError> {
    Date::parse(value, DATE_FORMAT).map_err(|_| BridgeError::InvalidDate {
        field: field.to_owned(),
        value: value.to_owned(),
    })
}

/// Reads an optional `YYYY-MM-DD` date.
///
/// # Errors
/// As [`date`].
pub fn optional_date(field: &str, value: Option<&String>) -> Result<Option<Date>, BridgeError> {
    match value {
        Some(text) => date(field, text).map(Some),
        None => Ok(None),
    }
}

/// Renders an instant back to RFC 3339.
///
/// # Errors
/// [`BridgeError::Unrepresentable`] for an instant outside the years RFC 3339 can spell.
pub fn format_instant(field: &str, value: OffsetDateTime) -> Result<String, BridgeError> {
    value
        .format(&Rfc3339)
        .map_err(|_| BridgeError::Unrepresentable {
            field: field.to_owned(),
        })
}

/// Renders a date back to `YYYY-MM-DD`.
///
/// # Errors
/// [`BridgeError::Unrepresentable`] for a year outside four digits.
pub fn format_date(field: &str, value: Date) -> Result<String, BridgeError> {
    value
        .format(DATE_FORMAT)
        .map_err(|_| BridgeError::Unrepresentable {
            field: field.to_owned(),
        })
}
