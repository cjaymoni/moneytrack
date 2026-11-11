//! What the boundary refuses.
//!
//! Everything arriving here has been through a SQLite column, a drift mapper and possibly a
//! sync merge written by a client one release ahead. These tests are the record of what the
//! bridge does with input that is wrong — which is to name the field and stop, never to drop
//! the row and carry on with a balance that is quietly short.

// Sealed like the library's own modules: the workspace only denies unsafe, and a `deny`
// can be switched off by an `allow` in the file that wants it.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::{NOW, account, money, sync, transaction, uuid};
use mt_bridge::dto::{SyncMetaDto, TransactionDto, transactions_to_core};
use mt_bridge::enums::Direction;
use mt_bridge::error::BridgeError;
use mt_bridge::scalar::MoneyDto;

#[test]
fn an_id_that_is_not_a_uuid_names_the_field_it_arrived_in() {
    let mut row = account(1, 0);
    row.id = "wallet-1".to_owned();

    let error = row.to_core().unwrap_err();

    assert_eq!(
        error,
        BridgeError::InvalidId {
            field: "account.id".to_owned(),
            value: "wallet-1".to_owned(),
        }
    );
}

#[test]
fn an_optional_id_that_is_present_and_malformed_is_an_error_not_a_silent_none() {
    let wallet = account(1, 0);
    let mut row = transaction(2, &wallet, Direction::Out, 1_000, NOW);
    row.category_id = Some("groceries".to_owned());

    let error = row.to_core().unwrap_err();

    assert!(matches!(
        error,
        BridgeError::InvalidId { ref field, .. } if field == "transaction.category_id"
    ));
}

#[test]
fn a_currency_that_is_not_three_letters_is_refused() {
    let mut row = account(1, 0);
    row.opening_balance = MoneyDto {
        minor_units: 0,
        currency: "cedi".to_owned(),
    };

    let error = row.to_core().unwrap_err();

    assert_eq!(
        error,
        BridgeError::InvalidCurrency {
            field: "account.opening_balance".to_owned(),
            value: "cedi".to_owned(),
        }
    );
}

#[test]
fn a_timestamp_that_is_not_rfc_3339_is_refused() {
    let wallet = account(1, 0);
    let row = transaction(2, &wallet, Direction::Out, 1_000, "10/11/2026");

    let error = row.to_core().unwrap_err();

    assert!(matches!(
        error,
        BridgeError::InvalidTimestamp { ref field, .. } if field == "transaction.booked_at"
    ));
}

#[test]
fn a_date_that_is_not_iso_is_refused() {
    let mut row = common::budget(1, 50_000, None);
    row.starts_on = "1 November 2026".to_owned();

    let error = row.to_core().unwrap_err();

    assert!(matches!(
        error,
        BridgeError::InvalidDate { ref field, .. } if field == "budget.starts_on"
    ));
}

#[test]
fn a_row_that_converts_but_does_not_validate_carries_the_cores_own_reason() {
    let mut row = account(1, 0);
    row.name = "   ".to_owned();

    let error = row.to_core().unwrap_err();

    match error {
        BridgeError::InvalidRow { row, reason } => {
            assert_eq!(row, "account");
            assert!(reason.contains("name"), "unhelpful reason: {reason}");
        }
        other => panic!("expected an invalid row, got {other:?}"),
    }
}

#[test]
fn a_negative_amount_is_refused_because_direction_carries_the_sign() {
    let wallet = account(1, 0);
    let mut row = transaction(2, &wallet, Direction::Out, 1_000, NOW);
    row.amount = money(-1_000);

    let error = row.to_core().unwrap_err();

    assert!(matches!(error, BridgeError::InvalidRow { ref row, .. } if row == "transaction"));
}

#[test]
fn one_bad_row_stops_the_batch_rather_than_being_dropped_from_it() {
    let wallet = account(1, 0);
    let good = transaction(2, &wallet, Direction::Out, 1_000, NOW);
    let mut bad = transaction(3, &wallet, Direction::In, 2_000, NOW);
    bad.account_id = "not-an-id".to_owned();

    // A dropped row is a balance wrong by an amount nobody can account for, which is worse
    // than a screen that fails to load.
    let error = transactions_to_core(&[good, bad]).unwrap_err();

    assert!(matches!(error, BridgeError::InvalidId { .. }));
}

#[test]
fn a_transaction_survives_the_round_trip_through_the_core_unchanged() {
    let wallet = account(1, 0);
    let original = transaction(2, &wallet, Direction::Out, 1_234, NOW);

    let rendered = TransactionDto::from_core(&original.to_core().unwrap()).unwrap();

    assert_eq!(rendered, original);
}

#[test]
fn sync_columns_round_trip_including_an_absent_deletion() {
    let original = SyncMetaDto {
        deleted_at: Some("2026-11-11T12:00:00Z".to_owned()),
        ..sync()
    };

    let rendered = SyncMetaDto::from_core(&original.to_core().unwrap()).unwrap();

    assert_eq!(rendered, original);
    assert_eq!(
        SyncMetaDto::from_core(&sync().to_core().unwrap()).unwrap(),
        sync()
    );
}

#[test]
fn the_device_id_is_checked_like_any_other_id() {
    let meta = SyncMetaDto {
        device_id: "this-handset".to_owned(),
        ..sync()
    };

    assert!(matches!(
        meta.to_core().unwrap_err(),
        BridgeError::InvalidId { ref field, .. } if field == "device_id"
    ));
}

#[test]
fn builders_produce_rows_the_core_accepts() {
    // Guards the rest of the suite: a builder that stopped validating would make every
    // other test here pass for the wrong reason.
    assert!(account(1, 0).to_core().is_ok());
    assert!(
        transaction(2, &account(1, 0), Direction::Out, 1_000, NOW)
            .to_core()
            .is_ok()
    );
    assert!(common::category(3, "Transport").to_core().is_ok());
    assert!(
        common::rule(4, "melcom", &common::category(3, "Transport"))
            .to_core()
            .is_ok()
    );
    assert!(common::budget(5, 50_000, None).to_core().is_ok());
    assert!(
        common::recurring(6, &account(1, 0), 120_000)
            .to_core()
            .is_ok()
    );
    assert!(common::candidate(2_000, NOW).to_core().is_ok());
    assert_ne!(uuid(1), uuid(2));
}
