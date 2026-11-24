//! Reading a bank statement into candidates for the inbox.
//!
//! A statement arrives as bytes in one of two shapes. [`csv`] reads a spreadsheet export
//! through a column mapping the person sets once per account, because no two banks agree
//! on what the columns are called or in which order they come. [`ofx`] reads the format
//! several banks export precisely so nobody has to set up a mapping: its fields are defined.
//!
//! Both produce a [`Statement`]: the [`Line`]s that read cleanly and a [`LineProblem`] for
//! every one that did not. A line that will not parse is reported, never guessed at. A
//! statement's closing-balance footer, a page header repeated half way down, a row whose
//! amount is in a column the mapping does not name — each is listed back to the person with
//! its line number, and the rest of the file still comes in.
//!
//! [`stage`] then ties the lines to the account the person chose and turns each into a
//! [`Candidate`] with a fingerprint. Nothing here categorises, matches duplicates, or writes
//! anything: the ladder in [`categorise`](crate::categorise) and the matcher in
//! [`dedup`](crate::dedup) run afterwards, and the inbox decides.
//!
//! ## Why the fingerprint counts repeats
//!
//! Uploading the same statement twice must not offer its rows twice, so each staged line
//! carries a digest of what it says. But two GHS 20 trotro fares on the same day in the same
//! wallet are two lines that say exactly the same thing, and both are real. So the digest
//! also covers how many identical lines came before it in the file: the first fare is
//! occurrence 0 and the second is occurrence 1, on every upload of that statement and on
//! the overlapping half of next month's.

pub mod csv;
pub mod ofx;
mod text;

use core::fmt::Write as _;
use std::collections::HashMap;

use sha2::{Digest, Sha256};
use time::Date;

use crate::enums::Direction;
use crate::ids::AccountId;
use crate::model::{Candidate, MAX_AMOUNT_MINOR};
use crate::money::{Currency, Money};
use crate::zone::Zone;

pub use text::DateFormat;

/// The largest statement file read, in bytes. A year of daily card use is well under this.
pub const MAX_STATEMENT_BYTES: usize = 5 * 1024 * 1024;

/// The most movements one statement may carry.
///
/// Every one becomes an inbox row someone has to look at; past this, the file is almost
/// certainly not one account's statement.
pub const MAX_STATEMENT_LINES: usize = 10_000;

/// Version tag mixed into every fingerprint. Changing what the digest covers means
/// changing this, so old and new fingerprints can never be mistaken for each other.
const FINGERPRINT_VERSION: &str = "mt-ingest-v1";

/// Separates fields inside the fingerprint input, so `("ab", "c")` and `("a", "bc")` hash
/// differently. The ASCII unit separator cannot appear in a cleaned field.
const FIELD_SEPARATOR: char = '\u{1f}';

/// One movement read from a statement, not yet tied to an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Where it was in the source: the physical line of a CSV file, or the position of the
    /// transaction in an OFX file, counting from 1. Shown back to the person beside any
    /// problem, so they can find it in the file they have open.
    pub line_no: u32,
    /// The date the statement gives. Statements carry dates, not instants; the person's
    /// zone turns it into one in [`stage`].
    pub booked_on: Date,
    /// Which way the money moved.
    pub direction: Direction,
    /// How much, never negative.
    pub amount: Money,
    /// The statement's description of the movement.
    pub label: String,
    /// Who was paid or paid in, when the statement says separately.
    pub counterparty: Option<String>,
    /// The institution's own reference for the movement.
    pub external_id: Option<String>,
    /// The balance the statement reports after the movement.
    pub reported_balance: Option<Money>,
}

