//! Budgets: which period, which rows count, and where the line falls.

// Sealed like the library it tests: an integration test is its own crate root, and the
// workspace only denies unsafe, which an `allow` in this file would switch off.
#![forbid(unsafe_code)]

#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

mod common;

use common::{
    ACCRA, FAR_EAST, FAR_WEST, GHS, Shifting, USD, budget, category, id, midnight, minus_ten,
    recurring, transaction,
};
use mt_core::budget::{
    BudgetError, CategoryTree, Level, Status, evaluate, evaluate_all, period_bounds,
    period_containing,
};
use mt_core::enums::{BudgetPeriod, Direction, RecurrenceFreq, TxStatus};
use mt_core::ids::CategoryId;
use mt_core::ids::TxId;
use mt_core::ledger::BalanceBasis;
use mt_core::model::{Budget, Category, RecurringRule, Transaction};
use mt_core::money::Money;
use mt_core::recurrence::generate;
use time::macros::{date, datetime};
use time::{Date, Duration};
use uuid::Uuid;

/// A monthly cedi cap on one category, warning at 80%.
fn monthly_cap(category_id: Option<CategoryId>, cap_minor: i64) -> Budget {
    Budget {
        category_id,
        period: BudgetPeriod::Monthly,
        cap: Money::new(cap_minor, GHS),
        starts_on: date!(2026 - 01 - 01),
        ends_on: None,
        warn_at_bps: 8_000,
        ..budget()
    }
}

/// A confirmed outgoing row on a local date.
fn spend(category_id: Option<CategoryId>, minor: i64, on: Date) -> Transaction {
    Transaction {
        id: id(),
        direction: Direction::Out,
        amount: Money::new(minor, GHS),
        booked_at: midnight(on),
        category_id,
        status: TxStatus::Confirmed,
        ..transaction()
    }
}

/// Evaluates a budget against rows on a date, with no category nesting in play.
fn status_on(budget: &Budget, rows: &[Transaction], on: Date) -> Option<Status> {
    evaluate(
        budget,
        &CategoryTree::default(),
        rows,
        on,
        BalanceBasis::Confirmed,
        ACCRA,
    )
    .unwrap()
}

// ------------------------------------------------------------------------------ periods ---

#[test]
fn a_monthly_budget_counts_the_calendar_month_the_date_falls_in() {
    let budget = monthly_cap(None, 50_000);

    let period = period_containing(&budget, date!(2026 - 03 - 17)).unwrap();

    assert_eq!(period.starts_on, date!(2026 - 03 - 01));
    assert_eq!(period.ends_on, date!(2026 - 03 - 31));
}

#[test]
fn february_gets_the_right_number_of_days_in_a_leap_year_and_out_of_one() {
    let budget = monthly_cap(None, 50_000);

    assert_eq!(
        period_containing(&budget, date!(2026 - 02 - 10))
            .unwrap()
            .ends_on,
        date!(2026 - 02 - 28)
    );
    assert_eq!(
        period_containing(&budget, date!(2028 - 02 - 10))
            .unwrap()
            .ends_on,
        date!(2028 - 02 - 29)
    );
}

#[test]
fn a_weekly_budget_counts_seven_days_from_the_day_it_started() {
    let budget = Budget {
        period: BudgetPeriod::Weekly,
        starts_on: date!(2026 - 03 - 04),
        ..monthly_cap(None, 20_000)
    };

    let first = period_containing(&budget, date!(2026 - 03 - 06)).unwrap();
    let third = period_containing(&budget, date!(2026 - 03 - 19)).unwrap();

    assert_eq!(first.starts_on, date!(2026 - 03 - 04));
    assert_eq!(first.ends_on, date!(2026 - 03 - 10));
    assert_eq!(third.starts_on, date!(2026 - 03 - 18));
    assert_eq!(third.ends_on, date!(2026 - 03 - 24));
}

