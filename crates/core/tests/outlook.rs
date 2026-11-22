//! The outlook: the projected walk read forward, its low point, and the first row that runs
//! an account short.

#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

mod common;

use common::{GHS, NOW, account, days_after, id, transaction};
use mt_core::enums::{Direction, TxStatus};
use mt_core::ledger::{BalanceBasis, walk};
use mt_core::model::{Account, Transaction};
use mt_core::money::Money;
use mt_core::outlook::{FULL_BPS, Shortfall, outlook};
use time::Duration;

fn wallet(opening_minor: i64) -> Account {
    Account {
        opening_balance: Money::new(opening_minor, GHS),
        ..account()
    }
}

fn row(
    account: &Account,
    direction: Direction,
    minor: i64,
    days: i64,
    status: TxStatus,
    label: &str,
) -> Transaction {
    Transaction {
        id: id(),
        account_id: account.id,
        direction,
        amount: Money::new(minor, GHS),
        booked_at: days_after(days),
        status,
        label: label.to_owned(),
        ..transaction()
    }
}

const WEEKS: Duration = Duration::days(90);

#[test]
fn the_first_row_to_go_below_zero_is_the_shortfall() {
    let account = wallet(50_000);
    let rows = vec![
        row(
            &account,
            Direction::Out,
            10_000,
            -3,
            TxStatus::Confirmed,
            "Groceries",
        ),
        row(
            &account,
            Direction::Out,
            30_000,
            5,
            TxStatus::Pending,
            "School fees",
        ),
        row(
            &account,
            Direction::Out,
            20_000,
            10,
            TxStatus::Pending,
            "Rent",
        ),
        row(
            &account,
            Direction::In,
            100_000,
            20,
            TxStatus::Pending,
            "Salary",
        ),
    ];
    let walked = walk(&account, &rows, BalanceBasis::Projected).unwrap();

    let ahead = outlook(&walked, NOW, NOW + WEEKS);

    assert_eq!(ahead.start, Money::new(40_000, GHS));
    assert_eq!(ahead.points.len(), 3);
    match ahead.shortfall {
        Shortfall::At(entry) => {
            assert_eq!(entry.transaction.label, "Rent");
            assert_eq!(entry.balance_after, Money::new(-10_000, GHS));
        }
        other => panic!("expected a shortfall at the rent, got {other:?}"),
    }
    assert_eq!(ahead.low, Money::new(-10_000, GHS));
    assert_eq!(ahead.low_at.unwrap().label, "Rent");
    assert_eq!(ahead.high, Money::new(90_000, GHS));
}

#[test]
fn an_account_that_stays_above_zero_has_no_shortfall_and_its_low_may_be_today() {
    let account = wallet(50_000);
    let rows = vec![row(
        &account,
        Direction::In,
        10_000,
        3,
        TxStatus::Pending,
        "Refund",
    )];
    let walked = walk(&account, &rows, BalanceBasis::Projected).unwrap();

    let ahead = outlook(&walked, NOW, NOW + WEEKS);

    assert_eq!(ahead.shortfall, Shortfall::None);
    assert_eq!(ahead.low, Money::new(50_000, GHS));
    assert!(ahead.low_at.is_none(), "the low is the start, not a row");
}

#[test]
fn an_account_already_below_zero_says_so_rather_than_blaming_the_next_row() {
    let account = wallet(1_000);
    let rows = vec![
        // Pending and dated in the past: the projected walk counts it, so today is short.
        row(
            &account,
            Direction::Out,
            5_000,
            -2,
            TxStatus::Pending,
            "Overdue bill",
        ),
        row(
            &account,
            Direction::Out,
            1_000,
            4,
            TxStatus::Pending,
            "Airtime",
        ),
    ];
    let walked = walk(&account, &rows, BalanceBasis::Projected).unwrap();

    let ahead = outlook(&walked, NOW, NOW + WEEKS);

    assert_eq!(ahead.start, Money::new(-4_000, GHS));
    assert_eq!(ahead.shortfall, Shortfall::Already);
}

#[test]
fn rows_past_the_window_are_not_seen() {
    let account = wallet(10_000);
    let rows = vec![row(
        &account,
        Direction::Out,
        50_000,
        200,
        TxStatus::Pending,
        "Far off",
    )];
    let walked = walk(&account, &rows, BalanceBasis::Projected).unwrap();

    let ahead = outlook(&walked, NOW, NOW + WEEKS);

    assert!(ahead.points.is_empty());
    assert_eq!(ahead.shortfall, Shortfall::None);
}

#[test]
fn the_first_of_two_equal_lows_is_the_one_named() {
    let account = wallet(10_000);
    let rows = vec![
        row(
            &account,
            Direction::Out,
            5_000,
            1,
            TxStatus::Pending,
            "First",
        ),
        row(&account, Direction::In, 5_000, 2, TxStatus::Pending, "Back"),
        row(
            &account,
            Direction::Out,
            5_000,
            3,
            TxStatus::Pending,
            "Second",
        ),
    ];
    let walked = walk(&account, &rows, BalanceBasis::Projected).unwrap();

    let ahead = outlook(&walked, NOW, NOW + WEEKS);

    assert_eq!(ahead.low_at.unwrap().label, "First");
}

#[test]
fn levels_span_the_range_and_zero_is_placed_when_the_line_crosses_it() {
    let account = wallet(10_000);
    let rows = vec![
        row(
            &account,
            Direction::Out,
            30_000,
            1,
            TxStatus::Pending,
            "Down",
        ),
        row(&account, Direction::In, 40_000, 2, TxStatus::Pending, "Up"),
    ];
    let walked = walk(&account, &rows, BalanceBasis::Projected).unwrap();

    let ahead = outlook(&walked, NOW, NOW + WEEKS);

    // From −200.00 to 200.00: the low is 0, the high is the whole, zero is halfway.
    assert_eq!(ahead.level_bps(ahead.low), 0);
    assert_eq!(ahead.level_bps(ahead.high), FULL_BPS);
    assert_eq!(ahead.level_bps(ahead.start), 7_500);
    assert_eq!(ahead.zero_level_bps(), Some(5_000));
}

#[test]
fn a_flat_line_sits_in_the_middle_and_zero_is_not_placed_above_it() {
    let account = wallet(10_000);
    let walked = walk(&account, &[], BalanceBasis::Projected).unwrap();

    let ahead = outlook(&walked, NOW, NOW + WEEKS);

    assert_eq!(ahead.level_bps(ahead.start), FULL_BPS / 2);
    assert_eq!(ahead.zero_level_bps(), None);
}
