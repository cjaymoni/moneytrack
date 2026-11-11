//! Expanding recurring templates: the cadences, the short months, and idempotency.

#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

mod common;

use common::{
    ACCRA, FAR_EAST, FAR_WEST, NOW, Shifting, id, midnight, minus_ten, recurring, transaction,
};
use mt_core::enums::{Direction, RecurrenceFreq, RecurrenceScope, TxSource, TxStatus};
use mt_core::ids::TxId;
use mt_core::model::{RecurringRule, Transaction};
use mt_core::recurrence::{
    Generation, MAX_GENERATED_PER_CALL, RecurrenceError, generate, instances_for_edit, occurrences,
    scope_selects,
};
use mt_core::zone::Zone;
use time::macros::{date, datetime};
use time::{Date, Duration};
use uuid::Uuid;

/// Ids handed out in sequence, so a generated row can be named in an assertion.
fn counter() -> impl FnMut() -> TxId {
    let mut next = 0u128;
    move || {
        next += 1;
        TxId::from_uuid(Uuid::from_u128(next))
    }
}

/// The dates a template fires on in a window, as plain dates.
fn dates(rule: &RecurringRule, from: Date, through: Date) -> Vec<Date> {
    occurrences(rule, from, through).unwrap()
}

// ------------------------------------------------------------------------------- daily ---

#[test]
fn a_daily_rule_fires_every_day_from_its_start() {
    let rule = RecurringRule {
        freq: RecurrenceFreq::Daily,
        interval: 1,
        by_month_day: None,
        by_weekday: None,
        starts_on: date!(2026 - 03 - 01),
        ..recurring()
    };

    assert_eq!(
        dates(&rule, date!(2026 - 03 - 01), date!(2026 - 03 - 04)),
        vec![
            date!(2026 - 03 - 01),
            date!(2026 - 03 - 02),
            date!(2026 - 03 - 03),
            date!(2026 - 03 - 04),
        ]
    );
}

#[test]
fn an_interval_skips_the_days_in_between() {
    let rule = RecurringRule {
        freq: RecurrenceFreq::Daily,
        interval: 3,
        by_month_day: None,
        by_weekday: None,
        starts_on: date!(2026 - 03 - 01),
        ..recurring()
    };

    assert_eq!(
        dates(&rule, date!(2026 - 03 - 01), date!(2026 - 03 - 10)),
        vec![
            date!(2026 - 03 - 01),
            date!(2026 - 03 - 04),
            date!(2026 - 03 - 07),
            date!(2026 - 03 - 10),
        ]
    );
}

// ------------------------------------------------------------------------------ weekly ---

#[test]
fn a_weekly_rule_moves_forward_to_its_chosen_day_before_it_starts() {
    // 2026-03-01 is a Sunday. Asked for Wednesdays, the first firing is the 4th, not the 1st.
    let rule = RecurringRule {
        freq: RecurrenceFreq::Weekly,
        interval: 1,
        by_month_day: None,
        by_weekday: Some(2),
        starts_on: date!(2026 - 03 - 01),
        ..recurring()
    };

    assert_eq!(
        dates(&rule, date!(2026 - 03 - 01), date!(2026 - 03 - 20)),
        vec![
            date!(2026 - 03 - 04),
            date!(2026 - 03 - 11),
            date!(2026 - 03 - 18),
        ]
    );
}

#[test]
fn a_weekly_rule_that_already_starts_on_its_chosen_day_fires_that_day() {
    // 2026-03-04 is itself a Wednesday; there is nothing to move forward to.
    let rule = RecurringRule {
        freq: RecurrenceFreq::Weekly,
        interval: 2,
        by_month_day: None,
        by_weekday: Some(2),
        starts_on: date!(2026 - 03 - 04),
        ..recurring()
    };

    assert_eq!(
        dates(&rule, date!(2026 - 03 - 01), date!(2026 - 03 - 31)),
        vec![date!(2026 - 03 - 04), date!(2026 - 03 - 18)]
    );
}

// ----------------------------------------------------------------------------- monthly ---

#[test]
fn a_monthly_rule_fires_on_the_same_day_each_month() {
    let rule = RecurringRule {
        freq: RecurrenceFreq::Monthly,
        interval: 1,
        by_month_day: Some(15),
        by_weekday: None,
        starts_on: date!(2026 - 01 - 15),
        ..recurring()
    };

    assert_eq!(
        dates(&rule, date!(2026 - 01 - 01), date!(2026 - 04 - 30)),
        vec![
            date!(2026 - 01 - 15),
            date!(2026 - 02 - 15),
            date!(2026 - 03 - 15),
            date!(2026 - 04 - 15),
        ]
    );
}

