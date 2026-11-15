//! Opening a database: the key, the schema, and the two refusals.

#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{FixedClock, NOW, key, other_key, store, wallet};
use mt_store::{KEY_LEN, SCHEMA_VERSION, Store, StoreError};
use tempfile::TempDir;

#[test]
fn a_new_file_gets_the_schema_and_a_device_id() {
    let (store, _directory) = store();

    assert_eq!(store.accounts().unwrap(), Vec::new());
    assert_ne!(store.device_id().to_string(), String::new());
}

#[test]
fn reopening_keeps_the_device_id_and_the_rows() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("ledger.db");

    let (device_id, account_id) = {
        let store = Store::open_with(&path, &key(), FixedClock::at(NOW)).unwrap();
        let account = store.create_account(&wallet(10_000)).unwrap();
        (store.device_id(), account.id)
    };

    let reopened = Store::open_with(&path, &key(), FixedClock::at(NOW)).unwrap();

    assert_eq!(reopened.device_id(), device_id);
    assert_eq!(reopened.account(account_id).unwrap().id, account_id);
}

#[test]
fn the_wrong_key_is_refused_rather_than_opening_an_empty_ledger() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("ledger.db");
    {
        let store = Store::open_with(&path, &key(), FixedClock::at(NOW)).unwrap();
        store.create_account(&wallet(10_000)).unwrap();
    }

    let error = Store::open_with(&path, &other_key(), FixedClock::at(NOW)).unwrap_err();

    assert_eq!(error, StoreError::Unreadable);
}

#[test]
fn the_file_on_disk_is_not_a_readable_database() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("ledger.db");
    {
        let store = Store::open_with(&path, &key(), FixedClock::at(NOW)).unwrap();
        store.create_account(&wallet(10_000)).unwrap();
    }

    // Plain SQLite, no key: the point of encrypting at rest is that this fails.
    let plain = rusqlite::Connection::open(&path).unwrap();
    let read = plain.query_row("SELECT count(*) FROM sqlite_schema", [], |row| {
        row.get::<_, i64>(0)
    });

    assert!(read.is_err(), "the database opened without the key");
}

#[test]
fn a_key_of_the_wrong_length_is_refused_before_the_file_is_touched() {
    let directory = TempDir::new().unwrap();

    let error = Store::open_with(
        &directory.path().join("ledger.db"),
        b"too short",
        FixedClock::at(NOW),
    )
    .unwrap_err();

    assert_eq!(
        error,
        StoreError::KeyLength {
            expected: KEY_LEN,
            supplied: 9,
        }
    );
    assert!(
        !directory.path().join("ledger.db").exists(),
        "a refused key still created the file"
    );
}

#[test]
fn a_database_from_a_newer_build_is_refused() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("ledger.db");
    {
        let _store = Store::open_with(&path, &key(), FixedClock::at(NOW)).unwrap();
    }
    age_forward(&path, SCHEMA_VERSION + 1);

    let error = Store::open_with(&path, &key(), FixedClock::at(NOW)).unwrap_err();

    assert_eq!(
        error,
        StoreError::SchemaTooNew {
            found: SCHEMA_VERSION + 1,
            known: SCHEMA_VERSION,
        }
    );
}

#[test]
fn opening_an_existing_database_twice_does_not_run_the_schema_again() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("ledger.db");

    let first = Store::open_with(&path, &key(), FixedClock::at(NOW)).unwrap();
    let account = first.create_account(&wallet(10_000)).unwrap();
    drop(first);

    let second = Store::open_with(&path, &key(), FixedClock::at(NOW)).unwrap();

    assert_eq!(second.accounts().unwrap().len(), 1);
    assert_eq!(
        second.accounts().unwrap().first().map(|row| row.id),
        Some(account.id)
    );
}

/// Stamps a schema version onto an existing file, as a later build would have left it.
fn age_forward(path: &std::path::Path, version: i64) {
    let connection = rusqlite::Connection::open(path).unwrap();
    let hex: String = key().iter().map(|byte| format!("{byte:02x}")).collect();
    connection
        .execute_batch(&format!(
            "PRAGMA key = \"x'{hex}'\"; PRAGMA user_version = {version};"
        ))
        .unwrap();
}