/// Why one line of a statement was left out.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Problem {
    /// The mapping names a column this line does not have.
    #[error("no column {column} on this line")]
    MissingColumn {
        /// The 0-based column the mapping asked for.
        column: u16,
    },
    /// The date column did not hold a date in the mapping's format.
    #[error("`{text}` is not a date in the chosen format")]
    BadDate {
        /// What was there, trimmed and shortened.
        text: String,
    },
    /// The amount column did not hold an amount in the account's currency.
    #[error("`{text}` is not an amount")]
    BadAmount {
        /// What was there, trimmed and shortened.
        text: String,
    },
    /// Neither the money-in nor the money-out column had anything in it.
    #[error("no amount on this line")]
    NoAmount,
    /// Both the money-in and the money-out column had an amount.
    #[error("an amount in both the money-in and money-out columns")]
    BothAmounts,
    /// The amount was zero, which moves nothing.
    #[error("the amount is zero")]
    ZeroAmount,
    /// The amount is larger than any balance the ledger can hold.
    #[error("the amount is too large to record")]
    AmountTooLarge,
    /// The movement is in a different currency from the account.
    #[error("this transaction is in {code}, not the account's currency")]
    OtherCurrency {
        /// The code the statement gave it.
        code: String,
    },
    /// The line could not be read at all: an unclosed quote, a broken OFX element.
    #[error("the line could not be read: {reason}")]
    Malformed {
        /// What the reader reported.
        reason: String,
    },
}

/// A line that was left out, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineProblem {
    /// Where it was, counted as [`Line::line_no`] is.
    pub line_no: u32,
    /// Why it was left out.
    pub problem: Problem,
}

/// Why a whole file was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IngestError {
    /// The file had nothing in it, or nothing but blank lines.
    #[error("the file is empty")]
    Empty,
    /// The file is larger than [`MAX_STATEMENT_BYTES`].
    #[error("the file is {bytes} bytes; the most that can be imported at once is {limit}")]
    TooLarge {
        /// How big it was.
        bytes: usize,
        /// The limit.
        limit: usize,
    },
    /// The file holds more than [`MAX_STATEMENT_LINES`] movements.
    #[error("the file has more than {limit} rows; split it and import each part")]
    TooManyLines {
        /// The limit.
        limit: usize,
    },
    /// The file does not look like OFX.
    #[error("this is not an OFX file")]
    NotOfx,
    /// The OFX file holds statements for more than one account.
    #[error("this file holds {count} statements; export one account at a time")]
    SeveralStatements {
        /// How many it holds.
        count: usize,
    },
    /// The statement is in a different currency from the account it is going into.
    #[error("the statement is in {statement} but the account is in {account}")]
    CurrencyMismatch {
        /// The statement's currency.
        statement: String,
        /// The account's currency.
        account: Currency,
    },
    /// The column mapping cannot describe a statement.
    #[error("the column mapping is not usable: {reason}")]
    BadMapping {
        /// What is wrong with it.
        reason: &'static str,
    },
}

/// What a reader got out of a file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Statement {
    /// The movements that read cleanly, in file order.
    pub lines: Vec<Line>,
    /// The lines that did not, in file order.
    pub problems: Vec<LineProblem>,
}

/// A statement line tied to an account, ready to be categorised, matched and reviewed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged {
    /// Where it was in the source.
    pub line_no: u32,
    /// Hex SHA-256 of what the line says and how many identical lines preceded it. Two
    /// uploads of the same statement give the same fingerprints; see the module docs.
    pub fingerprint: String,
    /// The proposed transaction, with no category yet.
    pub candidate: Candidate,
}

/// What [`stage`] produced: the lines ready for the inbox, and any it had to leave out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Staging {
    /// Ready for the inbox, in file order.
    pub staged: Vec<Staged>,
    /// Lines that read but would not make a valid row, in file order.
    pub problems: Vec<LineProblem>,
}

