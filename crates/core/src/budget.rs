//! Caps, and whether spending has run past them.
//!
//! A budget answers one question on one date: of the cap for the period this date falls in,
//! how much is left. Everything that makes that question subtle is here — which period, which
//! rows count, and which categories roll up into which.
//!
//! ## Caps are per currency, never converted
//!
//! A GHS 500 cap and a USD 50 cap on the same category are two budgets. Converting one into
//! the other would make last month's verdict change when the cedi moves, so a month that was
//! under budget at the time could be over budget by the time you looked back at it. Rows in
//! any other currency are not counted against a cap, not converted into it.
//!
//! ## Transfers never count as spending
//!
//! Moving money from the bank to MoMo is not spending it. Both legs are dropped, the same
//! way [`crate::ledger::flows`] drops them.

use std::collections::{HashMap, HashSet};

use crate::enums::{BudgetPeriod, Direction};
use crate::ids::{BudgetId, CategoryId};
use crate::ledger::BalanceBasis;
use crate::model::ValidationError;
use crate::model::{BPS_SCALE, Budget, Category, Transaction};
use crate::money::{Money, MoneyError};
use crate::zone::Zone;
use time::{Date, Duration, OffsetDateTime};

/// A budget could not be evaluated.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BudgetError {
    /// The budget itself does not validate, so there is no sound answer to give.
    ///
    /// A cap of zero or less is the case that matters: without this, spending nothing
    /// against it reports `Over`, and the user is told they have blown a budget that does
    /// not exist. A corrupt row arriving over sync must fail loudly, not confidently.
    #[error("the budget is not valid: {0}")]
    Invalid(#[from] ValidationError),

    /// Totalling the period's spending left `i64` minor units.
    #[error("spending against budget {budget} overflowed i64 minor units: {source}")]
    Arithmetic {
        /// The budget being evaluated.
        budget: BudgetId,
        /// The arithmetic failure underneath.
        #[source]
        source: MoneyError,
    },

    /// A period boundary fell outside the calendar.
    #[error("budget {budget} has a period boundary outside the supported calendar")]
    PeriodOutOfRange {
        /// The budget being evaluated.
        budget: BudgetId,
    },
}

/// Where spending stands against a cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Below the warning threshold.
    Under,
    /// At or past `warn_at_bps` of the cap, but still inside it.
    Warning,
    /// At or past the cap. Equal to the cap counts as over: the cap is the amount you have
    /// to spend, so spending all of it means there is none left.
    Over,
}

/// One window a cap applies over, both ends inclusive, in local dates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Period {
    /// First day counted.
    pub starts_on: Date,
    /// Last day counted.
    pub ends_on: Date,
}

impl Period {
    /// Whether a local date falls in the window.
    #[must_use]
    pub fn contains(&self, date: Date) -> bool {
        date >= self.starts_on && date <= self.ends_on
    }
}

/// What a budget looks like on a given date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    /// The budget evaluated.
    pub budget: BudgetId,
    /// The window counted.
    pub period: Period,
    /// The cap for this period.
    pub cap: Money,
    /// Spending counted against it, as a positive amount.
    pub spent: Money,
    /// `cap - spent`. Negative once the cap has been passed.
    pub remaining: Money,
    /// Spending as basis points of the cap. Capped at [`u16::MAX`] rather than wrapping, so
    /// spending eight times the cap is reported as "far past it" rather than as 20%.
    pub used_bps: u16,
    /// Under, warning, or over.
    pub level: Level,
}

impl Status {
    /// True once the cap has been reached or passed.
    #[must_use]
    pub fn is_over(&self) -> bool {
        matches!(self.level, Level::Over)
    }
}

