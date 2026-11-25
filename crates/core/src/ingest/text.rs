//! Reading the cells of a statement: dates, amounts and descriptions as banks print them.

use serde::{Deserialize, Serialize};
use time::{Date, Month};

use crate::model::{EARLIEST_BOOKED_AT, LATEST_BOOKED_AT, MAX_LABEL_LEN};
use crate::money::{Currency, Money};

/// How much of an unreadable cell is quoted back in a [`Problem`](super::Problem). Enough
/// to recognise it, not enough to turn the problem list into a copy of the file.
const MAX_QUOTED_CHARS: usize = 40;

/// Two-digit years are read as this century. A statement from the 1990s is not something
/// anyone will import into a ledger started this decade.
const TWO_DIGIT_YEAR_BASE: i32 = 2000;

/// The order of day, month and year in a statement's date column.
///
/// The separators are not part of the format: `03/11/2026`, `03-11-2026`, `03.11.2026`,
/// `03-Nov-2026` and `3 Nov 2026` all read the same under [`DateFormat::DayMonthYear`],
/// and a time after the date is ignored. What cannot be worked out from the text is which
/// number is the day, and getting that wrong moves a purchase by months, so the person
/// says once per account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateFormat {
    /// `2026-11-03`.
    YearMonthDay,
    /// `03/11/2026`, the usual order in Ghana and most of the world.
    DayMonthYear,
    /// `11/03/2026`, as some US-built banking systems still export.
    MonthDayYear,
}

/// Reads a date cell in the given order, or `None` if it is not one.
pub(super) fn parse_date(text: &str, format: DateFormat) -> Option<Date> {
    let text = without_iso_time_marker(text);
    let mut parts = text
        .split(['/', '-', '.', ' ', ','])
        .filter(|part| !part.is_empty());
    let (first, second, third) = (parts.next()?, parts.next()?, parts.next()?);
    let (year, month, day) = match format {
        DateFormat::YearMonthDay => (first, second, third),
        DateFormat::DayMonthYear => (third, second, first),
        DateFormat::MonthDayYear => (third, first, second),
    };
    Date::from_calendar_date(parse_year(year)?, parse_month(month)?, parse_day(day)?).ok()
}

/// Replaces the `T` between an ISO date and its time with a space.
///
/// Only a `T` with a digit on each side: the one in `OCT` is part of the month.
fn without_iso_time_marker(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    chars
        .iter()
        .enumerate()
        .map(|(at, &c)| {
            let is_between_digits = c == 'T'
                && at > 0
                && chars.get(at - 1).is_some_and(char::is_ascii_digit)
                && chars.get(at + 1).is_some_and(char::is_ascii_digit);
            if is_between_digits { ' ' } else { c }
        })
        .collect()
}

/// A four-digit year, or a two-digit one read as this century, inside the years a ledger
/// row may carry.
fn parse_year(text: &str) -> Option<i32> {
    let digits = leading_digits(text);
    let value: i32 = digits.parse().ok()?;
    let year = match digits.len() {
        4 => value,
        2 => TWO_DIGIT_YEAR_BASE + value,
        _ => return None,
    };
    is_ledger_year(year).then_some(year)
}

/// Whether a year falls inside the span a ledger row may be booked in.
///
/// `0226` and `2999` are typing slips, not dates; letting one through fails the database's
/// own bound, and that would refuse the whole file over one cell.
pub(super) fn is_ledger_year(year: i32) -> bool {
    (EARLIEST_BOOKED_AT.year()..LATEST_BOOKED_AT.year()).contains(&year)
}

/// A month as a number or an English name, full or abbreviated (`Sept` included).
fn parse_month(text: &str) -> Option<Month> {
    if text.bytes().all(|b| b.is_ascii_digit()) {
        let number: u8 = text.parse().ok()?;
        return Month::try_from(number).ok();
    }
    let lower = text.to_ascii_lowercase();
    let prefix = lower.get(..3)?;
    let month = match prefix {
        "jan" => Month::January,
        "feb" => Month::February,
        "mar" => Month::March,
        "apr" => Month::April,
        "may" => Month::May,
        "jun" => Month::June,
        "jul" => Month::July,
        "aug" => Month::August,
        "sep" => Month::September,
        "oct" => Month::October,
        "nov" => Month::November,
        "dec" => Month::December,
        _ => return None,
    };
    // "Mayday" is not a month: the rest has to be the rest of the name or nothing.
    let full = month.to_string().to_ascii_lowercase();
    let is_month_name = full.starts_with(&lower) || lower == "sept";
    is_month_name.then_some(month)
}

/// A day of the month, ignoring anything after its digits (a time glued on by `T`).
fn parse_day(text: &str) -> Option<u8> {
    let digits = leading_digits(text);
    if digits.is_empty() || digits.len() > 2 {
        return None;
    }
    digits.parse().ok()
}

/// The run of ASCII digits a string starts with.
fn leading_digits(text: &str) -> &str {
    let end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    text.get(..end).unwrap_or("")
}

/// What an amount cell held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Amount {
    /// The cell was empty, or held only a dash.
    Blank,
    /// A signed amount: negative is money out.
    Value(Money),
}

