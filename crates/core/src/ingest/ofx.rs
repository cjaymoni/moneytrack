//! OFX statements, both the SGML 1.x dialect and the XML 2.x one.
//!
//! OFX names its own fields, so there is no mapping to set up: every movement is a
//! `STMTTRN` element with a signed `TRNAMT`, a `DTPOSTED`, a `FITID` that is the bank's own
//! id for the line, and a `NAME` and `MEMO` describing it.
//!
//! The two dialects differ in one way that matters here: 1.x leaves leaf elements unclosed
//! (`<TRNAMT>-20.00` and then straight on to the next tag) where 2.x closes them. So this is
//! not an XML parser. It walks tags in order, takes the text after each one up to the next
//! `<` as its value, and treats a closing tag as nothing more than a boundary. That reads
//! both dialects with the same few lines and needs no dependency that would also have to
//! be taught about SGML.

use std::collections::HashMap;

use super::text;
use super::{IngestError, Line, LineProblem, Problem, Statement};
use crate::model::{MAX_COUNTERPARTY_LEN, MAX_EXTERNAL_ID_LEN};
use crate::money::{Currency, Money};

/// The aggregate one movement lives in.
const TRANSACTION: &str = "STMTTRN";
/// The aggregate that closes a statement's list of movements.
const TRANSACTION_LIST: &str = "BANKTRANLIST";
/// The statement aggregates: a bank account and a card account.
const STATEMENTS: [&str; 2] = ["STMTRS", "CCSTMTRS"];
/// The element carrying the statement's currency.
const CURRENCY: &str = "CURDEF";

/// Reads an OFX statement.
///
/// # Errors
/// [`IngestError`] when the whole file is unusable: empty, too large, not OFX, more than
/// one account's statement, or a currency other than the account's. A movement that cannot
/// be read is a [`LineProblem`] in the result instead.
pub fn read(bytes: &[u8], currency: Currency) -> Result<Statement, IngestError> {
    let text = super::decode(bytes)?;
    if !text.to_ascii_uppercase().contains("<OFX>") {
        return Err(IngestError::NotOfx);
    }

    let mut statement = Statement::default();
    let mut statements = 0_usize;
    let mut open: Option<HashMap<String, String>> = None;
    let mut position = 0_u32;

    for element in elements(&text) {
        match element {
            Element::Open(tag, value) => {
                if STATEMENTS.contains(&tag.as_str()) {
                    statements += 1;
                    if statements > 1 {
                        return Err(IngestError::SeveralStatements { count: statements });
                    }
                }
                if tag == CURRENCY {
                    check_currency(&value, currency)?;
                }
                if tag == TRANSACTION {
                    // An unclosed movement is ended by the next one starting.
                    if let Some(fields) = open.take() {
                        finish(fields, position, currency, &mut statement)?;
                    }
                    position = position.saturating_add(1);
                    open = Some(HashMap::new());
                } else if let Some(fields) = open.as_mut() {
                    fields.entry(tag).or_insert(value);
                }
            }
            Element::Close(tag) => {
                if tag == TRANSACTION || tag == TRANSACTION_LIST {
                    if let Some(fields) = open.take() {
                        finish(fields, position, currency, &mut statement)?;
                    }
                }
            }
        }
    }
    if let Some(fields) = open.take() {
        finish(fields, position, currency, &mut statement)?;
    }
    if statement.lines.is_empty() && statement.problems.is_empty() {
        return Err(IngestError::Empty);
    }
    Ok(statement)
}

/// Turns one movement's fields into a line or a problem.
fn finish(
    fields: HashMap<String, String>,
    position: u32,
    currency: Currency,
    statement: &mut Statement,
) -> Result<(), IngestError> {
    match read_transaction(&fields, position, currency) {
        Ok(line) => statement.lines.push(line),
        Err(problem) => statement.problems.push(LineProblem {
            line_no: position,
            problem,
        }),
    }
    super::check_line_count(statement.lines.len() + statement.problems.len())
}

/// Refuses a statement in a currency other than the account's.
fn check_currency(code: &str, account: Currency) -> Result<(), IngestError> {
    let code = code.trim();
    match Currency::parse(code) {
        Ok(statement) if statement == account => Ok(()),
        _ => Err(IngestError::CurrencyMismatch {
            statement: text::quoted(code),
            account,
        }),
    }
}