#[test]
fn a_day_that_a_month_is_too_short_for_clamps_to_the_last_day_of_that_month() {
    // The rule the plan records: a bill set for the 31st should still bill in February, on
    // the 28th, rather than going quiet for a month.
    let rule = RecurringRule {
        freq: RecurrenceFreq::Monthly,
        interval: 1,
        by_month_day: Some(31),
        by_weekday: None,
        starts_on: date!(2026 - 01 - 31),
        ..recurring()
    };

    assert_eq!(
        dates(&rule, date!(2026 - 01 - 01), date!(2026 - 05 - 31)),
        vec![
            date!(2026 - 01 - 31),
            date!(2026 - 02 - 28),
            date!(2026 - 03 - 31),
            date!(2026 - 04 - 30),
            date!(2026 - 05 - 31),
        ]
    );
}

#[test]
fn a_short_month_does_not_drag_every_later_month_down_with_it() {
    // This is the bug that working the next date out from the previous one produces: June's
    // clamp to the 30th sticks, and by August the bill arrives on the 30th for ever. Every
    // occurrence here is computed from the template's start instead.
    let rule = RecurringRule {
        freq: RecurrenceFreq::Monthly,
        interval: 1,
        by_month_day: Some(31),
        by_weekday: None,
        starts_on: date!(2026 - 05 - 31),
        ..recurring()
    };

    let fired = dates(&rule, date!(2026 - 05 - 01), date!(2026 - 08 - 31));

    assert_eq!(
        fired,
        vec![
            date!(2026 - 05 - 31),
            date!(2026 - 06 - 30),
            date!(2026 - 07 - 31),
            date!(2026 - 08 - 31),
        ]
    );
}

#[test]
fn february_in_a_leap_year_gets_its_twenty_ninth() {
    let rule = RecurringRule {
        freq: RecurrenceFreq::Monthly,
        interval: 1,
        by_month_day: Some(30),
        by_weekday: None,
        starts_on: date!(2028 - 01 - 30),
        ..recurring()
    };

    let fired = dates(&rule, date!(2028 - 02 - 01), date!(2028 - 02 - 29));

    assert_eq!(fired, vec![date!(2028 - 02 - 29)]);
}

#[test]
fn a_monthly_interval_skips_the_months_in_between() {
    let rule = RecurringRule {
        freq: RecurrenceFreq::Monthly,
        interval: 3,
        by_month_day: Some(1),
        by_weekday: None,
        starts_on: date!(2026 - 01 - 01),
        ..recurring()
    };

    assert_eq!(
        dates(&rule, date!(2026 - 01 - 01), date!(2026 - 12 - 31)),
        vec![
            date!(2026 - 01 - 01),
            date!(2026 - 04 - 01),
            date!(2026 - 07 - 01),
            date!(2026 - 10 - 01),
        ]
    );
}

#[test]
fn an_occurrence_before_the_template_starts_is_not_generated() {
    // The anchor month holds a firing on the 1st, but the template only starts on the 20th.
    let rule = RecurringRule {
        freq: RecurrenceFreq::Monthly,
        interval: 1,
        by_month_day: Some(1),
        by_weekday: None,
        starts_on: date!(2026 - 01 - 20),
        ..recurring()
    };

    assert_eq!(
        dates(&rule, date!(2026 - 01 - 01), date!(2026 - 03 - 31)),
        vec![date!(2026 - 02 - 01), date!(2026 - 03 - 01)]
    );
}

// ------------------------------------------------------------------------------ yearly ---

#[test]
fn a_yearly_rule_keeps_its_month_and_clamps_its_day() {
    let rule = RecurringRule {
        freq: RecurrenceFreq::Yearly,
        interval: 1,
        by_month_day: Some(29),
        by_weekday: None,
        starts_on: date!(2028 - 02 - 29),
        ..recurring()
    };

    assert_eq!(
        dates(&rule, date!(2028 - 01 - 01), date!(2030 - 12 - 31)),
        vec![
            date!(2028 - 02 - 29),
            date!(2029 - 02 - 28),
            date!(2030 - 02 - 28),
        ]
    );
}

