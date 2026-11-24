//! Money in and out, month by month, on one scale a chart can be drawn against.
//!
//! The totals are [`crate::ledger::flows`]'s rule applied to each month in turn, through the
//! same [`counts_as_flow`] test the breakdown uses, so a month here always agrees with the
//! breakdown of that month: transfers, deleted rows, rows the basis does not admit and rows
//! in another currency are left out.
//!
//! ## The scale
//!
//! A chart of several months needs every bar measured against the same top, and the
//! alternative is the screen dividing one amount by another — arithmetic on money in the
//! place that is not allowed to do any. So the scale is worked out here: a round step, a
//! top that is a whole number of steps at or above the largest amount, and the gridline
//! amounts between. Each figure then carries its height on that scale in basis points.

use crate::breakdown::WHOLE_BPS;
use crate::budget::Period;
use crate::enums::Direction;
use crate::ledger::{BalanceBasis, LedgerError, counts_as_flow};
use crate::model::Transaction;
use crate::money::{Currency, Money};
use crate::zone::Zone;

/// How many gaps the scale aims for between zero and its top. Fewer when the amounts are
/// small enough that the step reaches one whole unit of the currency first.
const INTERVALS: i64 = 4;

/// Several months of earning and spending in one currency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trend {
    /// The currency every figure is in.
    pub currency: Currency,
    /// One per period asked for, in the order asked.
    pub months: Vec<MonthFlow>,
    /// The scale every month's bars are drawn against.
    pub scale: Scale,
}

/// One month's money in and out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonthFlow {
    /// The local dates counted, both ends inclusive.
    pub period: Period,
    /// Money that arrived.
    pub income: Money,
    /// Money that left, as a positive amount.
    pub spending: Money,
    /// `income - spending`. Negative means more left than arrived.
    pub net: Money,
}

/// A chart's vertical scale: zero at the bottom, `top` at the top, a gridline at each tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scale {
    /// The amount at the top: a whole number of round steps, never below the largest bar.
    pub top: Money,
    /// Evenly spaced from zero up to and including `top`. Just zero when there is nothing.
    pub ticks: Vec<Money>,
}

impl Scale {
    /// The smallest round scale that `highest` fits under.
    ///
    /// The step is 1, 2 or 5 times a power of ten in minor units, and never less than one
    /// whole unit of the currency, so a gridline never reads "0.50".
    #[must_use]
    pub fn covering(highest: Money) -> Self {
        let currency = highest.currency();
        let zero = Money::zero(currency);
        let most = highest.minor().max(0);
        if most == 0 {
            return Self {
                top: zero,
                ticks: vec![zero],
            };
        }

        let step = round_step(ceil_div(most, INTERVALS)).max(currency.scale());
        let intervals = ceil_div(most, step);
        // Only an amount within a few steps of `i64::MAX` fails to fit; one step of the
        // amount itself is still a true scale for it.
        let (step, intervals) = match step.checked_mul(intervals) {
            Some(_) => (step, intervals),
            None => (most, 1),
        };
        let ticks = (0..=intervals)
            .map(|index| Money::new(step.saturating_mul(index), currency))
            .collect::<Vec<_>>();
        Self {
            top: Money::new(step.saturating_mul(intervals), currency),
            ticks,
        }
    }

    /// Where `amount` sits on the scale, in basis points: 0 at zero, [`WHOLE_BPS`] at the
    /// top. Negative amounts sit at zero; a scale with nothing on it puts everything there.
    #[must_use]
    pub fn level_bps(&self, amount: Money) -> u16 {
        let top = i128::from(self.top.minor());
        if top <= 0 {
            return 0;
        }
        let above = i128::from(amount.minor().max(0));
        let level = above.saturating_mul(i128::from(WHOLE_BPS)) / top;
        u16::try_from(level).map_or(WHOLE_BPS, |bps| bps.min(WHOLE_BPS))
    }
}

/// The `count` calendar months ending with `month` of `year`, oldest first.
///
/// `None` only if one of them falls outside the calendar `time` can represent.
#[must_use]
pub fn months_ending(year: i32, month: time::Month, count: u8) -> Option<Vec<Period>> {
    let mut periods = Vec::with_capacity(usize::from(count));
    let (mut year, mut month) = (year, month);
    for _ in 0..count {
        let starts_on = time::Date::from_calendar_date(year, month, 1).ok()?;
        let ends_on = time::Date::from_calendar_date(year, month, month.length(year)).ok()?;
        periods.push(Period { starts_on, ends_on });
        if month == time::Month::January {
            year = year.checked_sub(1)?;
        }
        month = month.previous();
    }
    periods.reverse();
    Some(periods)
}

