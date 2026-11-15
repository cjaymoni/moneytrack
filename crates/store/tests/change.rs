//! The change bus: what the screens re-query on.

#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::{Arc, Mutex};

use common::{spend, store, wallet};
use mt_core::ledger::BalanceBasis;
use mt_store::{Change, ChangeObserver, NewTransaction, Store, Table};

/// Records what it is told.
#[derive(Debug, Default)]
struct Recorder(Mutex<Vec<Change>>);

impl Recorder {
    fn changes(&self) -> Vec<Change> {
        self.0.lock().unwrap().clone()
    }
}

impl ChangeObserver for Recorder {
    fn changed(&self, change: &Change) {
        self.0.lock().unwrap().push(change.clone());
    }
}

#[test]
fn a_write_names_the_table_it_touched() {
    let (store, _directory) = store();
    let recorder = Arc::new(Recorder::default());
    let observer: Arc<dyn ChangeObserver> = recorder.clone();
    store.subscribe(observer).unwrap();

    let account = store.create_account(&wallet(10_000)).unwrap();
    store.record_transaction(&spend(account.id, 1_000)).unwrap();

    assert_eq!(
        recorder.changes(),
        vec![
            Change {
                revision: 1,
                tables: vec![Table::Accounts],
            },
            Change {
                revision: 2,
                tables: vec![Table::Transactions],
            },
        ]
    );
}

#[test]
fn the_revision_is_readable_without_subscribing() {
    let (store, _directory) = store();

    assert_eq!(store.revision(), 0);
    let account = store.create_account(&wallet(0)).unwrap();
    assert_eq!(store.revision(), 1);
    store.record_transaction(&spend(account.id, 1)).unwrap();
    assert_eq!(store.revision(), 2);
}

#[test]
fn a_refused_write_publishes_nothing() {
    let (store, _directory) = store();
    let account = store.create_account(&wallet(0)).unwrap();
    let recorder = Arc::new(Recorder::default());
    let observer: Arc<dyn ChangeObserver> = recorder.clone();
    store.subscribe(observer).unwrap();

    store
        .record_transaction(&NewTransaction {
            label: "  ".to_owned(),
            ..spend(account.id, 1_000)
        })
        .unwrap_err();

    assert_eq!(recorder.changes(), Vec::new());
    assert_eq!(store.revision(), 1);
}

#[test]
fn unsubscribing_stops_the_calls_and_doing_it_twice_is_not_an_error() {
    let (store, _directory) = store();
    let recorder = Arc::new(Recorder::default());
    let observer: Arc<dyn ChangeObserver> = recorder.clone();
    let subscription = store.subscribe(observer).unwrap();

    store.create_account(&wallet(0)).unwrap();
    store.unsubscribe(subscription).unwrap();
    store.unsubscribe(subscription).unwrap();
    store.create_account(&wallet(0)).unwrap();

    assert_eq!(recorder.changes().len(), 1);
}

#[test]
fn every_subscriber_hears_the_same_change() {
    let (store, _directory) = store();
    let first = Arc::new(Recorder::default());
    let second = Arc::new(Recorder::default());
    let first_observer: Arc<dyn ChangeObserver> = first.clone();
    let second_observer: Arc<dyn ChangeObserver> = second.clone();
    store.subscribe(first_observer).unwrap();
    store.subscribe(second_observer).unwrap();

    store.create_account(&wallet(0)).unwrap();

    assert_eq!(first.changes(), second.changes());
    assert_eq!(first.changes().len(), 1);
}

/// Re-queries the store from inside the notification, which is exactly what a screen does.
struct Requerier {
    store: Mutex<Option<Arc<Store>>>,
    balances: Mutex<Vec<usize>>,
}

impl ChangeObserver for Requerier {
    fn changed(&self, _change: &Change) {
        let store = self.store.lock().unwrap();
        if let Some(store) = store.as_ref() {
            let balances = store.account_balances(BalanceBasis::Confirmed).unwrap();
            self.balances.lock().unwrap().push(balances.len());
        }
    }
}

#[test]
fn an_observer_may_query_the_store_from_inside_the_notification() {
    let (store, _directory) = store();
    let store = Arc::new(store);
    let observer = Arc::new(Requerier {
        store: Mutex::new(Some(Arc::clone(&store))),
        balances: Mutex::new(Vec::new()),
    });
    let listener: Arc<dyn ChangeObserver> = observer.clone();
    store.subscribe(listener).unwrap();

    store.create_account(&wallet(10_000)).unwrap();

    // The count a screen would have rendered: the write it was told about is committed and
    // visible by the time it is told, not a moment after.
    assert_eq!(*observer.balances.lock().unwrap(), vec![1]);
}