#[test]
fn a_yearly_budget_counts_the_calendar_year() {
    let budget = Budget {
        period: BudgetPeriod::Yearly,
        ..monthly_cap(None, 500_000)
    };

    let period = period_containing(&budget, date!(2026 - 07 - 04)).unwrap();

    assert_eq!(period.starts_on, date!(2026 - 01 - 01));
    assert_eq!(period.ends_on, date!(2026 - 12 - 31));
}

#[test]
fn the_first_period_starts_when_the_budget_does_not_when_the_month_does() {
    // A budget created on the 15th must not count spending from the 1st, which happened
    // before there was a budget to be over.
    let budget = Budget {
        starts_on: date!(2026 - 03 - 15),
        ..monthly_cap(None, 50_000)
    };

    let period = period_containing(&budget, date!(2026 - 03 - 20)).unwrap();

    assert_eq!(period.starts_on, date!(2026 - 03 - 15));
    assert_eq!(period.ends_on, date!(2026 - 03 - 31));
}

#[test]
fn the_last_period_ends_when_the_budget_does() {
    let budget = Budget {
        ends_on: Some(date!(2026 - 03 - 10)),
        ..monthly_cap(None, 50_000)
    };

    let period = period_containing(&budget, date!(2026 - 03 - 05)).unwrap();

    assert_eq!(period.ends_on, date!(2026 - 03 - 10));
}

#[test]
fn a_budget_is_dormant_outside_its_own_lifetime() {
    let budget = Budget {
        starts_on: date!(2026 - 02 - 01),
        ends_on: Some(date!(2026 - 04 - 30)),
        ..monthly_cap(None, 50_000)
    };

    assert_eq!(period_containing(&budget, date!(2026 - 01 - 31)), None);
    assert_eq!(period_containing(&budget, date!(2026 - 05 - 01)), None);
    assert!(period_containing(&budget, date!(2026 - 03 - 01)).is_some());
}

#[test]
fn a_dormant_budget_is_not_an_error_it_is_simply_no_answer() {
    let budget = Budget {
        starts_on: date!(2026 - 06 - 01),
        ..monthly_cap(None, 50_000)
    };

    assert_eq!(status_on(&budget, &[], date!(2026 - 01 - 15)), None);
}

#[test]
fn a_period_turns_into_the_instants_a_store_can_be_queried_with() {
    let budget = monthly_cap(None, 50_000);
    let period = period_containing(&budget, date!(2026 - 03 - 17)).unwrap();

    let (from, to) = period_bounds(&period, ACCRA);

    assert_eq!(from, midnight(date!(2026 - 03 - 01)));
    assert_eq!(to.date(), date!(2026 - 03 - 31));
    // Inclusive at the far end, so a purchase at 23:59 is inside the month it looks inside.
    assert!(to > midnight(date!(2026 - 03 - 31)));
}

// --------------------------------------------------------------------------- what counts ---

#[test]
fn spending_inside_the_period_counts_and_spending_outside_it_does_not() {
    let budget = monthly_cap(None, 50_000);
    let rows = vec![
        spend(None, 10_000, date!(2026 - 02 - 28)),
        spend(None, 12_000, date!(2026 - 03 - 01)),
        spend(None, 3_000, date!(2026 - 03 - 31)),
        spend(None, 99_000, date!(2026 - 04 - 01)),
    ];

    let status = status_on(&budget, &rows, date!(2026 - 03 - 15)).unwrap();

    assert_eq!(status.spent, Money::new(15_000, GHS));
    assert_eq!(status.remaining, Money::new(35_000, GHS));
}

#[test]
fn money_coming_in_is_not_spending() {
    let budget = monthly_cap(None, 50_000);
    let rows = vec![Transaction {
        direction: Direction::In,
        ..spend(None, 40_000, date!(2026 - 03 - 10))
    }];

    let status = status_on(&budget, &rows, date!(2026 - 03 - 15)).unwrap();

    assert_eq!(status.spent, Money::new(0, GHS));
}

