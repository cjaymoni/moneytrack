//! The published surface over a real database.
//!
//! The crate's other tests hand rows to the rules directly. These open a file, write to it
//! and read it back through exactly the calls Swift and Kotlin make, which is the only way
//! to find out whether the boundary and the store agree about what a row is.

#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::{Arc, Mutex};

use common::{NOW, account_input, money, transaction_input};
use mt_ffi::enums::{BalanceBasis, Direction, TxStatus};
use mt_ffi::store::{
    ChangeDto, ChangeObserver, ChangedTable, NewTransactionDto, Store, StoreError, open_store,
};
use tempfile::TempDir;

/// A key of the right length, which is the only thing the cipher asks of it.
fn key() -> Vec<u8> {
    (0u8..32).collect()
}

/// A store in its own temp directory.
async fn store() -> (Arc<Store>, TempDir) {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("ledger.db");
    let store = open_store(path.to_string_lossy().into_owned(), key())
        .await
        .unwrap();
    (store, directory)
}

#[tokio::test]
async fn a_balance_comes_back_through_the_same_calls_an_app_makes() {
    let (store, _directory) = store().await;

    let account = store.create_account(account_input(25_000)).await.unwrap();
    store
        .record_transaction(transaction_input(&account.id, 4_050))
        .await
        .unwrap();
    let balance = store
        .account_balance(account.id.clone(), BalanceBasis::Confirmed)
        .await
        .unwrap();

    assert_eq!(balance, money(20_950));
    assert_eq!(account.currency, "GHS");
    assert_eq!(account.opening_balance, money(25_000));
}

#[tokio::test]
async fn a_row_reads_back_in_the_spellings_the_wire_commits_to() {
    let (store, _directory) = store().await;
    let account = store.create_account(account_input(0)).await.unwrap();

    let written = store
        .record_transaction(transaction_input(&account.id, 1_234))
        .await
        .unwrap();

    assert_eq!(written.booked_at, NOW);
    assert_eq!(written.amount, money(1_234));
    assert_eq!(written.direction, Direction::Out);
    assert_eq!(written.status, TxStatus::Confirmed);
    assert_eq!(written.account_id, account.id);
    assert_eq!(written.sync.device_id, store.device_id());
    assert_eq!(
        store
            .transactions_for_account(account.id)
            .await
            .unwrap()
            .first(),
        Some(&written)
    );
}

#[tokio::test]
async fn every_balance_comes_back_at_once() {
    let (store, _directory) = store().await;
    let first = store.create_account(account_input(10_000)).await.unwrap();
    let second = store.create_account(account_input(2_500)).await.unwrap();

    let balances = store
        .account_balances(BalanceBasis::Confirmed)
        .await
        .unwrap();

    let ids: Vec<&str> = balances
        .iter()
        .map(|balance| balance.account_id.as_str())
        .collect();
    assert!(ids.contains(&first.id.as_str()));
    assert!(ids.contains(&second.id.as_str()));
    assert_eq!(balances.len(), 2);
}

#[tokio::test]
async fn a_pending_row_shows_in_the_outlook_and_not_in_the_balance() {
    let (store, _directory) = store().await;
    let account = store.create_account(account_input(25_000)).await.unwrap();
    store
        .record_transaction(NewTransactionDto {
            booked_at: "2026-12-01T00:00:00Z".to_owned(),
            status: TxStatus::Pending,
            amount: money(90_000),
            label: "Rent".to_owned(),
            ..transaction_input(&account.id, 0)
        })
        .await
        .unwrap();

    let confirmed = store
        .account_balance(account.id.clone(), BalanceBasis::Confirmed)
        .await
        .unwrap();
    let projected = store
        .account_balance(account.id, BalanceBasis::Projected)
        .await
        .unwrap();

    assert_eq!(confirmed, money(25_000));
    assert_eq!(projected, money(-65_000));
}

#[tokio::test]
async fn an_id_that_is_not_a_uuid_names_the_field_rather_than_returning_nothing() {
    let (store, _directory) = store().await;

    let error = store
        .account_balance("wallet-1".to_owned(), BalanceBasis::Confirmed)
        .await
        .unwrap_err();

    assert_eq!(
        error,
        StoreError::Value {
            reason: "account_id is not a UUID: wallet-1".to_owned(),
        }
    );
}

#[tokio::test]
async fn an_account_that_does_not_exist_is_not_found_rather_than_empty() {
    let (store, _directory) = store().await;
    let absent = "0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b".to_owned();

    let error = store
        .account_balance(absent.clone(), BalanceBasis::Confirmed)
        .await
        .unwrap_err();

    assert_eq!(
        error,
        StoreError::NotFound {
            entity: "account".to_owned(),
            id: absent,
        }
    );
}

#[tokio::test]
async fn a_key_of_the_wrong_length_is_refused_with_both_numbers() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("ledger.db");

    let error = open_store(path.to_string_lossy().into_owned(), vec![1, 2, 3])
        .await
        .unwrap_err();

    assert_eq!(
        error,
        StoreError::KeyLength {
            expected: 32,
            supplied: 3,
        }
    );
}

#[tokio::test]
async fn the_wrong_key_refuses_the_file_rather_than_opening_an_empty_one() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("ledger.db");
    let written = open_store(path.to_string_lossy().into_owned(), key())
        .await
        .unwrap();
    written.create_account(account_input(10_000)).await.unwrap();
    drop(written);

    let error = open_store(
        path.to_string_lossy().into_owned(),
        (32u8..64).collect::<Vec<u8>>(),
    )
    .await
    .unwrap_err();

    assert_eq!(error, StoreError::Unreadable);
}

/// Records what it is told, as a screen's stream adapter would.
#[derive(Default)]
struct Recorder(Mutex<Vec<ChangeDto>>);

impl ChangeObserver for Recorder {
    fn changed(&self, change: ChangeDto) {
        self.0.lock().unwrap().push(change);
    }
}

#[tokio::test]
async fn a_subscriber_is_told_which_table_changed() {
    let (store, _directory) = store().await;
    let recorder = Arc::new(Recorder::default());
    let observer: Arc<dyn ChangeObserver> = recorder.clone();
    let subscription = store.subscribe(observer).unwrap();

    let account = store.create_account(account_input(0)).await.unwrap();
    store
        .record_transaction(transaction_input(&account.id, 100))
        .await
        .unwrap();
    store.unsubscribe(subscription).unwrap();
    store.create_account(account_input(0)).await.unwrap();

    assert_eq!(
        *recorder.0.lock().unwrap(),
        vec![
            ChangeDto {
                revision: 1,
                tables: vec![ChangedTable::Accounts],
            },
            ChangeDto {
                revision: 2,
                tables: vec![ChangedTable::Transactions],
            },
        ]
    );
    assert_eq!(store.revision(), 3);
}
