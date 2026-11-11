//! Expanding a recurring template into concrete dated rows.
//!
//! A recurring bill is not evaluated on the fly when a screen asks for it. It generates real
//! `pending` transaction rows forward in time, which is what lets the outlook and the history
//! be one list walked by one function — see [`crate::ledger`]. The price is that generation
//! must be idempotent, and `generated_through` on the template is how: everything up to that
//! date has already been written, so the next run starts the day after.
//!
//! ## Occurrences are computed from the anchor, never from the row before
//!
//! A monthly rule on the 31st falls on the 30th in June and the 28th in February. Working
//! the next date out by adding a month to the last one would make June's clamp stick, and by
//! August the bill would be arriving on the 28th for ever. Every occurrence here is computed
//! from the template's start date and an occurrence number, so a short month borrows nothing
//! from the month after it.
//!
//! ## Time zones
//!
//! A rule stores an IANA zone name but this crate has no zone database. The caller resolves
//! it, by implementing [`Zone`] over the platform's own; see that trait for why it asks two
//! questions rather than one. For a zone without daylight saving, Ghana's among them, a
//! [`time::UtcOffset`] is itself a [`Zone`].

use crate::enums::{RecurrenceScope, TxSource, TxStatus};
use crate::ids::TxId;
use crate::model::{RecurringRule, SyncMeta, Transaction, ValidationError};
use crate::zone::Zone;
use time::{Date, Duration, Month, OffsetDateTime};

/// Most rows one call to [`generate`] will produce.
///
/// A daily rule asked for a decade would otherwise allocate four thousand rows in one go, on
/// a phone, inside a database transaction. Hitting this is a sign the window is too wide,
/// not that the template is wrong, so it is an error the caller can act on rather than a
/// silent truncation that loses the tail of the series.
pub const MAX_GENERATED_PER_CALL: usize = 1_000;

/// The template could not be expanded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecurrenceError {
    /// The template itself does not validate, so there is nothing sound to expand.
    #[error("the recurring template is not valid: {0}")]
    Invalid(#[from] ValidationError),

    /// An occurrence fell outside the calendar `time::Date` can represent.
    #[error("occurrence {occurrence} of the series falls outside the supported calendar")]
    OutOfRange {
        /// Which occurrence, counting from zero.
        occurrence: u32,
    },

    /// The window asked for more rows than one call may produce.
    #[error("that window needs more than {limit} rows; generate a shorter window")]
    TooMany {
        /// The cap that was hit, [`MAX_GENERATED_PER_CALL`].
        limit: usize,
    },
}

/// What one generation run produced.
///
/// Both fields are written together or neither is — see [`generate`] for what happens if a
/// caller writes one without the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generation {
    /// The new pending rows, in date order. Empty is normal and not an error: it means the
    /// template has already been generated this far, or the series has ended.
    pub rows: Vec<Transaction>,
    /// The value to write back to the template's `generated_through`.
    ///
    /// This is the end of the window asked for, not the date of the last row. Storing the
    /// last occurrence instead would make every later run re-walk the empty gap after it,
    /// and a rule whose series has ended would be re-examined for ever.
    pub generated_through: Date,
}

/// The dates a template fires on within `from..=through`, both ends inclusive.
///
/// Dates are local to the rule's own zone; nothing here is an instant yet. The window is
/// additionally clipped to the template's own `starts_on` and `ends_on`.
///
/// # Errors
/// [`RecurrenceError::Invalid`] if the template does not validate, [`RecurrenceError::TooMany`]
/// if the window holds more than [`MAX_GENERATED_PER_CALL`] occurrences, or
/// [`RecurrenceError::OutOfRange`] if the series walks off the end of the calendar.
pub fn occurrences(
    rule: &RecurringRule,
    from: Date,
    through: Date,
) -> Result<Vec<Date>, RecurrenceError> {
    rule.validate()?;

    let last = match rule.ends_on {
        Some(ends_on) if ends_on < through => ends_on,
        _ => through,
    };
    let first = if from > rule.starts_on {
        from
    } else {
        rule.starts_on
    };
    if first > last {
        return Ok(Vec::new());
    }

    let mut dates = Vec::new();
    let mut occurrence: u32 = 0;
    loop {
        let date = nth_occurrence(rule, occurrence)?;
        if date > last {
            break;
        }
        if date >= first {
            if dates.len() == MAX_GENERATED_PER_CALL {
                return Err(RecurrenceError::TooMany {
                    limit: MAX_GENERATED_PER_CALL,
                });
            }
            dates.push(date);
        }
        occurrence = occurrence
            .checked_add(1)
            .ok_or(RecurrenceError::OutOfRange { occurrence })?;
    }
    Ok(dates)
}

