//! The shape of a row on the wire.
//!
//! These values cross the `flutter_rust_bridge` boundary and will be the body of
//! `/sync/push`, so the encoding is a protocol commitment: changing it after a build ships
//! breaks every device in the field. The assertions below pin it deliberately rather than
//! letting it be whatever a dependency's default happens to be.

#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::{GHS, account, budget, candidate, inbox_item, recurring, transaction, transfer_pair};
use mt_core::model::{Account, Budget, Candidate, InboxItem, RecurringRule, Transaction};
use mt_core::money::{Currency, Money};

#[test]
fn instants_are_rfc3339_not_a_positional_array() {
    // `time`'s default is its internal component representation — [2026,314,9,0,0,0,0,0,0],
    // day-of-year 314, no stated timezone. It round-trips only because both ends happen to
    // be this crate, and it is unreadable in a log or a captured payload.
    let json = serde_json::to_string(&transaction()).unwrap();
    assert!(
        json.contains(r#""booked_at":"2026-11-10T09:00:00Z""#),
        "booked_at is not RFC 3339 in: {json}"
    );
    assert!(
        json.contains(r#""created_at":"2026-11-10T09:00:00Z""#),
        "created_at is not RFC 3339 in: {json}"
    );
    assert!(
        !json.contains("[2026,"),
        "a timestamp serialised as a component array in: {json}"
    );
}

#[test]
fn calendar_dates_are_plain_iso_days() {
    let json = serde_json::to_string(&recurring()).unwrap();
    assert!(
        json.contains(r#""starts_on":"2026-12-01""#),
        "starts_on is not YYYY-MM-DD in: {json}"
    );

    let json = serde_json::to_string(&budget()).unwrap();
    assert!(
        json.contains(r#""starts_on":"2026-01-01""#) && json.contains(r#""ends_on":"2026-12-31""#),
        "budget dates are not YYYY-MM-DD in: {json}"
    );
}

#[test]
fn an_absent_timestamp_is_null_not_a_missing_field() {
    let json = serde_json::to_string(&account()).unwrap();
    assert!(
        json.contains(r#""deleted_at":null"#) && json.contains(r#""archived_at":null"#),
        "nullable instants should still appear: {json}"
    );
}

#[test]
fn money_is_minor_units_plus_a_code() {
    let json = serde_json::to_string(&Money::new(1250, GHS)).unwrap();
    assert_eq!(json, r#"{"minor":1250,"currency":"GHS"}"#);
}

#[test]
fn sync_bookkeeping_is_flattened_onto_the_row_itself() {
    // Not nested under a `sync` key: the sync columns are columns of the row in both
    // databases, and the payload matches.
    let json = serde_json::to_string(&transaction()).unwrap();
    assert!(
        !json.contains(r#""sync":"#),
        "sync meta should be flat: {json}"
    );
    assert!(json.contains(r#""device_id":"#));
}

macro_rules! assert_round_trips {
    ($($value:expr => $ty:ty),* $(,)?) => {
        $(
            {
                let original = $value;
                let json = serde_json::to_string(&original).unwrap();
                let parsed: $ty = serde_json::from_str(&json)
                    .unwrap_or_else(|e| panic!("{} did not parse back from {json}: {e}",
                                               stringify!($ty)));
                assert_eq!(parsed, original, "{} did not round-trip", stringify!($ty));
            }
        )*
    };
}

#[test]
fn every_row_type_round_trips_through_json() {
    let [leg, _] = transfer_pair();
    assert_round_trips!(
        account() => Account,
        transaction() => Transaction,
        leg => Transaction,
        recurring() => RecurringRule,
        budget() => Budget,
        candidate() => Candidate,
        inbox_item() => InboxItem,
    );
}

#[test]
fn values_parse_from_a_reader_that_cannot_lend_out_borrowed_strings() {
    // `<&str>::deserialize` would fail here with "invalid type: string, expected a borrowed
    // string" on perfectly valid input — and a streaming reader, bincode, MessagePack and
    // whatever codec the bridge settles on are all in that category.
    let json = serde_json::to_string(&Money::new(1250, GHS)).unwrap();
    let parsed: Money = serde_json::from_reader(json.as_bytes()).unwrap();
    assert_eq!(parsed, Money::new(1250, GHS));

    let parsed: Currency = serde_json::from_reader(br#""GHS""#.as_slice()).unwrap();
    assert_eq!(parsed, GHS);
}

#[test]
fn an_unknown_currency_is_refused_on_the_way_in() {
    assert!(serde_json::from_str::<Currency>(r#""ZZZ""#).is_err());
    assert!(serde_json::from_str::<Money>(r#"{"minor":1,"currency":"ZZZ"}"#).is_err());
}
