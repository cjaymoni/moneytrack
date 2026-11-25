//! Month by month: a month here agrees with its breakdown, and every bar shares one round
//! scale worked out in integers.

#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

mod common;

use common::{ACCRA, FAR_EAST, GHS, USD, id, transaction, transfer_pair};
use mt_core::breakdown::{WHOLE_BPS, breakdown};
use mt_core::enums::{Direction, TxStatus};
use mt_core::ledger::BalanceBasis;
use mt_core::model::Transaction;
use mt_core::money::{Currency, Money};
use mt_core::trend::{Scale, months_ending, trend};
use time::macros::{date, datetime};
use time::{Month, OffsetDateTime};

fn row(direction: Direction, minor: i64, at: OffsetDateTime) -> Transaction {
    Transaction {
        id: id(),
        direction,
        amount: Money::new(minor, GHS),
        booked_at: at,
        ..transaction()
    }
}

fn minors(ticks: &[Money]) -> Vec<i64> {
    ticks.iter().map(|tick| tick.minor()).collect()
}

#[test]
fn months_ending_counts_back_across_a_new_year_oldest_first() {
    let periods = months_ending(2027, Month::February, 4).unwrap();
    let starts: Vec<_> = periods.iter().map(|period| period.starts_on).collect();
    assert_eq!(
        starts,
        [
            date!(2026 - 11 - 01),
            date!(2026 - 12 - 01),
            date!(2027 - 01 - 01),
            date!(2027 - 02 - 01)
        ]
    );
    assert_eq!(periods[3].ends_on, date!(2027 - 02 - 28));
    assert_eq!(months_ending(2026, Month::March, 0).unwrap(), []);
}

#[test]
fn each_month_totals_its_own_rows_and_agrees_with_its_breakdown() {
    let periods = months_ending(2026, Month::November, 2).unwrap();
    let rows = vec![
        row(Direction::In, 630_000, datetime!(2026-10-25 09:00 UTC)),
        row(Direction::Out, 120_000, datetime!(2026-10-28 09:00 UTC)),
        row(Direction::Out, 250_000, datetime!(2026-11-02 09:00 UTC)),
        row(Direction::Out, 15_000, datetime!(2026-11-20 09:00 UTC)),
        // Outside both months: not counted anywhere.
        row(Direction::Out, 99_999, datetime!(2026-09-30 09:00 UTC)),
    ];

    let result = trend(GHS, &rows, &periods, ACCRA, BalanceBasis::Confirmed).unwrap();

    let october = result.months[0];
    assert_eq!(
        (
            october.income.minor(),
            october.spending.minor(),
            october.net.minor()
        ),
        (630_000, 120_000, 510_000)
    );
    let november = result.months[1];
    assert_eq!(
        (
            november.income.minor(),
            november.spending.minor(),
            november.net.minor()
        ),
        (0, 265_000, -265_000)
    );
    for (month, period) in result.months.iter().zip(&periods) {
        let same = breakdown(GHS, &rows, &[], *period, ACCRA, BalanceBasis::Confirmed).unwrap();
        assert_eq!(month.spending, same.spending.total);
        assert_eq!(month.income, same.income.total);
        assert_eq!(month.net, same.net);
    }
}

#[test]
fn transfers_deleted_rows_pending_rows_and_other_currencies_are_left_out() {
    let periods = months_ending(2026, Month::November, 1).unwrap();
    let at = datetime!(2026-11-10 09:00 UTC);
    let mut deleted = row(Direction::Out, 1_000, at);
    deleted.sync.deleted_at = Some(at);
    let pending = Transaction {
        status: TxStatus::Pending,
        ..row(Direction::Out, 2_000, at)
    };
    let dollars = Transaction {
        amount: Money::new(4_000, USD),
        ..row(Direction::Out, 0, at)
    };
    let mut rows = vec![deleted, pending, dollars, row(Direction::Out, 500, at)];
    rows.extend(transfer_pair());

    let confirmed = trend(GHS, &rows, &periods, ACCRA, BalanceBasis::Confirmed).unwrap();
    assert_eq!(confirmed.months[0].spending.minor(), 500);

    let projected = trend(GHS, &rows, &periods, ACCRA, BalanceBasis::Projected).unwrap();
    assert_eq!(projected.months[0].spending.minor(), 2_500);
}

#[test]
fn a_row_near_midnight_lands_in_the_month_on_the_readers_clock() {
    let periods = months_ending(2026, Month::November, 2).unwrap();
    // 31 October in UTC, 1 November fourteen hours east.
    let rows = vec![row(Direction::Out, 700, datetime!(2026-10-31 12:00 UTC))];

    let east = trend(GHS, &rows, &periods, FAR_EAST, BalanceBasis::Confirmed).unwrap();
    assert_eq!(east.months[0].spending.minor(), 0);
    assert_eq!(east.months[1].spending.minor(), 700);
}

#[test]
fn every_bar_is_measured_against_one_round_top() {
    let periods = months_ending(2026, Month::November, 2).unwrap();
    let rows = vec![
        row(Direction::In, 630_000, datetime!(2026-10-25 09:00 UTC)),
        row(Direction::Out, 315_000, datetime!(2026-11-02 09:00 UTC)),
    ];

    let result = trend(GHS, &rows, &periods, ACCRA, BalanceBasis::Confirmed).unwrap();

    // 6 300.00 over four gaps is 1 575.00; the round step above it is 2 000.00.
    assert_eq!(result.scale.top.minor(), 800_000);
    assert_eq!(
        minors(&result.scale.ticks),
        [0, 200_000, 400_000, 600_000, 800_000]
    );
    assert_eq!(result.scale.level_bps(result.months[0].income), 7_875);
    assert_eq!(result.scale.level_bps(result.months[1].spending), 3_937);
    assert_eq!(result.scale.level_bps(Money::zero(GHS)), 0);
}

