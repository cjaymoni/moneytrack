//! Spreadsheet exports, read through a column mapping.
//!
//! A CSV statement says nothing about itself. One bank puts the date first and the amount
//! in one signed column; the next splits money in and money out into two columns and puts
//! three lines of account details above the header. So the person looks at a [`preview`]
//! once, says which column is which, and the [`Mapping`] is kept for that account and
//! reused for every statement after.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::text::{self, Amount, DateFormat};
use super::{IngestError, Line, LineProblem, Problem, Statement};
use crate::model::{MAX_COUNTERPARTY_LEN, MAX_EXTERNAL_ID_LEN};
use crate::money::{Currency, Money};

/// A 0-based column position.
pub type Column = u16;

/// The most leading lines a mapping may skip. Statements put an account summary above the
/// header; nobody puts fifty lines of one.
pub const MAX_SKIP_ROWS: u16 = 50;

/// The widest row a mapping may point into.
pub const MAX_COLUMN: Column = 200;

/// How many rows [`preview`] returns at most.
pub const MAX_PREVIEW_ROWS: usize = 20;

/// The longest cell [`preview`] shows. Enough to recognise a column.
const MAX_PREVIEW_CELL_CHARS: usize = 60;

/// How many lines the delimiter is guessed from.
const SNIFF_LINES: usize = 20;

/// What separates the cells. A closed set, because a bank's export is one of these and an
/// arbitrary byte here would be one more thing to validate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Delimiter {
    /// `,`
    Comma,
    /// `;`, as spreadsheets set to a decimal-comma locale export.
    Semicolon,
    /// A tab.
    Tab,
    /// `|`
    Pipe,
}

impl Delimiter {
    /// Every delimiter, in the order [`sniff`] prefers them on a tie.
    pub const ALL: [Self; 4] = [Self::Comma, Self::Semicolon, Self::Tab, Self::Pipe];

    /// The byte itself.
    #[must_use]
    pub const fn byte(self) -> u8 {
        match self {
            Self::Comma => b',',
            Self::Semicolon => b';',
            Self::Tab => b'\t',
            Self::Pipe => b'|',
        }
    }
}

/// Where the amount is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AmountColumns {
    /// One column, negative for money out. A `DR` or `CR` beside the number also counts.
    Signed {
        /// The column.
        column: Column,
    },
    /// Two columns, one filled per line.
    Split {
        /// The column money in is printed in.
        money_in: Column,
        /// The column money out is printed in, with or without a minus sign.
        money_out: Column,
    },
}

/// Which column of a CSV statement holds what.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mapping {
    /// What separates the cells.
    pub delimiter: Delimiter,
    /// Lines above the header (or above the first movement, if there is no header).
    pub skip_rows: u16,
    /// Whether the first line after the skipped ones names the columns.
    pub has_header: bool,
    /// The date column.
    pub date: Column,
    /// The order of day, month and year in it.
    pub date_format: DateFormat,
    /// Where the amount is.
    pub amount: AmountColumns,
    /// The description column, which becomes the label.
    pub label: Column,
    /// The column naming who was paid, if the statement has one.
    pub counterparty: Option<Column>,
    /// The bank's reference for the movement, if the statement has one.
    pub reference: Option<Column>,
    /// The running balance after each movement, if the statement has one.
    pub balance: Option<Column>,
}

impl Mapping {
    /// Checks that the mapping could describe a statement.
    ///
    /// # Errors
    /// [`IngestError::BadMapping`] saying what is wrong.
    pub fn validate(&self) -> Result<(), IngestError> {
        if self.skip_rows > MAX_SKIP_ROWS {
            return Err(IngestError::BadMapping {
                reason: "too many lines to skip",
            });
        }
        let amount_columns = match self.amount {
            AmountColumns::Signed { column } => vec![column],
            AmountColumns::Split {
                money_in,
                money_out,
            } => {
                if money_in == money_out {
                    return Err(IngestError::BadMapping {
                        reason: "money in and money out must be different columns",
                    });
                }
                vec![money_in, money_out]
            }
        };
        let named = [self.date, self.label]
            .into_iter()
            .chain(amount_columns.iter().copied())
            .chain(self.counterparty)
            .chain(self.reference)
            .chain(self.balance);
        for column in named {
            if column > MAX_COLUMN {
                return Err(IngestError::BadMapping {
                    reason: "a column is past the widest row allowed",
                });
            }
        }
        if amount_columns.contains(&self.date) {
            return Err(IngestError::BadMapping {
                reason: "the date and the amount must be different columns",
            });
        }
        Ok(())
    }
}

