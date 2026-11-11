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
        covers_from: Some("2025-12-30T00:00:00Z".to_owned()),
        covers_through: Some("2027-01-02T00:00:00Z".to_owned()),
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
        covers_from: Some("2026-10-01T00:00:00Z".to_owned()),
        covers_through: Some("2026-12-01T00:00:00Z".to_owned()),
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
        covers_from: Some("2026-10-01T00:00:00Z".to_owned()),
        covers_through: Some("2026-12-01T00:00:00Z".to_owned()),
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
        covers_from: None,
        covers_through: None,
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

// ------------------------------------------------------------- what the table admits to ---

#[test]
fn a_table_that_claims_a_window_is_held_to_it() {
    let zone = london().to_table().unwrap();

    assert!(
        zone.check_covers(
            "a 2026 question",
            datetime!(2026-06-01 00:00:00 UTC),
            datetime!(2026-06-30 00:00:00 UTC)
        )
        .is_ok()
    );

    match zone.check_covers(
        "a 2027 question",
        datetime!(2027-01-05 00:00:00 UTC),
        datetime!(2027-02-05 00:00:00 UTC),
    ) {
        Err(BridgeError::ZoneCoverage { field, needed }) => {
            assert_eq!(field, "a 2027 question");
            // The end that broke the claim: the near end is inside the window, the far
            // one is past it, and the message names the one actually out of bounds.
            assert!(
                needed.starts_with("2027-02-05"),
                "unhelpful bound: {needed}"
            );
        }
        other => panic!("expected a coverage refusal, got {other:?}"),
    }

    // Either end can be the one that breaks it.
    assert!(matches!(
        zone.check_covers(
            "a question running off the end",
            datetime!(2026-12-01 00:00:00 UTC),
            datetime!(2027-01-03 00:00:00 UTC)
        ),
        Err(BridgeError::ZoneCoverage { ref needed, .. }) if needed.starts_with("2027-01-03")
    ));
    assert!(matches!(
        zone.check_covers(
            "a question starting before the table",
            datetime!(2025-12-29 00:00:00 UTC),
            datetime!(2026-02-01 00:00:00 UTC)
        ),
        Err(BridgeError::ZoneCoverage { ref needed, .. }) if needed.starts_with("2025-12-29")
    ));
}

#[test]
fn a_zone_that_never_shifts_claims_no_window_and_answers_everything() {
    let zone = accra().to_table().unwrap();

    // Nothing to be wrong about: there are no transitions to be missing.
    assert!(
        zone.check_covers(
            "any question at all",
            datetime!(1990-01-01 00:00:00 UTC),
            datetime!(2199-01-01 00:00:00 UTC)
        )
        .is_ok()
    );
}

#[test]
fn a_window_that_ends_before_it_starts_is_refused() {
    let backwards = ZoneDto {
        base_offset_seconds: 0,
        transitions: Vec::new(),
        covers_from: Some("2026-12-31T00:00:00Z".to_owned()),
        covers_through: Some("2026-01-01T00:00:00Z".to_owned()),
    };

    assert!(matches!(
        backwards.to_table().unwrap_err(),
        BridgeError::ZoneClaim { ref field, .. } if field == "zone.covers_through"
    ));
}

#[test]
fn a_window_claimed_at_one_end_only_is_refused() {
    // The hazard this type exists to close, wearing the badge of a table that passed the
    // check: a claim that starts in January and never ends answers for ever from a
    // transition list that stops in October.
    let open_ended = ZoneDto {
        base_offset_seconds: 0,
        transitions: Vec::new(),
        covers_from: Some("2026-01-01T00:00:00Z".to_owned()),
        covers_through: None,
    };
    let open_started = ZoneDto {
        covers_from: None,
        covers_through: Some("2026-12-31T00:00:00Z".to_owned()),
        ..open_ended.clone()
    };

    assert!(matches!(
        open_ended.to_table().unwrap_err(),
        BridgeError::ZoneClaim { ref field, .. } if field == "zone.covers_through"
    ));
    assert!(matches!(
        open_started.to_table().unwrap_err(),
        BridgeError::ZoneClaim { ref field, .. } if field == "zone.covers_from"
    ));
}

#[test]
fn a_table_that_shifts_must_say_what_window_it_is_complete_for() {
    // The same hole as a one-sided claim, with both sides left open: a list of transitions
    // and no claim answers for ever, and the half of the year it is wrong about says
    // nothing. The doc said so; now the type does.
    let unclaimed = ZoneDto {
        base_offset_seconds: 0,
        covers_from: None,
        covers_through: None,
        transitions: vec![ZoneTransitionDto {
            at: "2026-03-29T01:00:00Z".to_owned(),
            offset_seconds: 3_600,
        }],
    };

    assert!(matches!(
        unclaimed.to_table().unwrap_err(),
        BridgeError::ZoneClaim { ref field, .. } if field == "zone.covers_from"
    ));
    // A zone with nothing to be wrong about still needs no claim.
    assert!(ZoneDto::fixed(0).to_table().is_ok());
}