#[test]
fn the_scale_steps_by_one_two_or_five_and_never_below_one_unit() {
    let ghs = |minor| Scale::covering(Money::new(minor, GHS));

    assert_eq!(minors(&ghs(1_000_000).ticks), [0, 500_000, 1_000_000]);
    assert_eq!(minors(&ghs(950_000).ticks), [0, 500_000, 1_000_000]);
    assert_eq!(minors(&ghs(130_000).ticks), [0, 50_000, 100_000, 150_000]);
    // Fifty pesewas: the step is one cedi, not twenty pesewas.
    assert_eq!(minors(&ghs(50).ticks), [0, 100]);
    // A currency with no minor unit steps by one.
    let xof = Scale::covering(Money::new(3, Currency::XOF));
    assert_eq!(minors(&xof.ticks), [0, 1, 2, 3]);
}

#[test]
fn an_empty_scale_is_just_zero_and_puts_everything_there() {
    let scale = Scale::covering(Money::zero(GHS));
    assert_eq!(minors(&scale.ticks), [0]);
    assert_eq!(scale.level_bps(Money::new(5_000, GHS)), 0);

    let none = trend(
        GHS,
        &[],
        &months_ending(2026, Month::November, 3).unwrap(),
        ACCRA,
        BalanceBasis::Confirmed,
    )
    .unwrap();
    assert_eq!(none.months.len(), 3);
    assert!(
        none.months
            .iter()
            .all(|month| month.income.is_zero() && month.spending.is_zero())
    );
    assert_eq!(minors(&none.scale.ticks), [0]);
}

#[test]
fn a_huge_amount_still_gets_a_true_scale() {
    let scale = Scale::covering(Money::new(i64::MAX - 1, GHS));
    assert!(scale.top.minor() >= i64::MAX - 1);
    assert_eq!(scale.level_bps(Money::new(i64::MAX - 1, GHS)), WHOLE_BPS);
    assert_eq!(scale.level_bps(Money::new(-5, GHS)), 0);
}

mod day_by_day {
    use super::*;
    use mt_core::budget::Period;
    use mt_core::trend::daily;

    const NOVEMBER: Period = Period {
        starts_on: date!(2026 - 11 - 01),
        ends_on: date!(2026 - 11 - 30),
    };

    #[test]
    fn every_date_gets_a_day_even_with_nothing_spent() {
        let result = daily(GHS, &[], NOVEMBER, ACCRA, BalanceBasis::Confirmed).unwrap();
        assert_eq!(result.days.len(), 30);
        assert_eq!(result.days[0].date, date!(2026 - 11 - 01));
        assert_eq!(result.days[29].date, date!(2026 - 11 - 30));
        assert!(result.days.iter().all(|day| day.spending.is_zero()));
        assert!(result.total.is_zero());
        assert_eq!(minors(&result.scale.ticks), [0]);
    }

    #[test]
    fn each_day_totals_only_its_spending_and_the_days_add_up_to_the_month() {
        let rows = vec![
            row(Direction::Out, 2_000, datetime!(2026-11-03 08:00 UTC)),
            row(Direction::Out, 500, datetime!(2026-11-03 21:00 UTC)),
            row(Direction::Out, 12_000, datetime!(2026-11-30 09:00 UTC)),
            // Money in is not spending.
            row(Direction::In, 630_000, datetime!(2026-11-03 09:00 UTC)),
            // October.
            row(Direction::Out, 9_999, datetime!(2026-10-31 09:00 UTC)),
        ];

        let result = daily(GHS, &rows, NOVEMBER, ACCRA, BalanceBasis::Confirmed).unwrap();

        assert_eq!(result.days[2].spending.minor(), 2_500);
        assert_eq!(result.days[29].spending.minor(), 12_000);
        assert_eq!(result.total.minor(), 14_500);
        let month = breakdown(GHS, &rows, &[], NOVEMBER, ACCRA, BalanceBasis::Confirmed).unwrap();
        assert_eq!(result.total, month.spending.total);

        // 120.00 over four gaps is 30.00, so the step is 50.00 and the top 150.00.
        assert_eq!(minors(&result.scale.ticks), [0, 5_000, 10_000, 15_000]);
        assert_eq!(result.scale.level_bps(result.days[29].spending), 8_000);
    }

    #[test]
    fn a_late_purchase_lands_on_the_readers_date() {
        // 31 October at noon UTC is already 1 November fourteen hours east.
        let rows = vec![row(Direction::Out, 700, datetime!(2026-10-31 12:00 UTC))];
        let east = daily(GHS, &rows, NOVEMBER, FAR_EAST, BalanceBasis::Confirmed).unwrap();
        assert_eq!(east.days[0].spending.minor(), 700);
        let accra = daily(GHS, &rows, NOVEMBER, ACCRA, BalanceBasis::Confirmed).unwrap();
        assert!(accra.total.is_zero());
    }

    #[test]
    fn transfers_pending_rows_and_other_currencies_are_left_out() {
        let at = datetime!(2026-11-10 09:00 UTC);
        let pending = Transaction {
            status: TxStatus::Pending,
            ..row(Direction::Out, 2_000, at)
        };
        let dollars = Transaction {
            amount: Money::new(4_000, USD),
            ..row(Direction::Out, 0, at)
        };
        let mut rows = vec![pending, dollars, row(Direction::Out, 500, at)];
        rows.extend(transfer_pair());

        let result = daily(GHS, &rows, NOVEMBER, ACCRA, BalanceBasis::Confirmed).unwrap();
        assert_eq!(result.total.minor(), 500);
    }
}