#[test]
fn moving_money_between_your_own_accounts_does_not_eat_a_budget() {
    let budget = monthly_cap(None, 50_000);
    let rows = vec![Transaction {
        transfer_id: Some(id()),
        category_id: None,
        ..spend(None, 40_000, date!(2026 - 03 - 10))
    }];

    let status = status_on(&budget, &rows, date!(2026 - 03 - 15)).unwrap();

    assert_eq!(status.spent, Money::new(0, GHS));
}

#[test]
fn a_cap_in_one_currency_ignores_spending_in_another_rather_than_converting_it() {
    // Converting would make last month's verdict move when the rate does.
    let budget = monthly_cap(None, 50_000);
    let rows = vec![Transaction {
        amount: Money::new(40_000, USD),
        ..spend(None, 40_000, date!(2026 - 03 - 10))
    }];

    let status = status_on(&budget, &rows, date!(2026 - 03 - 15)).unwrap();

    assert_eq!(status.spent, Money::new(0, GHS));
    assert_eq!(status.spent.currency(), GHS);
}

#[test]
fn a_deleted_row_does_not_count_against_a_cap() {
    let budget = monthly_cap(None, 50_000);
    let mut deleted = spend(None, 40_000, date!(2026 - 03 - 10));
    deleted.sync.deleted_at = Some(midnight(date!(2026 - 03 - 11)));

    let status = status_on(&budget, &[deleted], date!(2026 - 03 - 15)).unwrap();

    assert_eq!(status.spent, Money::new(0, GHS));
}

#[test]
fn a_pending_row_counts_only_against_a_projected_basis() {
    let budget = monthly_cap(None, 50_000);
    let rows = vec![Transaction {
        status: TxStatus::Pending,
        ..spend(None, 40_000, date!(2026 - 03 - 20))
    }];

    let settled = status_on(&budget, &rows, date!(2026 - 03 - 15)).unwrap();
    let forecast = evaluate(
        &budget,
        &CategoryTree::default(),
        &rows,
        date!(2026 - 03 - 15),
        BalanceBasis::Projected,
        ACCRA,
    )
    .unwrap()
    .unwrap();

    assert_eq!(settled.spent, Money::new(0, GHS));
    assert_eq!(forecast.spent, Money::new(40_000, GHS));
}

// -------------------------------------------------------------------------- category scope ---

#[test]
fn a_cap_on_a_category_counts_only_that_category() {
    let transport: CategoryId = id();
    let food: CategoryId = id();
    let budget = monthly_cap(Some(transport), 50_000);
    let rows = vec![
        spend(Some(transport), 10_000, date!(2026 - 03 - 05)),
        spend(Some(food), 30_000, date!(2026 - 03 - 05)),
        spend(None, 7_000, date!(2026 - 03 - 05)),
    ];

    let status = status_on(&budget, &rows, date!(2026 - 03 - 15)).unwrap();

    assert_eq!(status.spent, Money::new(10_000, GHS));
}

#[test]
fn a_cap_on_a_parent_category_covers_the_categories_nested_under_it() {
    let transport = category();
    let trotro = Category {
        id: id(),
        name: "Trotro".to_owned(),
        parent_id: Some(transport.id),
        ..category()
    };
    let budget = monthly_cap(Some(transport.id), 50_000);
    let tree = CategoryTree::build(&[transport.clone(), trotro.clone()]);
    let rows = vec![
        spend(Some(transport.id), 4_000, date!(2026 - 03 - 05)),
        spend(Some(trotro.id), 6_000, date!(2026 - 03 - 06)),
    ];

    let status = evaluate(
        &budget,
        &tree,
        &rows,
        date!(2026 - 03 - 15),
        BalanceBasis::Confirmed,
        ACCRA,
    )
    .unwrap()
    .unwrap();

    assert_eq!(status.spent, Money::new(10_000, GHS));
}

