//! Where the money went: one period's earning and spending, grouped by category.
//!
//! The rule is [`crate::ledger::flows`] with the rows sorted into piles, and it keeps every
//! decision `flows` makes. Transfers are dropped, because moving your own money between your
//! own accounts is neither earning nor spending. Deleted rows, rows the basis does not admit
//! and rows in another currency are dropped. So the two always agree: the spending side of a
//! breakdown adds up to exactly the spending `flows` reports for the same rows, which a test
//! holds it to.
//!
//! ## Which side a row is on
//!
//! Its direction decides, not its category's kind. A refund that comes in filed under
//! "Food & groceries" is money that arrived, and it is counted on the income side under that
//! name. Netting it against the spending instead would be a guess about what the person
//! meant, and a breakdown that guesses cannot be checked against the ledger beside it.
//!
//! ## Nesting
//!
//! Categories nest one level. A child's rows count toward its parent's line, and the line
//! lists each child beside what was filed under the parent itself. A row whose category is
//! not among those given, or was deleted, is uncategorised: it still happened, and dropping
//! it would leave the total short by an amount nobody could find. A child whose parent is
//! gone stands as a line of its own.
//!
//! ## Shares
//!
//! Every line and every part carries its share of its side's total, in basis points, worked
//! out here in integers. A screen drawing a bar needs a fraction, and the alternative is the
//! screen dividing one amount by another, which is arithmetic on money in the place that is
//! not allowed to do any.

use std::collections::{BTreeMap, HashMap};

use crate::budget::Period;
use crate::enums::Direction;
use crate::ids::CategoryId;
use crate::ledger::{BalanceBasis, LedgerError, counts_as_flow};
use crate::model::{Category, Transaction};
use crate::money::{Currency, Money};
use crate::zone::Zone;

/// Ten thousand basis points make the whole.
pub const WHOLE_BPS: u16 = 10_000;

/// One period's earning and spending in one currency, by category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breakdown {
    /// The currency every figure is in.
    pub currency: Currency,
    /// The local dates counted, both ends inclusive.
    pub period: Period,
    /// Money that left.
    pub spending: Side,
    /// Money that arrived.
    pub income: Side,
    /// `income - spending`. Negative means more left than arrived.
    pub net: Money,
}

/// One side of a breakdown: its total and the lines that make it up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Side {
    /// Everything on this side, as a positive amount.
    pub total: Money,
    /// Largest first; equal totals in the categories' own order, uncategorised last.
    pub lines: Vec<Line>,
}

/// A top-level category, or the uncategorised pile, and what it came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The category, or `None` for rows filed under none.
    pub category: Option<CategoryId>,
    /// Everything in it, its children included.
    pub total: Money,
    /// `total` as basis points of the side's total.
    pub share_bps: u16,
    /// What was filed under this category itself rather than under one of its children.
    pub own: Money,
    /// Each child with anything in it, largest first.
    pub children: Vec<Part>,
}

/// A child category within a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Part {
    /// The child.
    pub category: CategoryId,
    /// What was filed under it.
    pub total: Money,
    /// `total` as basis points of the side's total, so a part and a line can be drawn on
    /// one scale.
    pub share_bps: u16,
}

/// Groups a period's rows by category, on each side.
///
/// `zone` decides which local date a row falls on, so a purchase at half past eleven at
/// night lands in the month the person thinks it does rather than whichever month it is in
/// UTC — the same reading [`crate::budget`] makes.
///
/// # Errors
/// [`LedgerError::Arithmetic`] naming the row if a total overflows.
pub fn breakdown<'a, I, Z>(
    currency: Currency,
    transactions: I,
    categories: &[Category],
    period: Period,
    zone: Z,
    basis: BalanceBasis,
) -> Result<Breakdown, LedgerError>
where
    I: IntoIterator<Item = &'a Transaction>,
    Z: Zone,
{
    let known: HashMap<CategoryId, &Category> = categories
        .iter()
        .filter(|category| !category.sync.is_deleted())
        .map(|category| (category.id, category))
        .collect();

    let mut spending = Piles::new(currency);
    let mut income = Piles::new(currency);

    for transaction in transactions {
        if !counts_as_flow(transaction, currency, basis)
            || !period.contains(zone.local_date(transaction.booked_at))
        {
            continue;
        }
        let piles = match transaction.direction {
            Direction::Out => &mut spending,
            Direction::In => &mut income,
        };
        piles
            .add(
                placement(transaction.category_id, &known),
                transaction.amount,
            )
            .map_err(|source| LedgerError::Arithmetic {
                transaction: transaction.id,
                source,
            })?;
    }

    let spending = spending.into_side(&known)?;
    let income = income.into_side(&known)?;
    let net = income
        .total
        .try_sub(spending.total)
        .map_err(|source| LedgerError::TotalOverflow { currency, source })?;

    Ok(Breakdown {
        currency,
        period,
        spending,
        income,
        net,
    })
}