/// Totals each period's money in and out, and the scale they share.
///
/// `zone` decides which local date a row falls on, as it does for the breakdown. A row in
/// none of the periods is not counted.
///
/// # Errors
/// [`LedgerError::Arithmetic`] naming the row if a month's total overflows, or
/// [`LedgerError::TotalOverflow`] if a month's net does.
pub fn trend<'a, I, Z>(
    currency: Currency,
    transactions: I,
    periods: &[Period],
    zone: Z,
    basis: BalanceBasis,
) -> Result<Trend, LedgerError>
where
    I: IntoIterator<Item = &'a Transaction>,
    Z: Zone,
{
    let zero = Money::zero(currency);
    let mut sums = vec![(zero, zero); periods.len()];

    for transaction in transactions {
        if !counts_as_flow(transaction, currency, basis) {
            continue;
        }
        let day = zone.local_date(transaction.booked_at);
        let Some(index) = periods.iter().position(|period| period.contains(day)) else {
            continue;
        };
        let Some((income, spending)) = sums.get_mut(index) else {
            continue;
        };
        let pile = match transaction.direction {
            Direction::In => income,
            Direction::Out => spending,
        };
        *pile = pile
            .try_add(transaction.amount)
            .map_err(|source| LedgerError::Arithmetic {
                transaction: transaction.id,
                source,
            })?;
    }

    let months = periods
        .iter()
        .zip(sums)
        .map(|(period, (income, spending))| {
            let net = income
                .try_sub(spending)
                .map_err(|source| LedgerError::TotalOverflow { currency, source })?;
            Ok(MonthFlow {
                period: *period,
                income,
                spending,
                net,
            })
        })
        .collect::<Result<Vec<_>, LedgerError>>()?;

    let highest = months
        .iter()
        .flat_map(|month| [month.income.minor(), month.spending.minor()])
        .max()
        .unwrap_or(0);

    Ok(Trend {
        currency,
        months,
        scale: Scale::covering(Money::new(highest, currency)),
    })
}

/// One month's spending, day by day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Daily {
    /// The currency every figure is in.
    pub currency: Currency,
    /// The dates counted, both ends inclusive.
    pub period: Period,
    /// One per date in the period, in order, days with nothing spent included.
    pub days: Vec<DaySpend>,
    /// Everything spent in the period, as a positive amount.
    pub total: Money,
    /// The scale every day's bar is drawn against.
    pub scale: Scale,
}

/// What left on one local date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DaySpend {
    /// The local date.
    pub date: time::Date,
    /// Money that left, as a positive amount.
    pub spending: Money,
}

/// Totals each date's spending in `period`, and the scale the days share.
///
/// The same rows count as for [`trend`] and the breakdown, so the days of a month add up
/// to that month's spending there.
///
/// # Errors
/// [`LedgerError::Arithmetic`] naming the row if a day's total overflows, or
/// [`LedgerError::TotalOverflow`] if the period's does.
pub fn daily<'a, I, Z>(
    currency: Currency,
    transactions: I,
    period: Period,
    zone: Z,
    basis: BalanceBasis,
) -> Result<Daily, LedgerError>
where
    I: IntoIterator<Item = &'a Transaction>,
    Z: Zone,
{
    let zero = Money::zero(currency);
    let mut days = Vec::new();
    let mut date = period.starts_on;
    while date <= period.ends_on {
        days.push(DaySpend {
            date,
            spending: zero,
        });
        let Some(next) = date.next_day() else { break };
        date = next;
    }

    for transaction in transactions {
        if transaction.direction != Direction::Out || !counts_as_flow(transaction, currency, basis)
        {
            continue;
        }
        let local = zone.local_date(transaction.booked_at);
        if !period.contains(local) {
            continue;
        }
        let offset = (local - period.starts_on).whole_days();
        let Some(day) = usize::try_from(offset)
            .ok()
            .and_then(|index| days.get_mut(index))
        else {
            continue;
        };
        day.spending =
            day.spending
                .try_add(transaction.amount)
                .map_err(|source| LedgerError::Arithmetic {
                    transaction: transaction.id,
                    source,
                })?;
    }

    let total = Money::total(currency, days.iter().map(|day| day.spending))
        .map_err(|source| LedgerError::TotalOverflow { currency, source })?;
    let highest = days
        .iter()
        .map(|day| day.spending.minor())
        .max()
        .unwrap_or(0);

    Ok(Daily {
        currency,
        period,
        days,
        total,
        scale: Scale::covering(Money::new(highest, currency)),
    })
}

/// `value / by`, rounded up, for a positive `value` and `by`.
fn ceil_div(value: i64, by: i64) -> i64 {
    value / by + i64::from(value % by != 0)
}

/// The smallest of 1, 2 or 5 times a power of ten that is at least `rough`.
fn round_step(rough: i64) -> i64 {
    let mut magnitude: i64 = 1;
    while magnitude <= rough / 10 {
        magnitude *= 10;
    }
    [1, 2, 5, 10]
        .into_iter()
        .filter_map(|factor| magnitude.checked_mul(factor))
        .find(|step| *step >= rough)
        .unwrap_or(rough)
}