/// Reads an amount cell as banks print them, or `None` if it is not one.
///
/// Accepted on top of a plain decimal: thousands separators (`1,200.50`), the account's own
/// currency code or sign on either side (`GHS 1,200.50`, `₵20`), brackets for a negative
/// (`(20.00)`), and a `CR` or `DR` marker, which decides the sign whatever else the cell says.
///
/// Anything else is refused rather than skipped over. A letter inside the digits (`12O.50`),
/// another currency (`USD 15.99` in a cedi account), an exponent (`1E6`) or a word this does
/// not know (`20.00 DEBIT`) would each read as a plausible wrong number if the stray
/// characters were dropped, and a wrong number that looks right is the worst outcome an
/// import can have.
///
/// The comma is always a thousands separator. Every currency in the registry that has
/// decimals writes them after a point in the statements this has been built against; a
/// bank that prints `20,00` gets a loud bad-amount problem, not a silent factor of a
/// hundred.
pub(super) fn parse_amount(text: &str, currency: Currency) -> Option<Amount> {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed == "-" {
        return Some(Amount::Blank);
    }

    let upper = trimmed.to_uppercase();
    let (body, marker) = strip_marker(&upper);
    let body = strip_currency(body, currency);
    let (body, bracketed) = match body.strip_prefix('(').and_then(|b| b.strip_suffix(')')) {
        Some(inner) => (strip_currency(inner.trim(), currency), true),
        None => (body.clone(), false),
    };

    let mut number = String::with_capacity(body.len());
    for c in body.chars() {
        match c {
            '0'..='9' | '.' | '-' | '+' => number.push(c),
            ',' | ' ' | '\u{a0}' => {}
            _ => return None,
        }
    }
    if !number.bytes().any(|b| b.is_ascii_digit()) {
        return None;
    }

    let parsed = Money::parse(&number, currency).ok()?;
    let magnitude = parsed.abs().ok()?;
    let is_negative = match marker {
        Some(Marker::Credit) => false,
        Some(Marker::Debit) => true,
        None => bracketed || parsed.is_negative(),
    };
    let signed = if is_negative {
        magnitude.try_neg().ok()?
    } else {
        magnitude
    };
    Some(Amount::Value(signed))
}

/// A credit or debit marker printed beside an amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Marker {
    Credit,
    Debit,
}

/// Splits a trailing or leading `CR`/`DR` off an upper-cased amount.
fn strip_marker(upper: &str) -> (&str, Option<Marker>) {
    for (word, marker) in [("CR", Marker::Credit), ("DR", Marker::Debit)] {
        if let Some(rest) = upper.strip_suffix(word) {
            return (rest.trim(), Some(marker));
        }
        if let Some(rest) = upper.strip_prefix(word) {
            return (rest.trim(), Some(marker));
        }
    }
    (upper, None)
}

/// Drops one of the account's own currency code or signs from either end of an amount,
/// keeping a leading sign where it was (`-₵20` → `-20`). Anything that is not the
/// account's currency is left in place, for the caller to refuse.
fn strip_currency(body: &str, currency: Currency) -> String {
    let (sign, unsigned) = match body.strip_prefix(['-', '+']) {
        Some(rest) => (body.get(..1).unwrap_or(""), rest.trim_start()),
        None => ("", body),
    };
    for mark in core::iter::once(currency.code()).chain(signs_of(currency).iter().copied()) {
        let rest = unsigned
            .strip_prefix(mark)
            .or_else(|| unsigned.strip_suffix(mark));
        if let Some(rest) = rest {
            return format!("{sign}{}", rest.trim());
        }
    }
    body.to_owned()
}

/// The signs a bank might print for a currency, longest first so `GH₵` wins over `₵`.
fn signs_of(currency: Currency) -> &'static [&'static str] {
    match currency.code() {
        "GHS" => &["GH₵", "GH¢", "₵", "¢"],
        "USD" | "CAD" | "AUD" => &["US$", "$"],
        "EUR" => &["€"],
        "GBP" => &["£"],
        "NGN" => &["₦"],
        "JPY" | "CNY" => &["¥"],
        "INR" => &["₹"],
        _ => &[],
    }
}

/// A description cell, tidied: control characters dropped, runs of whitespace collapsed,
/// and cut at the label limit on a character boundary.
///
/// Cutting rather than refusing is deliberate. A bank's description running past two
/// hundred characters is no reason to lose the money it describes.
pub(super) fn clean(text: &str) -> Option<String> {
    clean_to(text, MAX_LABEL_LEN)
}

/// [`clean`], to a given number of characters.
pub(super) fn clean_to(text: &str, max_chars: usize) -> Option<String> {
    let collapsed = text
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.is_empty() {
        return None;
    }
    Some(collapsed.chars().take(max_chars).collect())
}