/// Where a row is counted: the line it belongs to, and the child within it if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Placement {
    line: Option<CategoryId>,
    child: Option<CategoryId>,
}

fn placement(category: Option<CategoryId>, known: &HashMap<CategoryId, &Category>) -> Placement {
    let Some(found) = category.and_then(|id| known.get(&id)) else {
        return Placement {
            line: None,
            child: None,
        };
    };
    match found.parent_id {
        Some(parent) if known.contains_key(&parent) => Placement {
            line: Some(parent),
            child: Some(found.id),
        },
        _ => Placement {
            line: Some(found.id),
            child: None,
        },
    }
}

/// Running totals for one side while the rows are read.
struct Piles {
    currency: Currency,
    lines: BTreeMap<Option<CategoryId>, (Money, Money, BTreeMap<CategoryId, Money>)>,
}

impl Piles {
    fn new(currency: Currency) -> Self {
        Self {
            currency,
            lines: BTreeMap::new(),
        }
    }

    fn add(&mut self, at: Placement, amount: Money) -> Result<(), crate::money::MoneyError> {
        let zero = Money::zero(self.currency);
        let (total, own, children) = self
            .lines
            .entry(at.line)
            .or_insert_with(|| (zero, zero, BTreeMap::new()));
        *total = total.try_add(amount)?;
        match at.child {
            Some(child) => {
                let part = children.entry(child).or_insert(zero);
                *part = part.try_add(amount)?;
            }
            None => *own = own.try_add(amount)?,
        }
        Ok(())
    }

    fn into_side(self, known: &HashMap<CategoryId, &Category>) -> Result<Side, LedgerError> {
        let currency = self.currency;
        let total = Money::total(currency, self.lines.values().map(|(total, _, _)| *total))
            .map_err(|source| LedgerError::TotalOverflow { currency, source })?;

        let order = |category: &Option<CategoryId>| {
            category
                .and_then(|id| known.get(&id))
                .map(|found| found.sort_order)
        };

        let mut lines: Vec<Line> = self
            .lines
            .into_iter()
            .map(|(category, (line_total, own, children))| {
                let mut children: Vec<Part> = children
                    .into_iter()
                    .map(|(child, part_total)| Part {
                        category: child,
                        total: part_total,
                        share_bps: share_bps(part_total, total),
                    })
                    .collect();
                children.sort_by(|left, right| {
                    right
                        .total
                        .minor()
                        .cmp(&left.total.minor())
                        .then_with(|| left.category.cmp(&right.category))
                });
                Line {
                    category,
                    total: line_total,
                    share_bps: share_bps(line_total, total),
                    own,
                    children,
                }
            })
            .collect();

        // Largest first. On a tie, the categories' own order, and uncategorised after every
        // named line, so the order is the same on every device and every reload.
        lines.sort_by(|left, right| {
            right
                .total
                .minor()
                .cmp(&left.total.minor())
                .then_with(|| match (order(&left.category), order(&right.category)) {
                    (Some(a), Some(b)) => a.cmp(&b),
                    (Some(_), None) => core::cmp::Ordering::Less,
                    (None, Some(_)) => core::cmp::Ordering::Greater,
                    (None, None) => core::cmp::Ordering::Equal,
                })
                .then_with(|| match (left.category, right.category) {
                    (Some(a), Some(b)) => a.cmp(&b),
                    (Some(_), None) => core::cmp::Ordering::Less,
                    (None, Some(_)) => core::cmp::Ordering::Greater,
                    (None, None) => core::cmp::Ordering::Equal,
                })
        });

        Ok(Side { total, lines })
    }
}

/// `part` as basis points of `whole`, in integers, rounded down.
///
/// `i128` for the multiply, for the reason `budget` gives: an amount near the ceiling times
/// ten thousand does not fit in `i64`. A part can never exceed its whole here, so the result
/// fits; it is clamped rather than trusted all the same.
fn share_bps(part: Money, whole: Money) -> u16 {
    if whole.minor() <= 0 || part.minor() <= 0 {
        return 0;
    }
    let scaled = i128::from(part.minor()).saturating_mul(i128::from(WHOLE_BPS));
    let share = scaled / i128::from(whole.minor());
    u16::try_from(share).map_or(WHOLE_BPS, |bps| bps.min(WHOLE_BPS))
}
