//! The outlook: where an account is headed, and the first day it runs short.
//!
//! It is the projected walk read forward from now. The walk already admits the rows still
//! expected and orders them with its tiebreak, so the outlook adds no arithmetic of its own to
//! the balance — only which part of the walk to look at, where it is lowest, and the first row
//! that takes it below zero.
//!
//! ## Short already
//!
//! An account can be below zero before any expected row lands: an overdraft, or a pending row
//! dated in the past that has not been confirmed. That is said separately rather than reported
//! as a shortfall "now" against whichever row happened to do it, because the remedy is
//! different — something has already gone wrong, rather than something is about to.
//!
//! ## Levels
//!
//! A screen drawing the line needs each balance as a height. Working that out is the balance
//! minus the low, divided by the range — arithmetic on money, which the screen is not allowed
//! to do — so every point carries its height here, in basis points of the range, along with
//! where zero sits when the line crosses it.

use time::OffsetDateTime;

use crate::ledger::{Entry, Walk};
use crate::model::Transaction;
use crate::money::Money;

/// Ten thousand basis points span the range from the lowest balance to the highest.
pub const FULL_BPS: u16 = 10_000;

/// Whether, and when, an account runs out over the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortfall<'a> {
    /// It stays at or above zero throughout.
    None,
    /// It is already below zero at the start of the window.
    Already,
    /// This row is the first to take it below zero.
    At(Entry<'a>),
}

/// One account's balance from now to the end of the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outlook<'a> {
    /// The start of the window.
    pub from: OffsetDateTime,
    /// The end of the window, inclusive.
    pub until: OffsetDateTime,
    /// What the account stands at at `from`, counting every row booked up to then — pending
    /// rows dated in the past included, since the walk is projected.
    pub start: Money,
    /// Every row booked after `from` and up to `until`, with the balance after each.
    pub points: Vec<Entry<'a>>,
    /// The lowest balance over the window, `start` included.
    pub low: Money,
    /// The row that brings the balance to `low`, or `None` when the low is `start` itself.
    /// The first such row when the low is reached more than once.
    pub low_at: Option<&'a Transaction>,
    /// The highest balance over the window, `start` included.
    pub high: Money,
    /// Whether and when it runs out.
    pub shortfall: Shortfall<'a>,
}

/// Reads a walk forward from `from` to `until`.
///
/// Meant for a [`crate::ledger::BalanceBasis::Projected`] walk: on a confirmed one there is
/// nothing ahead to see, and the outlook is a flat line at today's balance.
#[must_use]
pub fn outlook<'a>(walk: &Walk<'a>, from: OffsetDateTime, until: OffsetDateTime) -> Outlook<'a> {
    let start = walk.balance_as_of(from);
    let points: Vec<Entry<'a>> = walk
        .entries
        .iter()
        .filter(|entry| entry.transaction.booked_at > from && entry.transaction.booked_at <= until)
        .copied()
        .collect();

    let mut low = start;
    let mut low_at = None;
    let mut high = start;
    for entry in &points {
        if entry.balance_after.minor() < low.minor() {
            low = entry.balance_after;
            low_at = Some(entry.transaction);
        }
        if entry.balance_after.minor() > high.minor() {
            high = entry.balance_after;
        }
    }

    let shortfall = if start.is_negative() {
        Shortfall::Already
    } else {
        points
            .iter()
            .find(|entry| entry.balance_after.is_negative())
            .copied()
            .map_or(Shortfall::None, Shortfall::At)
    };

    Outlook {
        from,
        until,
        start,
        points,
        low,
        low_at,
        high,
        shortfall,
    }
}

impl Outlook<'_> {
    /// Where `balance` sits between the low and the high, in basis points: 0 is the low,
    /// [`FULL_BPS`] the high.
    ///
    /// A flat window — no rows ahead, or rows that cancel out — has no range to place
    /// anything in, and every balance in it sits at the middle.
    #[must_use]
    pub fn level_bps(&self, balance: Money) -> u16 {
        let range = i128::from(self.high.minor()) - i128::from(self.low.minor());
        if range <= 0 {
            return FULL_BPS / 2;
        }
        let above = (i128::from(balance.minor()) - i128::from(self.low.minor())).max(0);
        let level = above.saturating_mul(i128::from(FULL_BPS)) / range;
        u16::try_from(level).map_or(FULL_BPS, |bps| bps.min(FULL_BPS))
    }

    /// Where zero sits on the same scale, when the line reaches or crosses it.
    #[must_use]
    pub fn zero_level_bps(&self) -> Option<u16> {
        if self.low.minor() <= 0 && self.high.minor() >= 0 {
            Some(self.level_bps(Money::zero(self.low.currency())))
        } else {
            None
        }
    }
}