// ------------------------------------------------------------------------------ windows ---

#[test]
fn the_series_stops_at_the_templates_end_date() {
    let rule = RecurringRule {
        freq: RecurrenceFreq::Monthly,
        interval: 1,
        by_month_day: Some(1),
        by_weekday: None,
        starts_on: date!(2026 - 01 - 01),
        ends_on: Some(date!(2026 - 03 - 15)),
        ..recurring()
    };

    assert_eq!(
        dates(&rule, date!(2026 - 01 - 01), date!(2026 - 12 - 31)),
        vec![
            date!(2026 - 01 - 01),
            date!(2026 - 02 - 01),
            date!(2026 - 03 - 01),
        ]
    );
}

#[test]
fn a_window_that_ends_before_the_template_starts_is_empty_rather_than_an_error() {
    let rule = RecurringRule {
        starts_on: date!(2026 - 06 - 01),
        ..recurring()
    };

    assert_eq!(
        occurrences(&rule, date!(2026 - 01 - 01), date!(2026 - 02 - 01)),
        Ok(Vec::new())
    );
}

#[test]
fn an_invalid_template_is_refused_rather_than_expanded() {
    let rule = RecurringRule {
        interval: 0,
        ..recurring()
    };

    assert!(matches!(
        occurrences(&rule, date!(2026 - 01 - 01), date!(2026 - 12 - 31)),
        Err(RecurrenceError::Invalid(_))
    ));
}

#[test]
fn a_window_too_wide_to_generate_in_one_call_is_an_error_not_a_truncation() {
    // Silently returning the first thousand would lose the tail of the series without
    // anything saying so, and `generated_through` would then claim it had been written.
    let rule = RecurringRule {
        freq: RecurrenceFreq::Daily,
        interval: 1,
        by_month_day: None,
        by_weekday: None,
        starts_on: date!(2026 - 01 - 01),
        ends_on: None,
        ..recurring()
    };

    assert_eq!(
        occurrences(&rule, date!(2026 - 01 - 01), date!(2036 - 01 - 01)),
        Err(RecurrenceError::TooMany {
            limit: MAX_GENERATED_PER_CALL
        })
    );
}

// --------------------------------------------------------------------------- generation ---

#[test]
fn generated_rows_are_pending_and_carry_the_templates_details() {
    let rule = RecurringRule {
        freq: RecurrenceFreq::Monthly,
        interval: 1,
        by_month_day: Some(1),
        by_weekday: None,
        starts_on: date!(2026 - 12 - 01),
        ..recurring()
    };

    let Generation {
        rows,
        generated_through,
    } = generate(&rule, date!(2027 - 02 - 28), NOW, ACCRA, counter()).unwrap();

    assert_eq!(rows.len(), 3);
    assert_eq!(generated_through, date!(2027 - 02 - 28));

    let first = &rows[0];
    assert_eq!(first.status, TxStatus::Pending);
    assert_eq!(first.source, TxSource::Recurring);
    assert_eq!(first.recurring_id, Some(rule.id));
    assert_eq!(first.account_id, rule.account_id);
    assert_eq!(first.amount, rule.amount);
    assert_eq!(first.direction, rule.direction);
    assert_eq!(first.label, rule.label);
    assert_eq!(first.booked_at, midnight(date!(2026 - 12 - 01)));
    assert_eq!(first.validate(), Ok(()));
}

#[test]
fn generating_twice_over_the_same_window_produces_the_rows_once() {
    // Generation runs whenever the app opens. Without the watermark, every launch would add
    // another copy of next month's rent.
    let rule = RecurringRule {
        freq: RecurrenceFreq::Monthly,
        interval: 1,
        by_month_day: Some(1),
        by_weekday: None,
        starts_on: date!(2026 - 12 - 01),
        ..recurring()
    };

    let first_run = generate(&rule, date!(2027 - 02 - 28), NOW, ACCRA, counter()).unwrap();
    let caught_up = RecurringRule {
        generated_through: Some(first_run.generated_through),
        ..rule.clone()
    };
    let second_run = generate(&caught_up, date!(2027 - 02 - 28), NOW, ACCRA, counter()).unwrap();

    assert_eq!(first_run.rows.len(), 3);
    assert!(second_run.rows.is_empty());
    assert_eq!(second_run.generated_through, date!(2027 - 02 - 28));
}