#[test]
fn a_cap_with_no_category_covers_every_outgoing_row_in_its_currency() {
    let budget = monthly_cap(None, 50_000);
    let rows = vec![
        spend(Some(id()), 10_000, date!(2026 - 03 - 05)),
        spend(None, 7_000, date!(2026 - 03 - 06)),
    ];

    let status = status_on(&budget, &rows, date!(2026 - 03 - 15)).unwrap();

    assert_eq!(status.spent, Money::new(17_000, GHS));
}

// ---------------------------------------------------------------------------- the verdict ---

#[test]
fn a_budget_barely_touched_is_under() {
    let budget = monthly_cap(None, 50_000);
    let rows = vec![spend(None, 1_000, date!(2026 - 03 - 05))];

    let status = status_on(&budget, &rows, date!(2026 - 03 - 15)).unwrap();

    assert_eq!(status.level, Level::Under);
    assert_eq!(status.used_bps, 200);
    assert!(!status.is_over());
}

#[test]
fn the_warning_fires_at_the_fraction_the_budget_asks_for() {
    let budget = monthly_cap(None, 50_000);

    let just_below = status_on(
        &budget,
        &[spend(None, 39_999, date!(2026 - 03 - 05))],
        date!(2026 - 03 - 15),
    )
    .unwrap();
    let exactly_at = status_on(
        &budget,
        &[spend(None, 40_000, date!(2026 - 03 - 05))],
        date!(2026 - 03 - 15),
    )
    .unwrap();

    assert_eq!(just_below.level, Level::Under);
    assert_eq!(exactly_at.level, Level::Warning);
    assert_eq!(exactly_at.used_bps, 8_000);
}

#[test]
fn spending_the_whole_cap_counts_as_over_not_as_just_inside() {
    // The cap is what there is to spend. Spending all of it leaves nothing.
    let budget = monthly_cap(None, 50_000);
    let rows = vec![spend(None, 50_000, date!(2026 - 03 - 05))];

    let status = status_on(&budget, &rows, date!(2026 - 03 - 15)).unwrap();

    assert_eq!(status.level, Level::Over);
    assert!(status.is_over());
    assert_eq!(status.remaining, Money::new(0, GHS));
}

#[test]
fn going_past_the_cap_leaves_a_negative_remainder() {
    let budget = monthly_cap(None, 50_000);
    let rows = vec![spend(None, 62_500, date!(2026 - 03 - 05))];

    let status = status_on(&budget, &rows, date!(2026 - 03 - 15)).unwrap();

    assert_eq!(status.level, Level::Over);
    assert_eq!(status.remaining, Money::new(-12_500, GHS));
    assert_eq!(status.used_bps, 12_500);
}

#[test]
fn spending_far_past_the_cap_is_reported_as_far_past_rather_than_wrapping_round() {
    let budget = monthly_cap(None, 100);
    let rows = vec![spend(None, 900_000_000, date!(2026 - 03 - 05))];

    let status = status_on(&budget, &rows, date!(2026 - 03 - 15)).unwrap();

    assert_eq!(status.level, Level::Over);
    assert_eq!(status.used_bps, u16::MAX);
}

#[test]
fn an_untouched_budget_reports_the_whole_cap_as_remaining() {
    let budget = monthly_cap(None, 50_000);

    let status = status_on(&budget, &[], date!(2026 - 03 - 15)).unwrap();

    assert_eq!(status.spent, Money::new(0, GHS));
    assert_eq!(status.remaining, Money::new(50_000, GHS));
    assert_eq!(status.used_bps, 0);
    assert_eq!(status.level, Level::Under);
}

// ------------------------------------------------------------------------------- the set ---