/// The date of the `occurrence`-th firing, counting the first as zero.
///
/// Computed from the template's start, never from the previous occurrence, so a month too
/// short to hold the chosen day clamps for that month alone.
fn nth_occurrence(rule: &RecurringRule, occurrence: u32) -> Result<Date, RecurrenceError> {
    use crate::enums::RecurrenceFreq::{Daily, Monthly, Weekly, Yearly};

    let out_of_range = || RecurrenceError::OutOfRange { occurrence };
    let steps = i64::from(occurrence)
        .checked_mul(i64::from(rule.interval))
        .ok_or_else(out_of_range)?;

    match rule.freq {
        Daily => rule
            .starts_on
            .checked_add(Duration::days(steps))
            .ok_or_else(out_of_range),
        Weekly => {
            let aligned = align_to_weekday(rule).ok_or_else(out_of_range)?;
            let days = steps.checked_mul(7).ok_or_else(out_of_range)?;
            aligned
                .checked_add(Duration::days(days))
                .ok_or_else(out_of_range)
        }
        Monthly => {
            let anchor = month_index(rule.starts_on.year(), rule.starts_on.month())
                .ok_or_else(out_of_range)?;
            let index = anchor.checked_add(steps).ok_or_else(out_of_range)?;
            let (year, month) = from_month_index(index).ok_or_else(out_of_range)?;
            clamped_date(year, month, day_of_month(rule)).ok_or_else(out_of_range)
        }
        Yearly => {
            let year = i64::from(rule.starts_on.year())
                .checked_add(steps)
                .ok_or_else(out_of_range)?;
            let year = i32::try_from(year).map_err(|_| out_of_range())?;
            clamped_date(year, rule.starts_on.month(), day_of_month(rule)).ok_or_else(out_of_range)
        }
    }
}

/// The first date on or after the template's start that falls on its chosen weekday.
fn align_to_weekday(rule: &RecurringRule) -> Option<Date> {
    let wanted = rule.by_weekday?;
    let current = rule.starts_on.weekday().number_days_from_monday();
    // Rust's `%` on the difference would need a signed cast; going the long way round the
    // week keeps it in u8 and says what it means.
    let forward = (u16::from(wanted) + 7 - u16::from(current)) % 7;
    rule.starts_on
        .checked_add(Duration::days(i64::from(forward)))
}

/// The day of the month a monthly or yearly rule asks for, defaulting to the start date's
/// own day. Validation requires the field, so the default is belt and braces.
fn day_of_month(rule: &RecurringRule) -> u8 {
    rule.by_month_day.unwrap_or_else(|| rule.starts_on.day())
}

/// Months since year zero, so month arithmetic is one addition rather than a carry loop.
///
/// Checked rather than saturating, although `time`'s calendar tops out around month index
/// 120,000 and cannot reach the edge: a saturate is exactly what would turn this strictly
/// increasing sequence into a fixed point, and the loop in [`occurrences`] relies on it
/// increasing to terminate at all.
fn month_index(year: i32, month: Month) -> Option<i64> {
    let month_number = i64::from(u8::from(month));
    i64::from(year)
        .checked_mul(12)?
        .checked_add(month_number - 1)
}

/// The inverse of [`month_index`].
fn from_month_index(index: i64) -> Option<(i32, Month)> {
    let year = i32::try_from(index.div_euclid(12)).ok()?;
    let month = u8::try_from(index.rem_euclid(12) + 1).ok()?;
    Month::try_from(month).ok().map(|month| (year, month))
}

/// A date on the chosen day, or on the last day of the month when it is too short.
///
/// Clamping rather than skipping is the decision the plan records: a rule set for the 31st
/// should bill in February, on the 28th, not go quiet for a month.
fn clamped_date(year: i32, month: Month, day: u8) -> Option<Date> {
    let last = time::util::days_in_month(month, year);
    let day = if day > last { last } else { day };
    Date::from_calendar_date(year, month, day).ok()
}

