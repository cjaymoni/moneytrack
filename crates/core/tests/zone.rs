//! The one property the rest of the crate leans on: a local date survives the round trip
//! out to an instant and back.
//!
//! [`mt_core::recurrence`] turns a local date into an instant to book a bill at;
//! [`mt_core::budget`] turns an instant back into a local date to decide which period it
//! falls in. If those two disagree for any zone, a generated bill lands in one month and is
//! counted in another, and nothing on screen explains it.

// Sealed like the library it tests: an integration test is its own crate root, and the
// workspace only denies unsafe, which an `allow` in this file would switch off.
#![forbid(unsafe_code)]

#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

mod common;

use common::{ACCRA, FAR_EAST, FAR_WEST, Shifting, minus_ten};
use mt_core::zone::Zone;
use time::macros::{date, datetime};
use time::{Date, Duration};

/// Every date in a window, so a property test does not have to pick its one lucky date.
fn days_of(from: Date, through: Date) -> Vec<Date> {
    let mut days = Vec::new();
    let mut day = from;
    while day <= through {
        days.push(day);
        day = match day.next_day() {
            Some(next) => next,
            None => break,
        };
    }
    days
}

/// Asserts the round trip over a window, naming the zone if it fails.
fn round_trips(zone: &impl Zone, label: &str, from: Date, through: Date) {
    for day in days_of(from, through) {
        let start = zone.start_of_day(day);
        assert_eq!(
            zone.local_date(start),
            day,
            "{label}: start_of_day({day}) = {start}, which reads back as another date"
        );
    }
}

#[test]
fn a_fixed_offset_round_trips() {
    for (label, zone) in [("UTC", ACCRA), ("-11", FAR_WEST), ("+14", FAR_EAST)] {
        round_trips(&zone, label, date!(2026 - 01 - 01), date!(2026 - 12 - 31));
    }
}

#[test]
fn a_zone_that_jumps_over_midnight_round_trips() {
    // Forward at midnight: the clocks go from 23:59 straight to 01:00, so this date has no
    // 00:00 at all. Taking the offset from the date alone produced an instant that read back
    // as the day before, which put a generated bill in the wrong month.
    let zone = Shifting::at_midnight(FAR_WEST, minus_ten(), date!(2026 - 11 - 01));

    round_trips(
        &zone,
        "forward at midnight",
        date!(2026 - 10 - 01),
        date!(2026 - 12 - 01),
    );
}

#[test]
fn a_zone_that_falls_back_through_midnight_round_trips() {
    let zone = Shifting::at_midnight(minus_ten(), FAR_WEST, date!(2026 - 11 - 01));

    round_trips(
        &zone,
        "back at midnight",
        date!(2026 - 10 - 01),
        date!(2026 - 12 - 01),
    );
}

#[test]
fn a_zone_that_shifts_at_two_in_the_morning_round_trips_in_both_directions() {
    let forward = Shifting::at_two_am(FAR_WEST, minus_ten(), date!(2026 - 03 - 29));
    let back = Shifting::at_two_am(minus_ten(), FAR_WEST, date!(2026 - 10 - 25));

    round_trips(
        &forward,
        "forward at 02:00",
        date!(2026 - 03 - 01),
        date!(2026 - 04 - 30),
    );
    round_trips(
        &back,
        "back at 02:00",
        date!(2026 - 10 - 01),
        date!(2026 - 11 - 30),
    );
}

#[test]
fn the_end_of_a_day_is_the_instant_before_the_next_one_starts() {
    // Reckoning the end of the day at the offset in force at its *start* loses the extra
    // hour of an autumn day, so a store query built from it would skip purchases that
    // `local_date` agrees are inside the window.
    let zone = Shifting::at_two_am(minus_ten(), FAR_WEST, date!(2026 - 10 - 25));
    let shift_day = date!(2026 - 10 - 25);

    let end = zone.end_of_day(shift_day);
    let next_start = zone.start_of_day(date!(2026 - 10 - 26));

    assert_eq!(end, next_start - Duration::nanoseconds(1));
    assert_eq!(zone.local_date(end), shift_day);
    // The day really is twenty-five hours long, and the window has to cover all of it.
    assert_eq!(
        end - zone.start_of_day(shift_day),
        Duration::hours(25) - Duration::nanoseconds(1)
    );
}

#[test]
fn every_instant_of_a_local_day_falls_inside_that_days_bounds() {
    let zone = Shifting::at_two_am(minus_ten(), FAR_WEST, date!(2026 - 10 - 25));

    for day in days_of(date!(2026 - 10 - 20), date!(2026 - 10 - 30)) {
        let (start, end) = (zone.start_of_day(day), zone.end_of_day(day));
        let mut at = start;
        while at <= end {
            assert_eq!(
                zone.local_date(at),
                day,
                "{at} is inside the bounds of {day} but reads as another date"
            );
            at += Duration::hours(1);
        }
    }
}

#[test]
fn a_fixed_offset_ends_its_day_a_nanosecond_before_midnight() {
    let end = FAR_WEST.end_of_day(date!(2026 - 03 - 17));

    assert_eq!(end, datetime!(2026-03-18 10:59:59.999999999 UTC));
}
