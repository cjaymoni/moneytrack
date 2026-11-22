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
use mt_core::model::{Account, Category, Transaction};
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

/// A category.
#[derive(Debug, Clone, Serialize)]
pub struct CategoryOut {
    /// Row id.
    pub id: String,
    /// What it is called.
    pub name: String,
    /// The category it sits under, absent for a root.
    pub parent_id: Option<String>,
    /// `income` or `expense`.
    pub kind: String,
    /// Ordering within its level.
    pub sort_order: i32,
}

impl From<&Category> for CategoryOut {
    fn from(category: &Category) -> Self {
        Self {
            id: category.id.to_string(),
            name: category.name.clone(),
            parent_id: category.parent_id.map(|id| id.to_string()),
            kind: category.kind.as_str().to_owned(),
            sort_order: category.sort_order,
        }
    }
}

/// What a browser sends to add a category.
#[derive(Debug, Deserialize)]
pub struct NewCategoryIn {
    /// What to call it.
    pub name: String,
    /// `income` or `expense`.
    pub kind: String,
    /// The category to put it under, if any.
    pub parent_id: Option<String>,
}

/// What a browser sends to change a row: the whole of what a person can change, not a patch.
///
/// Whole rather than field-by-field because a form holds the whole row anyway, and a patch
/// format has to spell "leave the note alone" differently from "clear the note", which is the
/// kind of distinction that gets lost between two clients.
#[derive(Debug, Deserialize)]
pub struct EditTransactionIn {
    /// `in` or `out`.
    pub direction: String,
    /// Minor units. This or `amount`.
    pub amount_minor: Option<i64>,
    /// The decimal text a person typed. This or `amount_minor`.
    pub amount: Option<String>,
    /// When it happened, RFC 3339. Required: an edit that forgot the date should not quietly
    /// move the row to now.
    pub booked_at: String,
    /// What it was.
    pub label: String,
    /// Merchant, sender or recipient; null clears it.
    pub counterparty: Option<String>,
    /// Free text; null clears it.
    pub note: Option<String>,
    /// `pending` or `confirmed`.
    pub status: String,
    /// The category; null leaves the row uncategorised.
    pub category_id: Option<String>,
}

/// What a browser sends to file a row under a category.
#[derive(Debug, Deserialize)]
pub struct SetCategoryIn {
    /// The category, or null to leave the row uncategorised.
    pub category_id: Option<String>,
}

/// One period's earning and spending in one currency, by category.
#[derive(Debug, Clone, Serialize)]
pub struct BreakdownOut {
    /// ISO 4217 code.
    pub currency: String,
    /// First local date counted, `YYYY-MM-DD`.
    pub starts_on: String,
    /// Last local date counted, inclusive.
    pub ends_on: String,
    /// Money that left.
    pub spending: SideOut,
    /// Money that arrived.
    pub income: SideOut,
    /// Income less spending.
    pub net: MoneyOut,
}

/// One side of a breakdown.
#[derive(Debug, Clone, Serialize)]
pub struct SideOut {
    /// Everything on this side.
    pub total: MoneyOut,
    /// Largest first.
    pub lines: Vec<LineOut>,
}

/// A share of a side, for a bar and for a label beside it.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ShareOut {
    /// Basis points of the side's total, 0 to 10 000, rounded down by `mt-core`.
    pub basis_points: u16,
    /// The same as a whole percentage for a person to read: "23%", or "<1%" for a sliver
    /// that would otherwise read as nothing.
    pub display: &'static str,
}

/// A top-level category, or the uncategorised pile.
#[derive(Debug, Clone, Serialize)]
pub struct LineOut {
    /// The category, absent for uncategorised.
    pub category_id: Option<String>,
    /// Its name, absent for uncategorised.
    pub name: Option<String>,
    /// Everything in it, children included.
    pub total: MoneyOut,
    /// Its share of the side.
    pub share: ShareOut,
    /// Filed under this category itself rather than a child.
    pub own: MoneyOut,
    /// Its share of the side.
    pub own_share: ShareOut,
    /// Each child with anything in it, largest first.
    pub children: Vec<PartOut>,
}

/// A child within a line.
#[derive(Debug, Clone, Serialize)]
pub struct PartOut {
    /// The child category.
    pub category_id: String,
    /// Its name.
    pub name: String,
    /// What was filed under it.
    pub total: MoneyOut,
    /// Its share of the side, on the same scale as the lines.
    pub share: ShareOut,
}

/// Renders a breakdown, naming each category from the person's own list.
pub fn breakdown_out(
    breakdown: &mt_core::breakdown::Breakdown,
    categories: &[Category],
) -> BreakdownOut {
    let name = |id: mt_core::ids::CategoryId| {
        categories
            .iter()
            .find(|category| category.id == id)
            .map(|category| category.name.clone())
    };
    let side = |side: &mt_core::breakdown::Side| SideOut {
        total: side.total.into(),
        lines: side
            .lines
            .iter()
            .map(|line| LineOut {
                category_id: line.category.map(|id| id.to_string()),
                name: line.category.and_then(name),
                total: line.total.into(),
                share: share(line.share_bps),
                own: line.own.into(),
                own_share: share(line.own_share_bps),
                children: line
                    .children
                    .iter()
                    .map(|part| PartOut {
                        category_id: part.category.to_string(),
                        name: name(part.category).unwrap_or_default(),
                        total: part.total.into(),
                        share: share(part.share_bps),
                    })
                    .collect(),
            })
            .collect(),
    };
    BreakdownOut {
        currency: breakdown.currency.code().to_owned(),
        starts_on: date(breakdown.period.starts_on),
        ends_on: date(breakdown.period.ends_on),
        spending: side(&breakdown.spending),
        income: side(&breakdown.income),
        net: breakdown.net.into(),
    }
}