/// An unreadable cell, shortened for quoting back in a problem.
pub(super) fn quoted(text: &str) -> String {
    text.trim().chars().take(MAX_QUOTED_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    const GHS: Currency = Currency::GHS;

    fn value(text: &str) -> i64 {
        match parse_amount(text, GHS) {
            Some(Amount::Value(money)) => money.minor(),
            other => panic!("{text:?} read as {other:?}"),
        }
    }

    #[test]
    fn dates_read_in_each_order_whatever_the_separator() {
        let day = date!(2026 - 11 - 03);
        assert_eq!(
            parse_date("2026-11-03", DateFormat::YearMonthDay),
            Some(day)
        );
        assert_eq!(
            parse_date("2026-11-03T14:22:00", DateFormat::YearMonthDay),
            Some(day)
        );
        assert_eq!(
            parse_date("03/11/2026", DateFormat::DayMonthYear),
            Some(day)
        );
        assert_eq!(parse_date("03.11.26", DateFormat::DayMonthYear), Some(day));
        assert_eq!(
            parse_date("3 Nov 2026 14:22", DateFormat::DayMonthYear),
            Some(day)
        );
        assert_eq!(
            parse_date("03-NOV-2026", DateFormat::DayMonthYear),
            Some(day)
        );
        assert_eq!(
            parse_date("03-OCT-2026", DateFormat::DayMonthYear),
            Some(date!(2026 - 10 - 03))
        );
        assert_eq!(
            parse_date("11/03/2026", DateFormat::MonthDayYear),
            Some(day)
        );
        assert_eq!(
            parse_date("Nov 3, 2026", DateFormat::MonthDayYear),
            Some(day)
        );
        assert_eq!(
            parse_date("3 Sept 2026", DateFormat::DayMonthYear),
            Some(date!(2026 - 09 - 03))
        );
    }

    #[test]
    fn things_that_are_not_dates_are_refused() {
        assert_eq!(
            parse_date("Closing balance", DateFormat::DayMonthYear),
            None
        );
        assert_eq!(parse_date("31/02/2026", DateFormat::DayMonthYear), None);
        assert_eq!(parse_date("13/13/2026", DateFormat::DayMonthYear), None);
        assert_eq!(parse_date("3 Mayday 2026", DateFormat::DayMonthYear), None);
        assert_eq!(parse_date("03/11/226", DateFormat::DayMonthYear), None);
        assert_eq!(parse_date("", DateFormat::DayMonthYear), None);
    }

    #[test]
    fn amounts_read_as_banks_print_them() {
        assert_eq!(value("20"), 2_000);
        assert_eq!(value("-20.50"), -2_050);
        assert_eq!(value("1,200.50"), 120_050);
        assert_eq!(value("GHS 1,200.50"), 120_050);
        assert_eq!(value("₵20.00"), 2_000);
        assert_eq!(value("(20.00)"), -2_000);
        assert_eq!(value("20.00 DR"), -2_000);
        assert_eq!(value("20.00CR"), 2_000);
        assert_eq!(value("-20.00 CR"), 2_000);
    }

    #[test]
    fn the_accounts_own_currency_may_sit_either_side_of_the_figure() {
        assert_eq!(value("GH₵ 20.00"), 2_000);
        assert_eq!(value("20.00 ghs"), 2_000);
        assert_eq!(value("-₵20"), -2_000);
        assert_eq!(value("(GHS 20.00)"), -2_000);
    }

    #[test]
    fn stray_letters_and_other_currencies_are_refused_not_dropped() {
        for cell in [
            "USD 15.99",
            "$15.99",
            "12O.50",
            "1E6",
            "20.00 DEBIT",
            "20 GHS 5",
        ] {
            assert_eq!(parse_amount(cell, GHS), None, "{cell}");
        }
        assert_eq!(
            parse_amount("$15.99", Currency::USD),
            Some(Amount::Value(Money::new(1_599, Currency::USD)))
        );
    }

    #[test]
    fn a_year_no_ledger_row_can_carry_is_not_a_date() {
        assert_eq!(parse_date("03/11/0226", DateFormat::DayMonthYear), None);
        assert_eq!(parse_date("01/01/2999", DateFormat::DayMonthYear), None);
    }

    #[test]
    fn a_blank_amount_cell_is_blank_not_zero() {
        assert_eq!(parse_amount("", GHS), Some(Amount::Blank));
        assert_eq!(parse_amount("  - ", GHS), Some(Amount::Blank));
    }

    #[test]
    fn an_amount_that_would_need_rounding_or_guessing_is_refused() {
        assert_eq!(parse_amount("20.505", GHS), None);
        assert_eq!(parse_amount("twenty", GHS), None);
        assert_eq!(parse_amount("20#00", GHS), None);
        assert_eq!(parse_amount("1.2.3", GHS), None);
    }

    #[test]
    fn descriptions_are_tidied_and_cut_on_a_character_boundary() {
        assert_eq!(
            clean("  POS   PURCHASE\tSHOPRITE \r\n"),
            Some("POS PURCHASE SHOPRITE".to_owned())
        );
        assert_eq!(clean(" \t "), None);
        let long = "é".repeat(MAX_LABEL_LEN + 10);
        assert_eq!(clean(&long).map(|s| s.chars().count()), Some(MAX_LABEL_LEN));
    }
}