#[test]
fn every_running_budget_is_evaluated_in_one_go() {
    let transport: CategoryId = id();
    let food: CategoryId = id();
    let budgets = vec![
        monthly_cap(Some(transport), 20_000),
        monthly_cap(Some(food), 30_000),
        Budget {
            starts_on: date!(2027 - 01 - 01),
            ..monthly_cap(None, 1_000)
        },
    ];
    let rows = vec![
        spend(Some(transport), 21_000, date!(2026 - 03 - 05)),
        spend(Some(food), 5_000, date!(2026 - 03 - 06)),
    ];

    let statuses = evaluate_all(
        &budgets,
        &[],
        &rows,
        date!(2026 - 03 - 15),
        BalanceBasis::Confirmed,
        ACCRA,
    )
    .unwrap();

    // The third budget has not started yet, so it is simply absent.
    assert_eq!(statuses.len(), 2);
    assert_eq!(statuses[0].level, Level::Over);
    assert_eq!(statuses[1].level, Level::Under);
}

#[test]
fn a_deleted_budget_is_not_evaluated() {
    let mut deleted = monthly_cap(None, 50_000);
    deleted.sync.deleted_at = Some(midnight(date!(2026 - 03 - 01)));

    let statuses = evaluate_all(
        &[deleted],
        &[],
        &[],
        date!(2026 - 03 - 15),
        BalanceBasis::Confirmed,
        ACCRA,
    )
    .unwrap();

    assert!(statuses.is_empty());
}