#[test]
fn generation_resumes_from_the_watermark_rather_than_from_the_start() {
    let rule = RecurringRule {
        freq: RecurrenceFreq::Monthly,
        interval: 1,
        by_month_day: Some(1),
        by_weekday: None,
        starts_on: date!(2026 - 12 - 01),
        generated_through: Some(date!(2027 - 01 - 31)),
        ..recurring()
    };

    let run = generate(&rule, date!(2027 - 03 - 31), NOW, ACCRA, counter()).unwrap();
    let fired: Vec<Date> = run.rows.iter().map(|row| row.booked_at.date()).collect();

    assert_eq!(fired, vec![date!(2027 - 02 - 01), date!(2027 - 03 - 01)]);
}

#[test]
fn asking_for_a_date_already_passed_leaves_the_watermark_where_it_was() {
    // Writing the narrower window back would move `generated_through` backwards, and the gap
    // between the two would then be generated a second time.
    let rule = RecurringRule {
        freq: RecurrenceFreq::Monthly,
        interval: 1,
        by_month_day: Some(1),
        by_weekday: None,
        starts_on: date!(2026 - 12 - 01),
        generated_through: Some(date!(2027 - 06 - 30)),
        ..recurring()
    };

    let run = generate(&rule, date!(2027 - 01 - 31), NOW, ACCRA, counter()).unwrap();

    assert!(run.rows.is_empty());
    assert_eq!(run.generated_through, date!(2027 - 06 - 30));
}

#[test]
fn a_template_whose_series_has_ended_generates_nothing_but_still_moves_the_watermark() {
    let rule = RecurringRule {
        freq: RecurrenceFreq::Monthly,
        interval: 1,
        by_month_day: Some(1),
        by_weekday: None,
        starts_on: date!(2026 - 01 - 01),
        ends_on: Some(date!(2026 - 03 - 01)),
        generated_through: Some(date!(2026 - 03 - 01)),
        ..recurring()
    };

    let run = generate(&rule, date!(2026 - 12 - 31), NOW, ACCRA, counter()).unwrap();

    assert!(run.rows.is_empty());
    assert_eq!(run.generated_through, date!(2026 - 12 - 31));
}

#[test]
fn every_generated_row_gets_its_own_id() {
    let rule = RecurringRule {
        freq: RecurrenceFreq::Daily,
        interval: 1,
        by_month_day: None,
        by_weekday: None,
        starts_on: date!(2026 - 12 - 01),
        ..recurring()
    };

    let run = generate(&rule, date!(2026 - 12 - 05), NOW, ACCRA, counter()).unwrap();
    let mut ids: Vec<TxId> = run.rows.iter().map(|row| row.id).collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();

    assert_eq!(total, 5);
    assert_eq!(ids.len(), total, "ids must be distinct");
}

// ---------------------------------------------------------------------------- edit scope ---

#[test]
fn each_edit_scope_reaches_exactly_the_instances_it_names() {
    let pivot = NOW;
    let earlier = NOW - Duration::days(1);
    let later = NOW + Duration::days(1);

    for (scope, expected) in [
        (RecurrenceScope::This, vec![false, true, false]),
        (RecurrenceScope::Future, vec![false, true, true]),
        (RecurrenceScope::Past, vec![true, true, false]),
        (RecurrenceScope::All, vec![true, true, true]),
    ] {
        let reached: Vec<bool> = [earlier, pivot, later]
            .into_iter()
            .map(|at| scope_selects(scope, pivot, at))
            .collect();
        assert_eq!(reached, expected, "scope {scope} selected the wrong rows");
    }
}

#[test]
fn an_edit_picks_out_only_this_templates_own_instances() {
    let rule = recurring();
    let mine = Transaction {
        recurring_id: Some(rule.id),
        booked_at: NOW,
        ..transaction()
    };
    let another_template = Transaction {
        id: id(),
        recurring_id: Some(id()),
        booked_at: NOW,
        ..transaction()
    };
    let typed_by_hand = Transaction {
        id: id(),
        recurring_id: None,
        booked_at: NOW,
        ..transaction()
    };
    let rows = vec![mine.clone(), another_template, typed_by_hand];

    let selected = instances_for_edit(&rule, RecurrenceScope::All, NOW, &rows);

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].id, mine.id);
}