/// The first rows of a file, split into cells, for choosing a mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    /// The delimiter the rows were split on.
    pub delimiter: Delimiter,
    /// Up to [`MAX_PREVIEW_ROWS`] rows, each cell trimmed and shortened.
    pub rows: Vec<Vec<String>>,
}

/// Splits the start of a file into cells so the person can say which column is which.
///
/// With no delimiter given, one is guessed with [`sniff`].
///
/// # Errors
/// [`IngestError::Empty`] or [`IngestError::TooLarge`].
pub fn preview(bytes: &[u8], delimiter: Option<Delimiter>) -> Result<Preview, IngestError> {
    let text = super::decode(bytes)?;
    let delimiter = delimiter.unwrap_or_else(|| sniff(&text));
    let mut rows = Vec::new();
    for record in reader(&text, delimiter).records() {
        if rows.len() >= MAX_PREVIEW_ROWS {
            break;
        }
        // A malformed record still takes a row, so the row the person picks as the header
        // here is the same record `read` skips to. The import reports it properly.
        let Ok(record) = record else {
            rows.push(Vec::new());
            continue;
        };
        rows.push(
            record
                .iter()
                .map(|cell| cell.trim().chars().take(MAX_PREVIEW_CELL_CHARS).collect())
                .collect(),
        );
    }
    Ok(Preview { delimiter, rows })
}

/// Guesses the delimiter from the first lines of a file.
///
/// The one whose count per line is the same, and highest, across the most lines wins;
/// a comma on a tie. Quoted cells are not special-cased: a comma inside a quoted
/// description throws one line's count off, and the lines around it outvote it.
#[must_use]
pub fn sniff(text: &str) -> Delimiter {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(SNIFF_LINES)
        .collect();
    let mut best = (Delimiter::Comma, 0_usize);
    for delimiter in Delimiter::ALL {
        let agreeing = lines_agreeing_on_a_count(&lines, delimiter.byte());
        if agreeing > best.1 {
            best = (delimiter, agreeing);
        }
    }
    best.0
}

/// How many lines share the most common non-zero count of a byte.
fn lines_agreeing_on_a_count(lines: &[&str], byte: u8) -> usize {
    let mut lines_per_count: HashMap<usize, usize> = HashMap::new();
    for line in lines {
        let count = line.bytes().filter(|&b| b == byte).count();
        if count > 0 {
            *lines_per_count.entry(count).or_insert(0) += 1;
        }
    }
    lines_per_count.into_values().max().unwrap_or(0)
}

/// Reads a CSV statement through a mapping.
///
/// # Errors
/// [`IngestError`] when the whole file is unusable: empty, too large, too many rows, or a
/// mapping that fails [`Mapping::validate`]. A single unreadable line is a
/// [`LineProblem`] in the result instead.
pub fn read(bytes: &[u8], mapping: &Mapping, currency: Currency) -> Result<Statement, IngestError> {
    mapping.validate()?;
    let text = super::decode(bytes)?;
    let header_rows = usize::from(mapping.skip_rows) + usize::from(mapping.has_header);

    let mut statement = Statement::default();
    for (index, record) in reader(&text, mapping.delimiter).records().enumerate() {
        let line_no = match &record {
            Ok(record) => record.position().map(|p| p.line()),
            Err(error) => error.position().map(|p| p.line()),
        }
        .and_then(|line| u32::try_from(line).ok())
        .unwrap_or(u32::MAX);

        if index < header_rows {
            continue;
        }
        let record = match record {
            Ok(record) => record,
            Err(error) => {
                statement.problems.push(LineProblem {
                    line_no,
                    problem: Problem::Malformed {
                        reason: error.to_string(),
                    },
                });
                super::check_line_count(statement.lines.len() + statement.problems.len())?;
                continue;
            }
        };
        if record.iter().all(|cell| cell.trim().is_empty()) {
            continue;
        }
        match read_line(&record, mapping, currency, line_no) {
            Ok(line) => statement.lines.push(line),
            Err(problem) => statement.problems.push(LineProblem { line_no, problem }),
        }
        super::check_line_count(statement.lines.len() + statement.problems.len())?;
    }
    if statement.lines.is_empty() && statement.problems.is_empty() {
        return Err(IngestError::Empty);
    }
    Ok(statement)
}