/// The period of `budget` that `date` falls in, clipped to the budget's own lifetime.
///
/// Returns `None` when the budget is not running on that date. Clipping matters at the
/// edges: a monthly budget created on the 15th counts the 15th to the end of the month, not
/// spending from the 1st that happened before the budget existed.
#[must_use]
pub fn period_containing(budget: &Budget, date: Date) -> Option<Period> {
    if date < budget.starts_on {
        return None;
    }
    if budget.ends_on.is_some_and(|ends_on| date > ends_on) {
        return None;
    }

    let raw = match budget.period {
        BudgetPeriod::Weekly => weekly_block(budget.starts_on, date)?,
        BudgetPeriod::Monthly => calendar_month(date)?,
        BudgetPeriod::Yearly => calendar_year(date)?,
    };

    let starts_on = raw.starts_on.max(budget.starts_on);
    let ends_on = match budget.ends_on {
        Some(ends_on) => raw.ends_on.min(ends_on),
        None => raw.ends_on,
    };
    // Unreachable for a `date` that passed the two guards above — it is itself a witness
    // that the intersection holds a day. Kept so a future change to either bound cannot
    // hand back a period that ends before it starts.
    (starts_on <= ends_on).then_some(Period { starts_on, ends_on })
}

/// The seven-day block, counted from the budget's start date, that `date` falls in.
fn weekly_block(anchor: Date, date: Date) -> Option<Period> {
    let days = (date - anchor).whole_days();
    let block_start_offset = days.div_euclid(7).checked_mul(7)?;
    let starts_on = anchor.checked_add(Duration::days(block_start_offset))?;
    let ends_on = starts_on.checked_add(Duration::days(6))?;
    Some(Period { starts_on, ends_on })
}

/// The calendar month `date` falls in.
fn calendar_month(date: Date) -> Option<Period> {
    let starts_on = date.replace_day(1).ok()?;
    let last = time::util::days_in_month(date.month(), date.year());
    let ends_on = date.replace_day(last).ok()?;
    Some(Period { starts_on, ends_on })
}

/// The calendar year `date` falls in.
fn calendar_year(date: Date) -> Option<Period> {
    let starts_on = Date::from_calendar_date(date.year(), time::Month::January, 1).ok()?;
    let ends_on = Date::from_calendar_date(date.year(), time::Month::December, 31).ok()?;
    Some(Period { starts_on, ends_on })
}

/// Which categories a budget counts, including the children of the one it names.
///
/// Categories nest one level, so a cap on "Transport" covers "Trotro" and "Fuel" beneath it.
/// Built once for a set of budgets rather than walked per transaction.
#[derive(Debug, Default)]
pub struct CategoryTree {
    children: HashMap<CategoryId, Vec<CategoryId>>,
}

impl CategoryTree {
    /// Indexes the parent links of a set of categories.
    #[must_use]
    pub fn build<'a, I>(categories: I) -> Self
    where
        I: IntoIterator<Item = &'a Category>,
    {
        let mut children: HashMap<CategoryId, Vec<CategoryId>> = HashMap::new();
        for category in categories {
            if category.sync.is_deleted() {
                continue;
            }
            if let Some(parent) = category.parent_id {
                children.entry(parent).or_default().push(category.id);
            }
        }
        Self { children }
    }

    /// The category itself and everything nested under it.
    #[must_use]
    pub fn scope_of(&self, category: CategoryId) -> HashSet<CategoryId> {
        let mut scope = HashSet::new();
        scope.insert(category);
        if let Some(children) = self.children.get(&category) {
            scope.extend(children.iter().copied());
        }
        scope
    }
}

/// Whether a row counts against a budget's cap.
///
/// Spending only: money out, not a transfer, not deleted, in the cap's own currency, inside
/// the period, and in the budget's category scope. A budget with no category caps every
/// outgoing row in that currency.
fn counts_against(
    transaction: &Transaction,
    budget: &Budget,
    scope: Option<&HashSet<CategoryId>>,
    period: &Period,
    basis: BalanceBasis,
    zone: &impl Zone,
) -> bool {
    if transaction.sync.is_deleted()
        || transaction.is_transfer()
        || transaction.direction != Direction::Out
        || !basis.admits(transaction.status)
        || transaction.amount.currency() != budget.cap.currency()
    {
        return false;
    }
    if let Some(scope) = scope {
        match transaction.category_id {
            Some(category) if scope.contains(&category) => {}
            _ => return false,
        }
    }
    // A purchase at half past eleven at night belongs to the day the user thinks it does,
    // not to whichever day it is in UTC, or a budget counts it in the wrong month.
    period.contains(zone.local_date(transaction.booked_at))
}

