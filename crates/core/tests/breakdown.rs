//! Where the money went: the breakdown agrees with `flows`, nests one level, and never
//! loses a row it cannot place.

#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

mod common;

use common::{ACCRA, GHS, USD, category, id, transaction, transfer_pair};
use mt_core::breakdown::{WHOLE_BPS, breakdown};
use mt_core::budget::Period;
use mt_core::enums::{CategoryKind, Direction, TxStatus};
use mt_core::ids::CategoryId;
use mt_core::ledger::{BalanceBasis, flows};
use mt_core::model::{Category, Transaction};
use mt_core::money::Money;
use time::macros::{date, datetime};
use time::{OffsetDateTime, UtcOffset};

const NOVEMBER: Period = Period {
    starts_on: date!(2026 - 11 - 01),
    ends_on: date!(2026 - 11 - 30),
};

fn named(name: &str, kind: CategoryKind, sort_order: i32) -> Category {
    Category {
        id: id(),
        name: name.to_owned(),
        kind,
        sort_order,
        ..category()
    }
}

fn under(parent: &Category, name: &str) -> Category {
    Category {
        id: id(),
        name: name.to_owned(),
        parent_id: Some(parent.id),
        kind: parent.kind,
        ..category()
    }
}

fn row(
    direction: Direction,
    minor: i64,
    category: Option<CategoryId>,
    at: OffsetDateTime,
) -> Transaction {
    Transaction {
        id: id(),
        direction,
        amount: Money::new(minor, GHS),
        category_id: category,
        booked_at: at,
        ..transaction()
    }
}

fn spend(minor: i64, category: Option<CategoryId>) -> Transaction {
    row(
        Direction::Out,
        minor,
        category,
        datetime!(2026-11-10 09:00 UTC),
    )
}

#[test]
fn spending_is_grouped_largest_first_and_adds_up_to_flows() {
    let food = named("Food", CategoryKind::Expense, 0);
    let transport = named("Transport", CategoryKind::Expense, 1);
    let rows = vec![
        spend(2_000, Some(transport.id)),
        spend(15_000, Some(food.id)),
        spend(3_000, Some(transport.id)),
        spend(500, None),
    ];

    let result = breakdown(
        GHS,
        &rows,
        &[food.clone(), transport.clone()],
        NOVEMBER,
        ACCRA,
        BalanceBasis::Confirmed,
    )
    .unwrap();

    let lines: Vec<(Option<CategoryId>, i64)> = result
        .spending
        .lines
        .iter()
        .map(|line| (line.category, line.total.minor()))
        .collect();
    assert_eq!(
        lines,
        [
            (Some(food.id), 15_000),
            (Some(transport.id), 5_000),
            (None, 500)
        ]
    );
    let flowed = flows(GHS, &rows, BalanceBasis::Confirmed).unwrap();
    assert_eq!(result.spending.total, flowed.spending);
    assert_eq!(result.net, flowed.net);
}

#[test]
fn a_childs_rows_count_toward_its_parent_and_are_listed_beside_its_own() {
    let transport = named("Transport", CategoryKind::Expense, 0);
    let trotro = under(&transport, "Trotro");
    let fuel = under(&transport, "Fuel");
    let rows = vec![
        spend(1_000, Some(transport.id)),
        spend(4_000, Some(trotro.id)),
        spend(6_000, Some(fuel.id)),
    ];

    let result = breakdown(
        GHS,
        &rows,
        &[transport.clone(), trotro.clone(), fuel.clone()],
        NOVEMBER,
        ACCRA,
        BalanceBasis::Confirmed,
    )
    .unwrap();

    assert_eq!(result.spending.lines.len(), 1);
    let line = &result.spending.lines[0];
    assert_eq!(line.category, Some(transport.id));
    assert_eq!(line.total.minor(), 11_000);
    assert_eq!(line.own.minor(), 1_000);
    let parts: Vec<(CategoryId, i64)> = line
        .children
        .iter()
        .map(|part| (part.category, part.total.minor()))
        .collect();
    assert_eq!(parts, [(fuel.id, 6_000), (trotro.id, 4_000)]);
}

#[test]
fn a_row_under_a_category_it_does_not_know_is_uncategorised_not_lost() {
    let gone = named("Gone", CategoryKind::Expense, 0);
    let mut deleted = named("Deleted", CategoryKind::Expense, 1);
    deleted.sync.deleted_at = Some(datetime!(2026-11-01 00:00 UTC));
    let rows = vec![spend(700, Some(gone.id)), spend(300, Some(deleted.id))];

    // `gone` is not handed in at all; `deleted` is, as a tombstone.
    let result = breakdown(
        GHS,
        &rows,
        &[deleted],
        NOVEMBER,
        ACCRA,
        BalanceBasis::Confirmed,
    )
    .unwrap();

    assert_eq!(result.spending.lines.len(), 1);
    assert_eq!(result.spending.lines[0].category, None);
    assert_eq!(result.spending.total.minor(), 1_000);
}

#[test]
fn a_child_whose_parent_is_gone_stands_on_its_own() {
    let transport = named("Transport", CategoryKind::Expense, 0);
    let trotro = under(&transport, "Trotro");
    let rows = vec![spend(400, Some(trotro.id))];

    let result = breakdown(
        GHS,
        &rows,
        std::slice::from_ref(&trotro),
        NOVEMBER,
        ACCRA,
        BalanceBasis::Confirmed,
    )
    .unwrap();

    let line = &result.spending.lines[0];
    assert_eq!(line.category, Some(trotro.id));
    assert_eq!(line.own.minor(), 400);
    assert!(line.children.is_empty());
}