/// Generates the pending rows a template owes up to `through`, and says how far it has got.
///
/// Idempotent: rows already generated, as recorded by the template's `generated_through`,
/// are not generated again. Asking for a date the template has already passed produces no
/// rows and leaves `generated_through` where it was.
///
/// Rows come back as `pending` with source `recurring`, booked at local midnight on each
/// date in the template's own zone. They are ordinary ledger rows from that point on: the
/// user can confirm one, edit one, or delete one, and the balance walk makes no distinction.
///
/// `new_id` supplies an id per row and `now` the sync timestamps, because this crate has
/// neither a clock nor a source of randomness. `zone` resolves the template's own zone.
///
/// ## The caller must write both halves of the result together
///
/// [`Generation`] hands back rows and a new watermark, and the idempotency this function
/// promises holds only if they are written in one database transaction. Writing the rows and
/// then failing to move `generated_through` produces the same rows again on the next run;
/// moving the watermark without the rows leaves a gap nothing will ever fill, because the
/// watermark only moves forward. Both schemas carry a unique index on
/// `(recurring_id, booked_at)` as a backstop against the first case, which is the one that
/// corrupts the numbers rather than merely losing a row.
///
/// # Errors
/// As [`occurrences`].
pub fn generate<I, Z>(
    rule: &RecurringRule,
    through: Date,
    now: OffsetDateTime,
    zone: Z,
    mut new_id: I,
) -> Result<Generation, RecurrenceError>
where
    I: FnMut() -> TxId,
    Z: Zone,
{
    rule.validate()?;

    let from = match rule.generated_through {
        Some(done) if done >= through => {
            // Already generated this far. Report the template's own watermark back rather
            // than the narrower window, so a caller that writes the result does not move
            // `generated_through` backwards and cause the gap to be generated twice.
            return Ok(Generation {
                rows: Vec::new(),
                generated_through: done,
            });
        }
        Some(done) => done.next_day().unwrap_or(done),
        None => rule.starts_on,
    };

    let dates = occurrences(rule, from, through)?;
    let rows = dates
        .into_iter()
        .map(|date| {
            let booked_at = zone.start_of_day(date);
            Transaction {
                id: new_id(),
                account_id: rule.account_id,
                direction: rule.direction,
                amount: rule.amount,
                booked_at,
                category_id: rule.category_id,
                label: rule.label.clone(),
                counterparty: rule.counterparty.clone(),
                note: None,
                status: TxStatus::Pending,
                source: TxSource::Recurring,
                source_ref: None,
                external_id: None,
                recurring_id: Some(rule.id),
                transfer_id: None,
                sync: SyncMeta {
                    created_at: now,
                    updated_at: now,
                    deleted_at: None,
                    device_id: rule.sync.device_id,
                },
            }
        })
        .collect();

    Ok(Generation {
        rows,
        generated_through: through,
    })
}

/// Whether an edit made with `scope` at `pivot` reaches an instance booked at `booked_at`.
///
/// Asked explicitly at edit time rather than guessed, which is the whole point: "change the
/// rent" means one of four quite different things, and every recurring-transaction feature
/// that gets this wrong got it wrong by choosing one of them silently.
#[must_use]
pub fn scope_selects(
    scope: RecurrenceScope,
    pivot: OffsetDateTime,
    booked_at: OffsetDateTime,
) -> bool {
    // Compared as instants rather than as local dates: the pivot is the row the user has
    // open, so there is no local-midnight question to get wrong here.
    match scope {
        RecurrenceScope::This => booked_at == pivot,
        RecurrenceScope::Future => booked_at >= pivot,
        RecurrenceScope::Past => booked_at <= pivot,
        RecurrenceScope::All => true,
    }
}

/// The instances of one template an edit at `pivot` with this `scope` applies to.
///
/// Soft-deleted rows are left out: an edit should not resurrect something the user removed.
#[must_use]
pub fn instances_for_edit<'a, I>(
    rule: &RecurringRule,
    scope: RecurrenceScope,
    pivot: OffsetDateTime,
    transactions: I,
) -> Vec<&'a Transaction>
where
    I: IntoIterator<Item = &'a Transaction>,
{
    transactions
        .into_iter()
        .filter(|transaction| {
            transaction.recurring_id == Some(rule.id)
                && !transaction.sync.is_deleted()
                && scope_selects(scope, pivot, transaction.booked_at)
        })
        .collect()
}