#[test]
fn an_edit_does_not_resurrect_an_instance_the_user_deleted() {
    let rule = recurring();
    let mut deleted = Transaction {
        recurring_id: Some(rule.id),
        booked_at: NOW,
        ..transaction()
    };
    deleted.sync.deleted_at = Some(NOW);
    let rows = vec![deleted];

    assert!(instances_for_edit(&rule, RecurrenceScope::All, NOW, &rows).is_empty());
}

#[test]
fn a_future_edit_leaves_the_instances_already_paid_alone() {
    let rule = recurring();
    let paid = Transaction {
        recurring_id: Some(rule.id),
        status: TxStatus::Confirmed,
        booked_at: NOW - Duration::days(30),
        direction: Direction::Out,
        ..transaction()
    };
    let upcoming = Transaction {
        id: id(),
        recurring_id: Some(rule.id),
        status: TxStatus::Pending,
        booked_at: NOW + Duration::days(1),
        ..transaction()
    };
    let rows = vec![paid, upcoming.clone()];

    let selected = instances_for_edit(&rule, RecurrenceScope::Future, NOW, &rows);

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].id, upcoming.id);
}

// ---------------------------------------------------------------------------- time zones ---

#[test]
fn a_bill_lands_at_local_midnight_not_at_utc_midnight() {
    // The whole point of carrying a zone on the template. Eleven hours west, local midnight
    // on 1 December is 11:00 UTC; booking it at 00:00 UTC would put the bill on the previous
    // day everywhere the user looks.
    let rule = RecurringRule {
        freq: RecurrenceFreq::Monthly,
        interval: 1,
        by_month_day: Some(1),
        by_weekday: None,
        starts_on: date!(2026 - 12 - 01),
        ..recurring()
    };

    let run = generate(&rule, date!(2026 - 12 - 31), NOW, FAR_WEST, counter()).unwrap();

    assert_eq!(run.rows.len(), 1);
    assert_eq!(run.rows[0].booked_at, datetime!(2026-12-01 11:00:00 UTC));
    assert_eq!(
        run.rows[0].booked_at.to_offset(FAR_WEST).date(),
        date!(2026 - 12 - 01)
    );
}

#[test]
fn the_same_holds_east_of_greenwich() {
    let rule = RecurringRule {
        freq: RecurrenceFreq::Monthly,
        interval: 1,
        by_month_day: Some(1),
        by_weekday: None,
        starts_on: date!(2026 - 12 - 01),
        ..recurring()
    };

    let run = generate(&rule, date!(2026 - 12 - 31), NOW, FAR_EAST, counter()).unwrap();

    assert_eq!(run.rows[0].booked_at, datetime!(2026-11-30 10:00:00 UTC));
    assert_eq!(
        run.rows[0].booked_at.to_offset(FAR_EAST).date(),
        date!(2026 - 12 - 01)
    );
}

#[test]
fn a_bill_due_after_a_zone_shifts_still_lands_on_the_day_it_was_asked_for() {
    // The zone jumps forward at midnight on 1 November, so local 1 November has no 00:00 at
    // all — the clocks go straight from 31 October 23:59 to 1 November 01:00. The bill still
    // has to land on the first, so its instant is the jump itself.
    //
    // What this must not do is book an instant that reads back as 31 October. That is what
    // taking the offset from the date alone produced, and it put November's rent in
    // October's budget while both modules' own tests passed.
    let rule = RecurringRule {
        freq: RecurrenceFreq::Monthly,
        interval: 1,
        by_month_day: Some(1),
        by_weekday: None,
        starts_on: date!(2026 - 10 - 01),
        ..recurring()
    };
    let zone = Shifting::at_midnight(FAR_WEST, minus_ten(), date!(2026 - 11 - 01));

    let run = generate(&rule, date!(2026 - 11 - 30), NOW, zone, counter()).unwrap();
    let landed: Vec<Date> = run
        .rows
        .iter()
        .map(|row| zone.local_date(row.booked_at))
        .collect();

    assert_eq!(landed, vec![date!(2026 - 10 - 01), date!(2026 - 11 - 01)]);
    // October is an ordinary day on the old offset; November is the transition instant.
    assert_eq!(
        run.rows.iter().map(|row| row.booked_at).collect::<Vec<_>>(),
        vec![
            datetime!(2026-10-01 11:00:00 UTC),
            datetime!(2026-11-01 11:00:00 UTC),
        ]
    );
}