/// Works out where one budget stands on a date.
///
/// `zone` resolves the user's own time zone, the same way [`crate::recurrence`] does and for
/// the same reason: this crate has no zone database.
///
/// Returns `Ok(None)` when the budget is not running on that date, which is not an error —
/// a caller evaluating every budget expects some of them to be dormant.
///
/// # Errors
/// [`BudgetError`] if the period cannot be worked out or the spending total overflows.
pub fn evaluate<'a, I, Z>(
    budget: &Budget,
    categories: &CategoryTree,
    transactions: I,
    on: Date,
    basis: BalanceBasis,
    zone: Z,
) -> Result<Option<Status>, BudgetError>
where
    I: IntoIterator<Item = &'a Transaction>,
    Z: Zone,
{
    budget.validate()?;
    let Some(period) = period_containing(budget, on) else {
        return Ok(None);
    };
    let scope = budget.category_id.map(|id| categories.scope_of(id));

    let mut spent = Money::zero(budget.cap.currency());
    for transaction in transactions {
        if !counts_against(transaction, budget, scope.as_ref(), &period, basis, &zone) {
            continue;
        }
        spent = spent
            .try_add(transaction.amount)
            .map_err(|source| BudgetError::Arithmetic {
                budget: budget.id,
                source,
            })?;
    }

    let remaining = budget
        .cap
        .try_sub(spent)
        .map_err(|source| BudgetError::Arithmetic {
            budget: budget.id,
            source,
        })?;
    let used_bps = used_basis_points(spent, budget.cap);
    // A threshold of zero means "no warning", not "warn from the first cedi" — the schema
    // permits zero, and reading it the other way would put every untouched budget on the
    // screen in amber.
    let warns = budget.warn_at_bps > 0 && used_bps >= budget.warn_at_bps;
    let level = if spent.minor() >= budget.cap.minor() {
        Level::Over
    } else if warns {
        Level::Warning
    } else {
        Level::Under
    };

    Ok(Some(Status {
        budget: budget.id,
        period,
        cap: budget.cap,
        spent,
        remaining,
        used_bps,
        level,
    }))
}

/// Spending as basis points of the cap, in integers throughout.
///
/// `i128` for the multiply: a cap near [`crate::model::MAX_AMOUNT_MINOR`] times ten thousand
/// does not fit in `i64`, and overflowing here would report an over-budget month as under.
fn used_basis_points(spent: Money, cap: Money) -> u16 {
    if cap.minor() <= 0 {
        return u16::MAX;
    }
    if spent.minor() <= 0 {
        return 0;
    }
    let scaled = i128::from(spent.minor()).saturating_mul(i128::from(BPS_SCALE));
    let used = scaled / i128::from(cap.minor());
    u16::try_from(used).unwrap_or(u16::MAX)
}

/// Evaluates every budget on one date, skipping those not running.
///
/// The category tree is built once and shared, rather than rebuilt per budget. The rows are
/// still walked once per budget, which is the right trade at this size: a person has a
/// handful of caps, and indexing spending by category up front would cost more than it
/// saved. If that ever stops being true, this is the function to change, not [`evaluate`].
///
/// # Errors
/// As [`evaluate`], naming the first budget that fails.
pub fn evaluate_all<Z>(
    budgets: &[Budget],
    categories: &[Category],
    transactions: &[Transaction],
    on: Date,
    basis: BalanceBasis,
    zone: Z,
) -> Result<Vec<Status>, BudgetError>
where
    Z: Zone + Copy,
{
    let tree = CategoryTree::build(categories);
    let mut statuses = Vec::new();
    for budget in budgets {
        if budget.sync.is_deleted() {
            continue;
        }
        if let Some(status) = evaluate(budget, &tree, transactions, on, basis, zone)? {
            statuses.push(status);
        }
    }
    Ok(statuses)
}

/// The instants a local period spans, for callers that have to query a store by timestamp.
///
/// Both ends inclusive: the second value is the last representable instant of the final
/// local day, so a purchase at 23:59 is inside the period it looks like it is inside.
#[must_use]
pub fn period_bounds<Z: Zone>(period: &Period, zone: Z) -> (OffsetDateTime, OffsetDateTime) {
    (
        zone.start_of_day(period.starts_on),
        zone.end_of_day(period.ends_on),
    )
}
