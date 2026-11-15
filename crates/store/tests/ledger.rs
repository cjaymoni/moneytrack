//! Rows in, rows out, and what they add up to.

#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{NOW, cedis, ghs, spend, store, wallet};
use mt_core::enums::{AccountKind, Direction, TxSource, TxStatus};
use mt_core::ids::AccountId;
use mt_core::ledger::BalanceBasis;
use mt_core::money::{Currency, Money};
use mt_store::{NewAccount, NewTransaction, StoreError};
use time::Duration;
use uuid::Uuid;

#[test]
fn an_account_reads_back_as_it_was_written() {
    let (store, _directory) = store();

    let written = store.create_account(&wallet(25_000)).unwrap();
    let read = store.account(written.id).unwrap();

    assert_eq!(read, written);
    assert_eq!(read.name, "Wallet");
    assert_eq!(read.kind, AccountKind::Momo);
    assert_eq!(read.opening_balance, cedis(25_000));
    assert_eq!(read.sync.device_id, store.device_id());
    assert_eq!(read.sync.created_at, NOW);
}

#[test]
fn accounts_come_back_in_the_order_they_were_opened() {
    let (store, _directory) = store();

    let first = store.create_account(&wallet(0)).unwrap();
    let second = store
        .create_account(&NewAccount {
            name: "Bank".to_owned(),
            ..wallet(0)
        })
        .unwrap();

    let order: Vec<AccountId> = store.accounts().unwrap().iter().map(|row| row.id).collect();

    assert_eq!(order, vec![first.id, second.id]);
}

#[test]
fn an_empty_account_holds_exactly_its_opening_balance() {
    let (store, _directory) = store();
    let account = store.create_account(&wallet(25_000)).unwrap();

    let balance = store
        .account_balance(account.id, BalanceBasis::Confirmed)
        .unwrap();

    assert_eq!(balance, cedis(25_000));
}

#[test]
fn a_transaction_reads_back_and_moves_the_balance() {
    let (store, _directory) = store();
    let account = store.create_account(&wallet(25_000)).unwrap();

    let written = store.record_transaction(&spend(account.id, 4_050)).unwrap();
    let read = store.transactions_for_account(account.id).unwrap();

    assert_eq!(read, vec![written.clone()]);
    assert_eq!(written.amount, cedis(4_050));
    assert_eq!(written.direction, Direction::Out);
    assert_eq!(
        store
            .account_balance(account.id, BalanceBasis::Confirmed)
            .unwrap(),
        cedis(20_950)
    );
}

#[test]
fn a_pending_row_is_the_forecast_and_not_the_balance() {
    let (store, _directory) = store();
    let account = store.create_account(&wallet(25_000)).unwrap();
    let rent = NewTransaction {
        booked_at: NOW + Duration::days(14),
        status: TxStatus::Pending,
        amount: cedis(90_000),
        label: "Rent".to_owned(),
        ..spend(account.id, 0)
    };
    store.record_transaction(&rent).unwrap();

    let confirmed = store
        .account_balance(account.id, BalanceBasis::Confirmed)
        .unwrap();
    let projected = store
        .account_balance(account.id, BalanceBasis::Projected)
        .unwrap();

    assert_eq!(confirmed, cedis(25_000));
    assert_eq!(projected, cedis(-65_000));
}

#[test]
fn money_in_and_money_out_both_count_the_right_way() {
    let (store, _directory) = store();
    let account = store.create_account(&wallet(0)).unwrap();
    store
        .record_transaction(&NewTransaction {
            direction: Direction::In,
            amount: cedis(10_000),
            label: "Salary".to_owned(),
            ..spend(account.id, 0)
        })
        .unwrap();
    store.record_transaction(&spend(account.id, 3_000)).unwrap();

    let balance = store
        .account_balance(account.id, BalanceBasis::Confirmed)
        .unwrap();

    assert_eq!(balance, cedis(7_000));
}

#[test]
fn every_account_is_balanced_in_one_pass() {
    let (store, _directory) = store();
    let wallet_account = store.create_account(&wallet(25_000)).unwrap();
    let bank = store
        .create_account(&NewAccount {
            name: "Bank".to_owned(),
            kind: AccountKind::Bank,
            ..wallet(500_000)
        })
        .unwrap();
    store
        .record_transaction(&spend(wallet_account.id, 5_000))
        .unwrap();

    let balances = store.account_balances(BalanceBasis::Confirmed).unwrap();

    assert_eq!(
        balances,
        vec![
            (wallet_account.id, cedis(20_000)),
            (bank.id, cedis(500_000)),
        ]
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>()
        .into_iter()
        .collect::<Vec<_>>()
    );
}

#[test]
fn a_currency_total_is_per_currency_and_never_converted() {
    let (store, _directory) = store();
    let cedi_account = store.create_account(&wallet(25_000)).unwrap();
    let dollars = Currency::parse("USD").unwrap();
    store
        .create_account(&NewAccount {
            name: "Dollars".to_owned(),
            currency: dollars,
            opening_balance: Money::new(10_000, dollars),
            ..wallet(0)
        })
        .unwrap();
    store
        .record_transaction(&spend(cedi_account.id, 5_000))
        .unwrap();

    let totals = store.currency_totals(BalanceBasis::Confirmed).unwrap();

    assert_eq!(
        totals,
        vec![
            (ghs(), cedis(20_000)),
            (dollars, Money::new(10_000, dollars))
        ]
    );
}