/// Ties a statement's lines to the account they are going into.
///
/// Each date becomes the start of that day in the person's zone. Statements give no time
/// worth keeping — banks that print one mostly print a placeholder — and the duplicate
/// matcher's close window is wide enough that a midnight statement line still meets the
/// afternoon alert for the same purchase.
///
/// Every line is checked as a candidate here, so nothing that would fail the database's
/// own bounds gets as far as the database: it comes back as a problem with its line
/// number, and the rest of the statement goes on without it.
///
/// A bank reference that more than one line of the statement carries is dropped from
/// those lines. A transfer and the levy on it often share one, and some banks fill the
/// column with `N/A`; kept, it would make every line after the first look like a certain
/// duplicate of the first, and the ledger refuses two rows with one reference.
#[must_use]
pub fn stage(statement: &Statement, account: AccountId, zone: &impl Zone) -> Staging {
    let mut references: HashMap<&str, u32> = HashMap::new();
    for line in &statement.lines {
        if let Some(reference) = line.external_id.as_deref() {
            *references.entry(reference).or_insert(0) += 1;
        }
    }
    let is_unique = |reference: &str| references.get(reference).copied() == Some(1);

    let mut staging = Staging::default();
    let mut seen: HashMap<String, u32> = HashMap::with_capacity(statement.lines.len());
    for line in &statement.lines {
        let candidate = Candidate {
            account_id: Some(account),
            direction: line.direction,
            amount: line.amount,
            booked_at: zone.start_of_day(line.booked_on),
            label: line.label.clone(),
            counterparty: line.counterparty.clone(),
            category_id: None,
            category_confidence_bps: 0,
            external_id: line
                .external_id
                .clone()
                .filter(|reference| is_unique(reference)),
            reported_balance: line.reported_balance,
        };
        if let Err(reason) = candidate.validate() {
            staging.problems.push(LineProblem {
                line_no: line.line_no,
                problem: Problem::Malformed {
                    reason: reason.to_string(),
                },
            });
            continue;
        }
        let content = fingerprint_content(account, line);
        let occurrence = seen.entry(content.clone()).or_insert(0);
        let fingerprint = digest(&content, *occurrence);
        *occurrence = occurrence.saturating_add(1);
        staging.staged.push(Staged {
            line_no: line.line_no,
            fingerprint,
            candidate,
        });
    }
    staging
}

/// What a line says, as one string, before the occurrence count is added.
///
/// Only what every way of reading the statement agrees on: the date, the movement and the
/// description. A payee, a reference or a running balance appears or not depending on how
/// the person mapped the columns, and a fingerprint that moved with the mapping would offer
/// the whole statement again the first time they added a column to it.
fn fingerprint_content(account: AccountId, line: &Line) -> String {
    let label = line
        .label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let fields = [
        FINGERPRINT_VERSION.to_owned(),
        account.to_string(),
        line.booked_on.to_string(),
        line.direction.as_str().to_owned(),
        line.amount.currency().code().to_owned(),
        line.amount.minor().to_string(),
        label,
    ];
    let mut content = String::new();
    for field in fields {
        content.push_str(&field);
        content.push(FIELD_SEPARATOR);
    }
    content
}

/// Lowercase hex SHA-256 of the content and its occurrence count.
fn digest(content: &str, occurrence: u32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hasher.update(occurrence.to_string().as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        // Writing to a String cannot fail.
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Turns file bytes into text, refusing a file that is too large or empty.
///
/// UTF-8 first, with a byte-order mark dropped. A file that is not UTF-8 is read as
/// Latin-1, which every byte sequence is: banks that export from Windows still do, and the
/// only thing that can go wrong is an accented letter in a description, never an amount.
fn decode(bytes: &[u8]) -> Result<String, IngestError> {
    if bytes.len() > MAX_STATEMENT_BYTES {
        return Err(IngestError::TooLarge {
            bytes: bytes.len(),
            limit: MAX_STATEMENT_BYTES,
        });
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let text = match core::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().map(|&byte| char::from(byte)).collect(),
    };
    if text.trim().is_empty() {
        return Err(IngestError::Empty);
    }
    Ok(text)
}

/// The size of an amount, refusing one the ledger could not hold.
fn magnitude(amount: Money) -> Result<Money, Problem> {
    let size = amount.abs().map_err(|_| Problem::AmountTooLarge)?;
    if size.minor() > MAX_AMOUNT_MINOR {
        return Err(Problem::AmountTooLarge);
    }
    Ok(size)
}

/// Splits a signed amount into the way it moved and how much: negative is money out.
fn split_signed(signed: Money) -> Result<(Direction, Money), Problem> {
    if signed.is_zero() {
        return Err(Problem::ZeroAmount);
    }
    let direction = if signed.is_negative() {
        Direction::Out
    } else {
        Direction::In
    };
    Ok((direction, magnitude(signed)?))
}

/// Refuses a statement with more movements than one review could reasonably take.
fn check_line_count(count: usize) -> Result<(), IngestError> {
    if count > MAX_STATEMENT_LINES {
        return Err(IngestError::TooManyLines {
            limit: MAX_STATEMENT_LINES,
        });
    }
    Ok(())
}