/// A reader over the whole text, every row as data and rows allowed to differ in width.
fn reader(text: &str, delimiter: Delimiter) -> ::csv::Reader<&[u8]> {
    ::csv::ReaderBuilder::new()
        .delimiter(delimiter.byte())
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes())
}

/// One record through the mapping.
fn read_line(
    record: &::csv::StringRecord,
    mapping: &Mapping,
    currency: Currency,
    line_no: u32,
) -> Result<Line, Problem> {
    let cell = |column: Column| {
        record
            .get(usize::from(column))
            .ok_or(Problem::MissingColumn { column })
    };
    let optional_cell = |column: Option<Column>| match column {
        Some(column) => cell(column).map(Some),
        None => Ok(None),
    };

    let date_text = cell(mapping.date)?;
    let booked_on =
        text::parse_date(date_text, mapping.date_format).ok_or_else(|| Problem::BadDate {
            text: text::quoted(date_text),
        })?;
    let (direction, amount) = super::split_signed(read_amount(&cell, mapping.amount, currency)?)?;

    let counterparty = optional_cell(mapping.counterparty)?
        .and_then(|text| text::clean_to(text, MAX_COUNTERPARTY_LEN));
    let external_id = optional_cell(mapping.reference)?
        .and_then(|text| text::clean_to(text, MAX_EXTERNAL_ID_LEN));
    let reported_balance = match optional_cell(mapping.balance)? {
        Some(balance_text) => match text::parse_amount(balance_text, currency) {
            Some(Amount::Value(balance)) => {
                // A balance this large is almost always the wrong column — a card or account
                // number — and the ledger could not hold it anyway.
                super::magnitude(balance)?;
                Some(balance)
            }
            Some(Amount::Blank) => None,
            None => {
                return Err(Problem::BadAmount {
                    text: text::quoted(balance_text),
                });
            }
        },
        None => None,
    };
    // A movement with no description still moved money. It keeps the counterparty, or
    // failing that its line number, so it can be found in the inbox and in the file.
    let label = text::clean(cell(mapping.label)?)
        .or_else(|| counterparty.clone())
        .unwrap_or_else(|| format!("Statement line {line_no}"));

    Ok(Line {
        line_no,
        booked_on,
        direction,
        amount,
        label,
        counterparty,
        external_id,
        reported_balance,
    })
}

/// The signed amount on a line: negative is money out.
fn read_amount<'r>(
    cell: &impl Fn(Column) -> Result<&'r str, Problem>,
    columns: AmountColumns,
    currency: Currency,
) -> Result<Money, Problem> {
    let read = |column: Column| {
        let text = cell(column)?;
        text::parse_amount(text, currency).ok_or_else(|| Problem::BadAmount {
            text: text::quoted(text),
        })
    };
    match columns {
        AmountColumns::Signed { column } => match read(column)? {
            Amount::Value(money) => Ok(money),
            Amount::Blank => Err(Problem::NoAmount),
        },
        AmountColumns::Split {
            money_in,
            money_out,
        } => match (
            blank_if_zero(read(money_in)?),
            blank_if_zero(read(money_out)?),
        ) {
            (Amount::Value(_), Amount::Value(_)) => Err(Problem::BothAmounts),
            (Amount::Blank, Amount::Blank) => Err(Problem::NoAmount),
            // Money in is money in, whatever sign the bank printed on it.
            (Amount::Value(incoming), Amount::Blank) => super::magnitude(incoming),
            // Some banks print the debit column with a minus and some without; either way
            // it is money out.
            (Amount::Blank, Amount::Value(outgoing)) => super::magnitude(outgoing)?
                .try_neg()
                .map_err(|_| Problem::AmountTooLarge),
        },
    }
}

/// In a pair of money-in and money-out columns, many banks print `0.00` in the side that
/// did not move rather than leaving it empty. A zero there is the same as a blank.
fn blank_if_zero(amount: Amount) -> Amount {
    match amount {
        Amount::Value(money) if money.is_zero() => Amount::Blank,
        other => other,
    }
}
