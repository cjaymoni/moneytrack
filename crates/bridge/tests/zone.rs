//! The zone table: the platform's answer, carried across the bridge.
//!
//! The core will not resolve `Africa/Accra`, so Dart reads its own zone database and hands
//! over the offsets. What matters here is the one property the rules lean on — that the
//! instant a local date begins reads back as that same local date — and that it survives the
//! two days a year a shifting zone makes it awkward.

#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::{accra, london};
use mt_bridge::error::BridgeError;
use mt_bridge::zone::{ZoneDto, ZoneTransitionDto};
use mt_core::zone::Zone;
use time::macros::{date, datetime};
use time::{Date, Duration, UtcOffset};

/// Every local date in `from..=to`, for round-trip checks across a shift.
fn days(from: Date, to: Date) -> Vec<Date> {
    let mut dates = Vec::new();
    let mut current = from;
    while current <= to {
        dates.push(current);
        match current.next_day() {
            Some(next) => current = next,
            None => break,
        }
    }
    dates
}

#[test]
fn a_fixed_zone_answers_both_questions_with_the_same_offset() {
    let zone = accra().to_table().unwrap();

    assert_eq!(
        zone.offset_for_instant(datetime!(2026-06-01 12:00:00 UTC)),
        UtcOffset::UTC
    );
    assert_eq!(zone.offset_for_date(date!(2026 - 06 - 01)), UtcOffset::UTC);
    assert_eq!(
        zone.start_of_day(date!(2026 - 06 - 01)),
        datetime!(2026-06-01 00:00:00 UTC)
    );
}

#[test]
fn the_base_offset_stands_before_the_first_transition_and_each_one_after_it() {
    let zone = london().to_table().unwrap();
    let hour = UtcOffset::from_whole_seconds(3_600).unwrap();

    assert_eq!(
        zone.offset_for_instant(datetime!(2026-01-15 12:00:00 UTC)),
        UtcOffset::UTC
    );
    assert_eq!(
        zone.offset_for_instant(datetime!(2026-06-15 12:00:00 UTC)),
        hour
    );
    assert_eq!(
        zone.offset_for_instant(datetime!(2026-12-15 12:00:00 UTC)),
        UtcOffset::UTC
    );
}

#[test]
fn a_transition_takes_effect_at_its_own_instant_and_not_before() {
    let zone = london().to_table().unwrap();
    let hour = UtcOffset::from_whole_seconds(3_600).unwrap();

    assert_eq!(
        zone.offset_for_instant(datetime!(2026-03-29 00:59:59 UTC)),
        UtcOffset::UTC
    );
    assert_eq!(
        zone.offset_for_instant(datetime!(2026-03-29 01:00:00 UTC)),
        hour
    );
}

#[test]
fn transitions_out_of_order_are_sorted_rather_than_trusted() {
    // The caller is a platform database read through Dart. A table in the wrong order would
    // otherwise answer with the wrong offset and never say so.
    let shuffled = ZoneDto {
        base_offset_seconds: 0,
        transitions: vec![
            ZoneTransitionDto {
                at: "2026-10-25T01:00:00Z".to_owned(),
                offset_seconds: 0,
            },
            ZoneTransitionDto {
                at: "2026-03-29T01:00:00Z".to_owned(),
                offset_seconds: 3_600,
            },
        ],
    };

    assert_eq!(
        shuffled.to_table().unwrap(),
        london().to_table().unwrap(),
        "a shuffled table must resolve to the same zone"
    );
}

#[test]
fn the_start_of_a_local_day_reads_back_as_that_day_across_both_shifts() {
    let zone = london().to_table().unwrap();

    for day in days(date!(2026 - 03 - 26), date!(2026 - 04 - 01))
        .into_iter()
        .chain(days(date!(2026 - 10 - 22), date!(2026 - 10 - 28)))
    {
        assert_eq!(
            zone.local_date(zone.start_of_day(day)),
            day,
            "start_of_day did not read back as {day}"
        );
    }
}

#[test]
fn a_day_whose_midnight_is_skipped_still_begins_somewhere_inside_itself() {
    // The clocks jump at local midnight: 1 November never has a 00:00 at all.
    let zone = ZoneDto {
        base_offset_seconds: 0,
        transitions: vec![ZoneTransitionDto {
            at: "2026-11-01T00:00:00Z".to_owned(),
            offset_seconds: 3_600,
        }],
    }
    .to_table()
    .unwrap();

    let first = zone.start_of_day(date!(2026 - 11 - 01));

    assert_eq!(zone.local_date(first), date!(2026 - 11 - 01));
    assert_eq!(first, datetime!(2026-11-01 00:00:00 UTC));
}

#[test]
fn a_day_whose_midnight_happens_twice_begins_at_one_of_them() {
    // The clocks fall back through local midnight: 00:00 on 1 November happens twice.
    let zone = ZoneDto {
        base_offset_seconds: 3_600,
        transitions: vec![ZoneTransitionDto {
            at: "2026-11-01T00:00:00Z".to_owned(),
            offset_seconds: 0,
        }],
    }
    .to_table()
    .unwrap();

    let first = zone.start_of_day(date!(2026 - 11 - 01));

    assert_eq!(zone.local_date(first), date!(2026 - 11 - 01));
}

#[test]
fn the_end_of_a_shifting_day_is_a_moment_before_the_next_one_starts() {
    let zone = london().to_table().unwrap();
    let autumn = date!(2026 - 10 - 25);

    let end = zone.end_of_day(autumn);

    // The autumn day is 25 hours long, and reckoning its end at the offset in force at its
    // start would lose the last of them.
    assert_eq!(zone.local_date(end), autumn);
    assert_eq!(
        end + Duration::nanoseconds(1),
        zone.start_of_day(date!(2026 - 10 - 26))
    );
}

#[test]
fn an_offset_beyond_the_calendar_is_refused_rather_than_wrapped() {
    let absurd = ZoneDto::fixed(200_000);

    assert!(matches!(
        absurd.to_table().unwrap_err(),
        BridgeError::OutOfRange { ref field, .. } if field == "zone.base_offset_seconds"
    ));
}

#[test]
fn a_transition_instant_that_does_not_parse_is_refused() {
    let broken = ZoneDto {
        base_offset_seconds: 0,
        transitions: vec![ZoneTransitionDto {
            at: "last Sunday in March".to_owned(),
            offset_seconds: 3_600,
        }],
    };

    assert!(matches!(
        broken.to_table().unwrap_err(),
        BridgeError::InvalidTimestamp { ref field, .. } if field == "zone.transition.at"
    ));
}