#[test]
fn direction_decides_the_side_not_the_categorys_kind() {
    let food = named("Food", CategoryKind::Expense, 0);
    let refund = row(
        Direction::In,
        2_500,
        Some(food.id),
        datetime!(2026-11-12 09:00 UTC),
    );

    let result = breakdown(
        GHS,
        &[spend(10_000, Some(food.id)), refund],
        std::slice::from_ref(&food),
        NOVEMBER,
        ACCRA,
        BalanceBasis::Confirmed,
    )
    .unwrap();

    assert_eq!(result.spending.total.minor(), 10_000);
    assert_eq!(result.income.total.minor(), 2_500);
    assert_eq!(result.income.lines[0].category, Some(food.id));
    assert_eq!(result.net.minor(), -7_500);
}

#[test]
fn transfers_other_currencies_pending_and_deleted_rows_are_left_out() {
    let [out_leg, in_leg] = transfer_pair();
    let dollars = Transaction {
        amount: Money::new(9_999, USD),
        ..spend(0, None)
    };
    let pending = Transaction {
        status: TxStatus::Pending,
        ..spend(4_000, None)
    };
    let mut deleted = spend(5_000, None);
    deleted.sync.deleted_at = Some(datetime!(2026-11-11 00:00 UTC));
    let kept = spend(100, None);
    let rows = vec![out_leg, in_leg, dollars, pending.clone(), deleted, kept];

    let confirmed = breakdown(GHS, &rows, &[], NOVEMBER, ACCRA, BalanceBasis::Confirmed).unwrap();
    assert_eq!(confirmed.spending.total.minor(), 100);
    assert_eq!(confirmed.income.total.minor(), 0);

    // The projected basis lets the pending row in, and nothing else changes.
    let projected = breakdown(GHS, &rows, &[], NOVEMBER, ACCRA, BalanceBasis::Projected).unwrap();
    assert_eq!(projected.spending.total.minor(), 4_100);
}

#[test]
fn the_period_is_read_in_the_persons_own_zone() {
    // 23:30 on 31 October in a zone three hours behind UTC is 02:30 on 1 November in UTC.
    // It belongs to October, because that is the date on the person's own clock.
    let behind = UtcOffset::from_hms(-3, 0, 0).unwrap();
    let late = row(Direction::Out, 1_000, None, datetime!(2026-11-01 02:30 UTC));

    let local = breakdown(
        GHS,
        std::slice::from_ref(&late),
        &[],
        NOVEMBER,
        behind,
        BalanceBasis::Confirmed,
    )
    .unwrap();
    assert_eq!(local.spending.total.minor(), 0);

    let utc = breakdown(
        GHS,
        std::slice::from_ref(&late),
        &[],
        NOVEMBER,
        ACCRA,
        BalanceBasis::Confirmed,
    )
    .unwrap();
    assert_eq!(utc.spending.total.minor(), 1_000);
}

#[test]
fn shares_are_basis_points_of_the_side_and_never_past_the_whole() {
    let food = named("Food", CategoryKind::Expense, 0);
    let transport = named("Transport", CategoryKind::Expense, 1);
    let trotro = under(&transport, "Trotro");
    let rows = vec![spend(2_000, Some(food.id)), spend(1_000, Some(trotro.id))];

    let result = breakdown(
        GHS,
        &rows,
        &[food, transport, trotro],
        NOVEMBER,
        ACCRA,
        BalanceBasis::Confirmed,
    )
    .unwrap();

    let lines = &result.spending.lines;
    // Rounded down: two thirds is 6 666, one third is 3 333.
    assert_eq!(lines[0].share_bps, 6_666);
    assert_eq!(lines[1].share_bps, 3_333);
    // A part is on the same scale as a line.
    assert_eq!(lines[1].children[0].share_bps, 3_333);

    let alone = breakdown(
        GHS,
        &[spend(i64::from(i32::MAX), None)],
        &[],
        NOVEMBER,
        ACCRA,
        BalanceBasis::Confirmed,
    )
    .unwrap();
    assert_eq!(alone.spending.lines[0].share_bps, WHOLE_BPS);
}

#[test]
fn an_empty_period_is_two_empty_sides() {
    let result = breakdown(GHS, &[], &[], NOVEMBER, ACCRA, BalanceBasis::Confirmed).unwrap();

    assert!(result.spending.lines.is_empty());
    assert!(result.income.lines.is_empty());
    assert!(result.net.is_zero());
}

#[test]
fn equal_totals_keep_the_categories_own_order_and_put_uncategorised_last() {
    let second = named("Second", CategoryKind::Expense, 2);
    let first = named("First", CategoryKind::Expense, 1);
    let rows = vec![
        spend(500, None),
        spend(500, Some(second.id)),
        spend(500, Some(first.id)),
    ];

    let result = breakdown(
        GHS,
        &rows,
        &[second.clone(), first.clone()],
        NOVEMBER,
        ACCRA,
        BalanceBasis::Confirmed,
    )
    .unwrap();

    let order: Vec<Option<CategoryId>> = result
        .spending
        .lines
        .iter()
        .map(|line| line.category)
        .collect();
    assert_eq!(order, [Some(first.id), Some(second.id), None]);
}