#[test]
fn a_spending_total_that_leaves_i64_names_the_budget_it_broke() {
    let budget = monthly_cap(None, 50_000);
    let rows = vec![
        spend(None, i64::MAX, date!(2026 - 03 - 05)),
        spend(None, i64::MAX, date!(2026 - 03 - 06)),
    ];

    match evaluate(
        &budget,
        &CategoryTree::default(),
        &rows,
        date!(2026 - 03 - 15),
        BalanceBasis::Confirmed,
        ACCRA,
    ) {
        Err(BudgetError::Arithmetic { budget: named, .. }) => assert_eq!(named, budget.id),
        other => panic!("expected an arithmetic error naming the budget, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------- time zones ---

#[test]
fn a_purchase_is_counted_in_the_month_the_user_was_living_in_not_the_utc_one() {
    // 10:30 UTC on 1 November is still 23:30 on 31 October eleven hours west. Counting it in
    // November would put it against a cap the user had not started spending yet.
    let budget = monthly_cap(None, 50_000);
    let row = Transaction {
        booked_at: datetime!(2026-11-01 10:30:00 UTC),
        ..spend(None, 12_000, date!(2026 - 01 - 01))
    };

    let october = evaluate(
        &budget,
        &CategoryTree::default(),
        std::slice::from_ref(&row),
        date!(2026 - 10 - 15),
        BalanceBasis::Confirmed,
        FAR_WEST,
    )
    .unwrap()
    .unwrap();
    let november = evaluate(
        &budget,
        &CategoryTree::default(),
        &[row],
        date!(2026 - 11 - 15),
        BalanceBasis::Confirmed,
        FAR_WEST,
    )
    .unwrap()
    .unwrap();

    assert_eq!(october.spent, Money::new(12_000, GHS));
    assert_eq!(november.spent, Money::new(0, GHS));
}

#[test]
fn the_same_holds_fourteen_hours_the_other_way() {
    // 15:00 UTC on 31 October is already 05:00 on 1 November fourteen hours east.
    let budget = monthly_cap(None, 50_000);
    let row = Transaction {
        booked_at: datetime!(2026-10-31 15:00:00 UTC),
        ..spend(None, 12_000, date!(2026 - 01 - 01))
    };

    let october = evaluate(
        &budget,
        &CategoryTree::default(),
        std::slice::from_ref(&row),
        date!(2026 - 10 - 15),
        BalanceBasis::Confirmed,
        FAR_EAST,
    )
    .unwrap()
    .unwrap();
    let november = evaluate(
        &budget,
        &CategoryTree::default(),
        &[row],
        date!(2026 - 11 - 15),
        BalanceBasis::Confirmed,
        FAR_EAST,
    )
    .unwrap()
    .unwrap();

    assert_eq!(october.spent, Money::new(0, GHS));
    assert_eq!(november.spent, Money::new(12_000, GHS));
}

#[test]
fn a_zone_that_shifts_on_the_first_of_the_month_still_counts_the_row_in_the_right_month() {
    // The case that a single date-keyed lookup gets wrong. The zone moves from -11 to -10 on
    // 1 November. A purchase at 10:30 UTC that day is locally 31 October 23:30 on the old
    // offset. Resolving the offset from the *UTC* date hands back -10 and dates the row 1
    // November, moving it into the wrong month with nothing on screen to explain it.
    let budget = monthly_cap(None, 50_000);
    let zone = Shifting::at_midnight(FAR_WEST, minus_ten(), date!(2026 - 11 - 01));
    let row = Transaction {
        booked_at: datetime!(2026-11-01 10:30:00 UTC),
        ..spend(None, 12_000, date!(2026 - 01 - 01))
    };

    let october = evaluate(
        &budget,
        &CategoryTree::default(),
        std::slice::from_ref(&row),
        date!(2026 - 10 - 15),
        BalanceBasis::Confirmed,
        zone,
    )
    .unwrap()
    .unwrap();
    let november = evaluate(
        &budget,
        &CategoryTree::default(),
        &[row],
        date!(2026 - 11 - 15),
        BalanceBasis::Confirmed,
        zone,
    )
    .unwrap()
    .unwrap();

    assert_eq!(october.spent, Money::new(12_000, GHS));
    assert_eq!(november.spent, Money::new(0, GHS));
}

#[test]
fn a_period_turns_into_instants_in_the_users_own_zone_not_in_utc() {
    let budget = monthly_cap(None, 50_000);
    let period = period_containing(&budget, date!(2026 - 03 - 17)).unwrap();

    let (from, to) = period_bounds(&period, FAR_WEST);

    assert_eq!(from, datetime!(2026-03-01 11:00:00 UTC));
    assert!(to > datetime!(2026-03-31 11:00:00 UTC));
}

// --------------------------------------------------------------------- refusing bad input ---

#[test]
fn a_budget_with_a_cap_of_nothing_is_refused_rather_than_reported_as_blown() {
    // Without this, spending nothing against a zero cap evaluates `0 >= 0` to Over and the
    // user is told they have blown a budget that does not exist. A corrupt row arriving over
    // sync has to fail loudly.
    let broken = Budget {
        cap: Money::new(0, GHS),
        ..monthly_cap(None, 50_000)
    };

    assert!(matches!(
        evaluate(
            &broken,
            &CategoryTree::default(),
            &[],
            date!(2026 - 03 - 15),
            BalanceBasis::Confirmed,
            ACCRA,
        ),
        Err(BudgetError::Invalid { budget, .. }) if budget == broken.id
    ));
}

#[test]
fn a_clock_skew_from_another_device_does_not_take_every_other_cap_off_the_screen() {
    // `Budget::validate` would refuse this row for its sync stamps. Evaluation does not care
    // about them: the cap is usable, and one device's skewed clock must not blank a user's
    // whole budgets screen.
    let mut skewed = monthly_cap(None, 50_000);
    skewed.sync.updated_at = skewed.sync.created_at - Duration::days(1);
    let healthy = monthly_cap(Some(id()), 30_000);

    let statuses = evaluate_all(
        &[skewed, healthy],
        &[],
        &[],
        date!(2026 - 03 - 15),
        BalanceBasis::Confirmed,
        ACCRA,
    )
    .unwrap();

    assert_eq!(statuses.len(), 2);
}

#[test]
fn a_budget_whose_dates_run_backwards_is_refused_by_name() {
    let backwards = Budget {
        starts_on: date!(2026 - 06 - 01),
        ends_on: Some(date!(2026 - 01 - 01)),
        ..monthly_cap(None, 50_000)
    };

    assert!(matches!(
        evaluate_all(
            std::slice::from_ref(&backwards),
            &[],
            &[],
            date!(2026 - 03 - 15),
            BalanceBasis::Confirmed,
            ACCRA,
        ),
        Err(BudgetError::Invalid { budget, .. }) if budget == backwards.id
    ));
}

#[test]
fn a_warning_threshold_of_zero_means_do_not_warn_rather_than_warn_at_once() {
    let budget = Budget {
        warn_at_bps: 0,
        ..monthly_cap(None, 50_000)
    };

    let untouched = status_on(&budget, &[], date!(2026 - 03 - 15)).unwrap();
    let halfway = status_on(
        &budget,
        &[spend(None, 25_000, date!(2026 - 03 - 05))],
        date!(2026 - 03 - 15),
    )
    .unwrap();

    assert_eq!(untouched.level, Level::Under);
    assert_eq!(halfway.level, Level::Under);
}

#[test]
fn the_edges_of_a_weekly_block_fall_where_they_should() {
    let budget = Budget {
        period: BudgetPeriod::Weekly,
        starts_on: date!(2026 - 03 - 04),
        ..monthly_cap(None, 20_000)
    };

    let anchor = period_containing(&budget, date!(2026 - 03 - 04)).unwrap();
    let last_day = period_containing(&budget, date!(2026 - 03 - 10)).unwrap();
    let next_block = period_containing(&budget, date!(2026 - 03 - 11)).unwrap();

    assert_eq!(anchor.starts_on, date!(2026 - 03 - 04));
    assert_eq!(last_day.starts_on, date!(2026 - 03 - 04));
    assert_eq!(last_day.ends_on, date!(2026 - 03 - 10));
    assert_eq!(next_block.starts_on, date!(2026 - 03 - 11));
    assert_eq!(next_block.ends_on, date!(2026 - 03 - 17));
}

#[test]
fn a_bill_generated_for_the_first_of_the_month_is_counted_in_that_month() {
    // The two modules have to agree, and nothing else in the suite makes them. `recurrence`
    // turns a local date into an instant; `budget` turns that instant back into a local
    // date. Under a zone that jumps over midnight these disagreed, so November's rent was
    // generated for 1 November and counted against October's cap.
    let zone = Shifting::at_midnight(FAR_WEST, minus_ten(), date!(2026 - 11 - 01));
    let account = id();
    let rule = RecurringRule {
        account_id: account,
        freq: RecurrenceFreq::Monthly,
        interval: 1,
        by_month_day: Some(1),
        by_weekday: None,
        starts_on: date!(2026 - 11 - 01),
        amount: Money::new(150_000, GHS),
        ..recurring()
    };
    let budget = Budget {
        cap: Money::new(200_000, GHS),
        ..monthly_cap(None, 200_000)
    };

    let mut next = 0u128;
    let generated = generate(
        &rule,
        date!(2026 - 11 - 30),
        midnight(date!(2026 - 10 - 01)),
        zone,
        || {
            next += 1;
            TxId::from_uuid(Uuid::from_u128(next))
        },
    )
    .unwrap();
    let rows: Vec<Transaction> = generated
        .rows
        .into_iter()
        .map(|row| Transaction {
            account_id: account,
            ..row
        })
        .collect();

    let november = evaluate(
        &budget,
        &CategoryTree::default(),
        &rows,
        date!(2026 - 11 - 15),
        BalanceBasis::Projected,
        zone,
    )
    .unwrap()
    .unwrap();
    let october = evaluate(
        &budget,
        &CategoryTree::default(),
        &rows,
        date!(2026 - 10 - 15),
        BalanceBasis::Projected,
        zone,
    )
    .unwrap()
    .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(november.spent, Money::new(150_000, GHS));
    assert_eq!(october.spent, Money::new(0, GHS));
}