/// One `STMTTRN`'s fields as a line.
fn read_transaction(
    fields: &HashMap<String, String>,
    position: u32,
    currency: Currency,
) -> Result<Line, Problem> {
    let field = |name: &str| {
        fields
            .get(name)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    };

    let posted = field("DTPOSTED").ok_or_else(|| Problem::Malformed {
        reason: "no DTPOSTED".to_owned(),
    })?;
    let booked_on = read_date(posted).ok_or_else(|| Problem::BadDate {
        text: text::quoted(posted),
    })?;

    // A movement in another currency carries its own `<CURRENCY>` aggregate, and its
    // TRNAMT is in that currency, not the account's.
    if let Some(symbol) = field("CURSYM") {
        if Currency::parse(symbol.trim()).ok() != Some(currency) {
            return Err(Problem::OtherCurrency {
                code: text::quoted(symbol),
            });
        }
    }
    let amount_text = field("TRNAMT").ok_or(Problem::NoAmount)?;
    let signed = read_amount(amount_text, currency).ok_or_else(|| Problem::BadAmount {
        text: text::quoted(amount_text),
    })?;
    let (direction, amount) = super::split_signed(signed)?;

    let name = field("NAME").and_then(text::clean);
    let memo = field("MEMO").and_then(text::clean);
    let label = name
        .clone()
        .or(memo)
        .unwrap_or_else(|| format!("Statement line {position}"));
    let counterparty = field("NAME").and_then(|n| text::clean_to(n, MAX_COUNTERPARTY_LEN));
    let external_id = field("FITID").and_then(|id| text::clean_to(id, MAX_EXTERNAL_ID_LEN));

    Ok(Line {
        line_no: position,
        booked_on,
        direction,
        amount,
        label,
        counterparty,
        external_id,
        reported_balance: None,
    })
}

/// The date part of an OFX datetime: `YYYYMMDD`, then optionally a time and a zone.
///
/// The time is dropped for the same reason as a CSV statement's; see
/// [`stage`](super::stage).
fn read_date(value: &str) -> Option<time::Date> {
    let digits = value.trim().get(..8)?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let year: i32 = digits.get(..4)?.parse().ok()?;
    if !text::is_ledger_year(year) {
        return None;
    }
    let month: u8 = digits.get(4..6)?.parse().ok()?;
    let day: u8 = digits.get(6..8)?.parse().ok()?;
    time::Date::from_calendar_date(year, time::Month::try_from(month).ok()?, day).ok()
}

/// A `TRNAMT`: signed, with either a point or — as the specification allows — a comma
/// before the decimals. Unlike a CSV cell, OFX has no thousands separators, so a comma here
/// can only be a decimal one.
fn read_amount(value: &str, currency: Currency) -> Option<Money> {
    let value = value.trim();
    let normalised = if value.contains('.') {
        value.to_owned()
    } else {
        value.replace(',', ".")
    };
    Money::parse(&normalised, currency).ok()
}

/// A tag in document order.
#[derive(Debug, PartialEq, Eq)]
enum Element {
    /// An opening tag, upper-cased, with the text that follows it up to the next tag.
    Open(String, String),
    /// A closing tag, upper-cased.
    Close(String),
}

/// Walks the tags of an OFX body in order. The SGML header before `<OFX>` has no tags and
/// is passed over; an XML declaration or processing instruction is skipped.
fn elements(text: &str) -> impl Iterator<Item = Element> + '_ {
    text.split('<').skip(1).filter_map(|piece| {
        let (tag, rest) = piece.split_once('>')?;
        let tag = tag.trim();
        if tag.starts_with('?') || tag.starts_with('!') {
            return None;
        }
        if let Some(closing) = tag.strip_prefix('/') {
            return Some(Element::Close(closing.trim().to_ascii_uppercase()));
        }
        // `<NAME/>` is an empty element in the XML dialect.
        let tag = tag.trim_end_matches('/').trim();
        Some(Element::Open(
            tag.to_ascii_uppercase(),
            unescape(rest.trim()),
        ))
    })
}

/// The five predefined XML entities and numeric character references, in one left-to-right
/// pass so `&amp;lt;` reads as the text `&lt;`, not as `<`. Anything else that looks like
/// an entity is left as written.
fn unescape(value: &str) -> String {
    if !value.contains('&') {
        return value.to_owned();
    }
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(at) = rest.find('&') {
        out.push_str(rest.get(..at).unwrap_or(""));
        let after = rest.get(at..).unwrap_or("");
        let Some(end) = after.find(';') else {
            out.push_str(after);
            return out;
        };
        let entity = after.get(1..end).unwrap_or("");
        match decode_entity(entity) {
            Some(c) => out.push(c),
            None => out.push_str(after.get(..=end).unwrap_or("")),
        }
        rest = after.get(end + 1..).unwrap_or("");
    }
    out.push_str(rest);
    out
}

/// One entity's character, if it is one this reader knows.
fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let number = entity.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}