/// Basis points as a whole percentage a person can read.
///
/// The labels are a fixed table rather than formatted, so the share needs no division on
/// either side of the wire and the output is a `&'static str`.
fn share(basis_points: u16) -> ShareOut {
    const PERCENT: [&str; 101] = [
        "0%", "1%", "2%", "3%", "4%", "5%", "6%", "7%", "8%", "9%", "10%", "11%", "12%", "13%",
        "14%", "15%", "16%", "17%", "18%", "19%", "20%", "21%", "22%", "23%", "24%", "25%", "26%",
        "27%", "28%", "29%", "30%", "31%", "32%", "33%", "34%", "35%", "36%", "37%", "38%", "39%",
        "40%", "41%", "42%", "43%", "44%", "45%", "46%", "47%", "48%", "49%", "50%", "51%", "52%",
        "53%", "54%", "55%", "56%", "57%", "58%", "59%", "60%", "61%", "62%", "63%", "64%", "65%",
        "66%", "67%", "68%", "69%", "70%", "71%", "72%", "73%", "74%", "75%", "76%", "77%", "78%",
        "79%", "80%", "81%", "82%", "83%", "84%", "85%", "86%", "87%", "88%", "89%", "90%", "91%",
        "92%", "93%", "94%", "95%", "96%", "97%", "98%", "99%", "100%",
    ];
    let display = if basis_points > 0 && basis_points < 100 {
        "<1%"
    } else {
        // Rounded down to the whole percent, as the basis points were: a share shown as
        // 100% is the whole side and nothing less.
        PERCENT
            .get(usize::from(basis_points / 100))
            .copied()
            .unwrap_or("100%")
    };
    ShareOut {
        basis_points,
        display,
    }
}

/// A local date as `YYYY-MM-DD`.
fn date(value: time::Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        value.year(),
        u8::from(value.month()),
        value.day()
    )
}

/// One account's balance from now to the end of the window.
#[derive(Debug, Clone, Serialize)]
pub struct OutlookOut {
    /// The account, with both balances.
    pub account: AccountOut,
    /// The start of the window, RFC 3339.
    pub from: String,
    /// The end of the window, RFC 3339, inclusive.
    pub until: String,
    /// What it stands at at the start, expected rows dated in the past included.
    pub start: MoneyOut,
    /// Where `start` sits on the line, 0 (the low) to 10 000 (the high).
    pub start_level_bps: u16,
    /// Every row ahead in the window, with the balance after each.
    pub points: Vec<OutlookPointOut>,
    /// The lowest balance over the window.
    pub low: MoneyOut,
    /// The row that brings it there, absent when the low is the start.
    pub low_transaction_id: Option<String>,
    /// The highest balance over the window.
    pub high: MoneyOut,
    /// Where zero sits on the line, when the line reaches or crosses it.
    pub zero_level_bps: Option<u16>,
    /// `none`, `already` or `at`.
    pub shortfall: &'static str,
    /// The row that first takes it below zero, when `shortfall` is `at`.
    pub shortfall_transaction_id: Option<String>,
}

/// A row ahead, and where it leaves the account.
#[derive(Debug, Clone, Serialize)]
pub struct OutlookPointOut {
    /// The row.
    pub transaction: TransactionOut,
    /// The balance after it, walked by `mt-core`.
    pub balance_after: MoneyOut,
    /// Where that balance sits on the line, 0 to 10 000.
    pub level_bps: u16,
}

/// Renders an outlook.
///
/// # Errors
/// [`ApiError::Internal`] if an instant will not render.
pub fn outlook_out(
    account: AccountOut,
    outlook: &mt_core::outlook::Outlook<'_>,
) -> Result<OutlookOut, ApiError> {
    use mt_core::outlook::Shortfall;
    let points = outlook
        .points
        .iter()
        .map(|entry| {
            Ok(OutlookPointOut {
                transaction: TransactionOut::try_from(entry.transaction)?,
                balance_after: entry.balance_after.into(),
                level_bps: outlook.level_bps(entry.balance_after),
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    let (shortfall, shortfall_transaction_id) = match outlook.shortfall {
        Shortfall::None => ("none", None),
        Shortfall::Already => ("already", None),
        Shortfall::At(entry) => ("at", Some(entry.transaction.id.to_string())),
    };
    Ok(OutlookOut {
        account,
        from: instant("from", outlook.from)?,
        until: instant("until", outlook.until)?,
        start: outlook.start.into(),
        start_level_bps: outlook.level_bps(outlook.start),
        points,
        low: outlook.low.into(),
        low_transaction_id: outlook.low_at.map(|row| row.id.to_string()),
        high: outlook.high.into(),
        zero_level_bps: outlook.zero_level_bps(),
        shortfall,
        shortfall_transaction_id,
    })
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
    /// The category, absent for uncategorised.
    pub category_id: Option<String>,
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
