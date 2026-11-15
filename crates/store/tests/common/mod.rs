//! What the store's tests all need: a key, a fixed clock, and a database in a temp dir.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};

use mt_core::enums::{AccountKind, Direction, TxSource, TxStatus};
use mt_core::ids::AccountId;
use mt_core::money::{Currency, Money};
use mt_store::{Clock, NewAccount, NewTransaction, Store};
use tempfile::TempDir;
use time::OffsetDateTime;
use time::macros::datetime;

/// A key of the right length. A test key, in a test, against a temp file — and still not a
/// constant anyone could mistake for a default: 32 bytes of counting.
pub fn key() -> Vec<u8> {
    (0u8..32).collect()
}

/// A different key of the right length.
pub fn other_key() -> Vec<u8> {
    (32u8..64).collect()
}

/// The instant every test hangs off, so expected values can be read by hand.
pub const NOW: OffsetDateTime = datetime!(2026-11-15 09:00:00 UTC);

/// A clock the test moves.
#[derive(Debug)]
pub struct FixedClock(Mutex<OffsetDateTime>);

impl FixedClock {
    pub fn at(instant: OffsetDateTime) -> Arc<Self> {
        Arc::new(Self(Mutex::new(instant)))
    }

    /// Moves the clock forward, so a second write is distinguishable from the first.
    pub fn advance(&self, by: time::Duration) {
        let mut now = self.0.lock().unwrap();
        *now += by;
    }
}

impl Clock for FixedClock {
    fn now(&self) -> OffsetDateTime {
        *self.0.lock().unwrap()
    }
}

/// A store in its own temp directory. The directory is returned too: dropping it deletes
/// the database, so a test that lets it go early is testing against a deleted file.
pub fn store() -> (Store, TempDir) {
    let clock = FixedClock::at(NOW);
    let directory = TempDir::new().unwrap();
    let store = Store::open_with(&directory.path().join("ledger.db"), &key(), clock).unwrap();
    (store, directory)
}

/// The cedi, which every test here is denominated in.
pub fn ghs() -> Currency {
    Currency::parse("GHS").unwrap()
}

/// An amount in cedi minor units.
pub fn cedis(minor: i64) -> Money {
    Money::new(minor, ghs())
}

/// A mobile money wallet with a known opening balance.
pub fn wallet(opening_minor: i64) -> NewAccount {
    NewAccount {
        name: "Wallet".to_owned(),
        kind: AccountKind::Momo,
        currency: ghs(),
        opening_balance: cedis(opening_minor),
    }
}

/// A confirmed row, booked at [`NOW`].
pub fn spend(account_id: AccountId, minor: i64) -> NewTransaction {
    NewTransaction {
        account_id,
        direction: Direction::Out,
        amount: cedis(minor),
        booked_at: NOW,
        category_id: None,
        label: "Trotro".to_owned(),
        counterparty: None,
        note: None,
        status: TxStatus::Confirmed,
        source: TxSource::Manual,
    }
}