#[test]
fn an_amount_in_the_wrong_currency_says_which_two_disagree() {
    let (store, _directory) = store();
    let account = store.create_account(&wallet(25_000)).unwrap();
    let dollars = Currency::parse("USD").unwrap();

    let error = store
        .record_transaction(&NewTransaction {
            amount: Money::new(1_000, dollars),
            ..spend(account.id, 0)
        })
        .unwrap_err();

    assert_eq!(
        error,
        StoreError::Invalid {
            entity: "transaction",
            reason: "the amount is in USD and the account holds GHS".to_owned(),
        }
    );
}

#[test]
fn an_opening_balance_in_the_wrong_currency_is_refused() {
    let (store, _directory) = store();
    let dollars = Currency::parse("USD").unwrap();

    let error = store
        .create_account(&NewAccount {
            opening_balance: Money::new(1_000, dollars),
            ..wallet(0)
        })
        .unwrap_err();

    assert_eq!(
        error,
        StoreError::Invalid {
            entity: "account",
            reason: "the opening balance is in USD and the account holds GHS".to_owned(),
        }
    );
}

#[test]
fn a_row_for_an_account_that_does_not_exist_names_the_account() {
    let (store, _directory) = store();
    let missing = AccountId::from(Uuid::now_v7());

    let error = store
        .record_transaction(&spend(missing, 1_000))
        .unwrap_err();

    assert_eq!(
        error,
        StoreError::NotFound {
            entity: "account",
            id: missing.to_string(),
        }
    );
}

#[test]
fn a_label_the_model_refuses_is_refused_before_it_reaches_the_schema() {
    let (store, _directory) = store();
    let account = store.create_account(&wallet(0)).unwrap();

    let error = store
        .record_transaction(&NewTransaction {
            label: "   ".to_owned(),
            ..spend(account.id, 1_000)
        })
        .unwrap_err();

    match error {
        StoreError::Invalid { entity, reason } => {
            assert_eq!(entity, "transaction");
            assert!(reason.contains("label"), "{reason} does not name the field");
        }
        other => panic!("expected the model's own refusal, got {other:?}"),
    }
}

#[test]
fn unicode_survives_the_round_trip_intact() {
    let (store, _directory) = store();
    let account = store.create_account(&wallet(0)).unwrap();

    let written = store
        .record_transaction(&NewTransaction {
            label: "Kɔkɔɔ — ₵ fare 🚐".to_owned(),
            counterparty: Some("Akosua Nyamekyɛ".to_owned()),
            ..spend(account.id, 1_000)
        })
        .unwrap();
    let read = store.transactions_for_account(account.id).unwrap();

    assert_eq!(read.first(), Some(&written));
    assert_eq!(
        read.first().map(|row| row.label.as_str()),
        Some("Kɔkɔɔ — ₵ fare 🚐")
    );
}

#[test]
fn an_amount_at_the_ceiling_the_schema_allows_survives_the_round_trip() {
    let (store, _directory) = store();
    let account = store.create_account(&wallet(0)).unwrap();
    let ceiling = 1_000_000_000_000_000;

    let written = store
        .record_transaction(&spend(account.id, ceiling))
        .unwrap();

    assert_eq!(
        store
            .transactions_for_account(account.id)
            .unwrap()
            .first()
            .map(|row| row.amount),
        Some(written.amount)
    );
    assert_eq!(written.amount, cedis(ceiling));
}

#[test]
fn a_zero_amount_is_a_row_like_any_other() {
    let (store, _directory) = store();
    let account = store.create_account(&wallet(1_000)).unwrap();

    store.record_transaction(&spend(account.id, 0)).unwrap();

    assert_eq!(
        store
            .account_balance(account.id, BalanceBasis::Confirmed)
            .unwrap(),
        cedis(1_000)
    );
}

#[test]
fn a_booked_at_before_the_epoch_reads_back_as_written() {
    let (store, _directory) = store();
    let account = store.create_account(&wallet(0)).unwrap();
    let in_1968 = time::macros::datetime!(1968-03-06 11:30:00 UTC);

    let written = store
        .record_transaction(&NewTransaction {
            booked_at: in_1968,
            source: TxSource::Adjustment,
            ..spend(account.id, 1_000)
        })
        .unwrap();

    assert_eq!(written.booked_at, in_1968);
    assert_eq!(
        store
            .transactions_for_account(account.id)
            .unwrap()
            .first()
            .map(|row| row.booked_at),
        Some(in_1968)
    );
}

#[test]
fn the_row_handed_back_is_the_row_on_disk_even_when_the_clock_is_finer_than_the_column() {
    // A clock hands out microseconds; the column keeps milliseconds. If the row returned
    // from a write kept the microseconds, it would compare unequal against itself a moment
    // later — and `updated_at` is what last-write-wins resolves conflicts on.
    let clock = common::FixedClock::at(NOW + Duration::microseconds(230_865));
    let directory = tempfile::TempDir::new().unwrap();
    let store =
        mt_store::Store::open_with(&directory.path().join("ledger.db"), &common::key(), clock)
            .unwrap();
    let account = store.create_account(&wallet(0)).unwrap();

    let written = store
        .record_transaction(&NewTransaction {
            booked_at: NOW + Duration::microseconds(1_999),
            ..spend(account.id, 1_000)
        })
        .unwrap();
    let read_back = store.transactions_for_account(account.id).unwrap();

    assert_eq!(read_back.first(), Some(&written));
    assert_eq!(written.sync.updated_at, NOW + Duration::milliseconds(230));
    assert_eq!(written.booked_at, NOW + Duration::milliseconds(1));
    assert_eq!(store.account(account.id).unwrap(), account);
}
