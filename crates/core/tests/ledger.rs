//! The balance walk: ordering, what counts, and what it refuses to guess at.

// Sealed like the library it tests: an integration test is its own crate root, and the
// workspace only denies unsafe, which an `allow` in this file would switch off.
#![forbid(unsafe_code)]

#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

mod common;

use common::{GHS, NOW, USD, account, days_after, id, sync, transaction, tx_in};
use mt_core::enums::{Direction, TxStatus};
use mt_core::ids::{AccountId, TransferId, TxId};
use mt_core::ledger::{
    BalanceBasis, LedgerError, balances_by_account, flows, totals_by_currency, walk,
};
use mt_core::model::{Account, Transaction};
use mt_core::money::Money;
use time::Duration;

/// An account with a known opening balance, so every expected figure can be read off.
fn wallet(opening_minor: i64) -> Account {
    Account {
        opening_balance: Money::new(opening_minor, GHS),
        ..account()
    }
}

// ----------------------------------------------------------------------------- ordering ---

#[test]
fn an_account_with_no_rows_stands_at_its_opening_balance() {
    let account = wallet(10_000);
    let walked = walk(&account, &[], BalanceBasis::Confirmed).unwrap();

    assert_eq!(walked.opening, Money::new(10_000, GHS));
    assert_eq!(walked.closing, Money::new(10_000, GHS));
    assert!(walked.entries.is_empty());
    assert_eq!(walked.first_shortfall(), None);
}

#[test]
fn rows_are_walked_in_booked_order_whatever_order_they_arrive_in() {
    let account = wallet(10_000);
    // Deliberately out of order: rows arrive from sync in whatever order the server sent.
    let rows = vec![
        tx_in(account.id, Direction::Out, 3_000, days_after(2)),
        tx_in(account.id, Direction::In, 5_000, days_after(1)),
        tx_in(account.id, Direction::Out, 1_000, NOW),
    ];

    let walked = walk(&account, &rows, BalanceBasis::Confirmed).unwrap();
    let balances: Vec<i64> = walked
        .entries
        .iter()
        .map(|entry| entry.balance_after.minor())
        .collect();

    assert_eq!(balances, vec![9_000, 14_000, 11_000]);
    assert_eq!(walked.closing, Money::new(11_000, GHS));
}

#[test]
fn two_rows_booked_at_the_same_instant_walk_in_the_same_order_on_every_device() {
    let account = wallet(0);
    let first = TxId::from_uuid(uuid::Uuid::from_u128(1));
    let second = TxId::from_uuid(uuid::Uuid::from_u128(2));

    let rows = |order: [TxId; 2]| {
        vec![
            Transaction {
                id: order[0],
                ..tx_in(account.id, Direction::In, 100, NOW)
            },
            Transaction {
                id: order[1],
                ..tx_in(account.id, Direction::Out, 40, NOW)
            },
        ]
    };

    // The same two rows, handed in the two possible orders. The id tiebreak has to make the
    // intermediate balances identical, or two devices disagree about a running total that
    // neither of them is wrong about.
    let one_way = rows([first, second]);
    let other_way = rows([second, first]);

    let walked = walk(&account, &one_way, BalanceBasis::Confirmed).unwrap();
    let reversed = walk(&account, &other_way, BalanceBasis::Confirmed).unwrap();

    assert_eq!(walked.entries[0].transaction.id, first);
    assert_eq!(walked.entries[0].balance_after, Money::new(100, GHS));
    assert_eq!(reversed.entries[0].transaction.id, first);
    assert_eq!(reversed.entries[0].balance_after, Money::new(-40, GHS));
}

// -------------------------------------------------------------------------- what counts ---

#[test]
fn a_deleted_row_leaves_the_balance_alone() {
    let account = wallet(10_000);
    let mut deleted = tx_in(account.id, Direction::Out, 3_000, NOW);
    deleted.sync.deleted_at = Some(NOW);

    let rows = [deleted];
    let walked = walk(&account, &rows, BalanceBasis::Confirmed).unwrap();

    assert!(walked.entries.is_empty());
    assert_eq!(walked.closing, Money::new(10_000, GHS));
}

#[test]
fn the_confirmed_basis_ignores_pending_rows_and_the_projected_basis_does_not() {
    let account = wallet(10_000);
    let rows = vec![
        tx_in(account.id, Direction::Out, 1_000, NOW),
        Transaction {
            status: TxStatus::Pending,
            ..tx_in(account.id, Direction::Out, 4_000, days_after(30))
        },
    ];

    let settled = walk(&account, &rows, BalanceBasis::Confirmed).unwrap();
    let forecast = walk(&account, &rows, BalanceBasis::Projected).unwrap();

    assert_eq!(settled.closing, Money::new(9_000, GHS));
    assert_eq!(forecast.closing, Money::new(5_000, GHS));
}

#[test]
fn a_pending_row_in_the_past_counts_the_same_as_one_in_the_future() {
    // The walk sorts by date and accumulates. It has no notion of "today", which is exactly
    // why there is no second projection engine to keep in step with this one.
    let account = wallet(10_000);
    let rows = vec![Transaction {
        status: TxStatus::Pending,
        ..tx_in(account.id, Direction::Out, 4_000, days_after(-30))
    }];

    let forecast = walk(&account, &rows, BalanceBasis::Projected).unwrap();

    assert_eq!(forecast.closing, Money::new(6_000, GHS));
}

#[test]
fn both_legs_of_a_transfer_move_their_own_account() {
    // Transfers are netted for reporting, never for balances: the money really did leave one
    // account and arrive in the other.
    let from = wallet(10_000);
    let to = Account {
        id: id(),
        opening_balance: Money::new(0, GHS),
        ..account()
    };
    let transfer_id: TransferId = id();

    let out = Transaction {
        transfer_id: Some(transfer_id),
        ..tx_in(from.id, Direction::Out, 2_500, NOW)
    };
    let into = Transaction {
        id: id(),
        transfer_id: Some(transfer_id),
        ..tx_in(to.id, Direction::In, 2_500, NOW)
    };

    assert_eq!(
        walk(&from, &[out], BalanceBasis::Confirmed)
            .unwrap()
            .closing,
        Money::new(7_500, GHS)
    );
    assert_eq!(
        walk(&to, &[into], BalanceBasis::Confirmed).unwrap().closing,
        Money::new(2_500, GHS)
    );
}

// -------------------------------------------------------------------- refusals, not guesses ---

#[test]
fn a_row_belonging_to_another_account_is_an_error_rather_than_a_quiet_omission() {
    let account = wallet(0);
    let stranger: AccountId = id();
    let row = tx_in(stranger, Direction::Out, 1_000, NOW);
    let row_id = row.id;

    assert_eq!(
        walk(&account, &[row], BalanceBasis::Confirmed),
        Err(LedgerError::WrongAccount {
            transaction: row_id,
            expected: account.id,
            found: stranger,
        })
    );
}

#[test]
fn a_row_in_the_wrong_currency_is_refused() {
    let account = wallet(0);
    let row = Transaction {
        amount: Money::new(1_000, USD),
        ..tx_in(account.id, Direction::Out, 1_000, NOW)
    };

    assert!(matches!(
        walk(&account, &[row], BalanceBasis::Confirmed),
        Err(LedgerError::CurrencyMismatch { .. })
    ));
}

#[test]
fn a_running_total_that_leaves_i64_names_the_row_that_broke_it() {
    let account = wallet(i64::MAX);
    let row = tx_in(account.id, Direction::In, 1, NOW);
    let row_id = row.id;

    match walk(&account, &[row], BalanceBasis::Confirmed) {
        Err(LedgerError::Arithmetic { transaction, .. }) => assert_eq!(transaction, row_id),
        other => panic!("expected an arithmetic error naming the row, got {other:?}"),
    }
}

// ------------------------------------------------------------------------------ outlook ---

#[test]
fn the_first_shortfall_is_the_row_that_takes_the_account_under() {
    let account = wallet(5_000);
    let rows = vec![
        tx_in(account.id, Direction::Out, 2_000, NOW),
        tx_in(account.id, Direction::Out, 4_000, days_after(1)),
        tx_in(account.id, Direction::Out, 1_000, days_after(2)),
    ];

    let walked = walk(&account, &rows, BalanceBasis::Projected).unwrap();
    let shortfall = walked.first_shortfall().unwrap();

    assert_eq!(shortfall.transaction.booked_at, days_after(1));
    assert_eq!(shortfall.balance_after, Money::new(-1_000, GHS));
}

#[test]
fn an_account_that_never_goes_under_has_no_shortfall() {
    let account = wallet(100_000);
    let rows = vec![tx_in(account.id, Direction::Out, 2_000, NOW)];

    let walked = walk(&account, &rows, BalanceBasis::Projected).unwrap();

    assert_eq!(walked.first_shortfall(), None);
}

#[test]
fn the_balance_as_of_an_instant_counts_everything_up_to_and_including_it() {
    let account = wallet(10_000);
    let rows = vec![
        tx_in(account.id, Direction::Out, 1_000, NOW),
        tx_in(account.id, Direction::Out, 2_000, days_after(5)),
    ];
    let walked = walk(&account, &rows, BalanceBasis::Confirmed).unwrap();

    assert_eq!(
        walked.balance_as_of(NOW - Duration::seconds(1)),
        Money::new(10_000, GHS)
    );
    assert_eq!(walked.balance_as_of(NOW), Money::new(9_000, GHS));
    assert_eq!(walked.balance_as_of(days_after(4)), Money::new(9_000, GHS));
    assert_eq!(walked.balance_as_of(days_after(99)), Money::new(7_000, GHS));
}

#[test]
fn a_window_of_the_walk_holds_the_rows_booked_inside_it() {
    let account = wallet(0);
    let rows = vec![
        tx_in(account.id, Direction::Out, 1, NOW),
        tx_in(account.id, Direction::Out, 2, days_after(5)),
        tx_in(account.id, Direction::Out, 3, days_after(10)),
    ];
    let walked = walk(&account, &rows, BalanceBasis::Confirmed).unwrap();

    let middle = walked.between(days_after(1), days_after(9));

    assert_eq!(middle.len(), 1);
    assert_eq!(middle[0].transaction.amount, Money::new(2, GHS));
}

// ------------------------------------------------------------------------------- totals ---

#[test]
fn balances_are_worked_out_for_every_account_in_one_pass() {
    let momo = wallet(10_000);
    let bank = Account {
        id: id(),
        opening_balance: Money::new(50_000, GHS),
        ..account()
    };
    let rows = vec![
        tx_in(momo.id, Direction::Out, 1_000, NOW),
        tx_in(bank.id, Direction::In, 5_000, NOW),
    ];

    let balances = balances_by_account(
        &[momo.clone(), bank.clone()],
        &rows,
        BalanceBasis::Confirmed,
    )
    .unwrap();

    assert_eq!(balances.get(&momo.id), Some(&Money::new(9_000, GHS)));
    assert_eq!(balances.get(&bank.id), Some(&Money::new(55_000, GHS)));
}

#[test]
fn a_row_naming_an_account_that_was_not_supplied_is_an_error() {
    let momo = wallet(0);
    let stranger: AccountId = id();
    let rows = vec![tx_in(stranger, Direction::Out, 1_000, NOW)];

    assert!(matches!(
        balances_by_account(&[momo], &rows, BalanceBasis::Confirmed),
        Err(LedgerError::UnknownAccount { .. })
    ));
}

#[test]
fn a_deleted_account_gets_no_balance_and_its_rows_do_not_break_the_totals() {
    // A soft delete leaves the rows behind by design. Treating them as orphans would make
    // the totals screen fail the moment someone removed an old wallet.
    let live = wallet(10_000);
    let mut closed = Account {
        id: id(),
        opening_balance: Money::new(5_000, GHS),
        ..account()
    };
    closed.sync.deleted_at = Some(NOW);
    let rows = vec![
        tx_in(live.id, Direction::Out, 1_000, NOW),
        tx_in(closed.id, Direction::Out, 500, NOW),
    ];

    let balances = balances_by_account(
        &[live.clone(), closed.clone()],
        &rows,
        BalanceBasis::Confirmed,
    )
    .unwrap();
    let totals =
        totals_by_currency(&[live.clone(), closed], &rows, BalanceBasis::Confirmed).unwrap();

    assert_eq!(balances.len(), 1);
    assert_eq!(balances.get(&live.id), Some(&Money::new(9_000, GHS)));
    assert_eq!(totals.get(&GHS), Some(&Money::new(9_000, GHS)));
}

#[test]
fn totals_are_kept_apart_by_currency_rather_than_converted_into_one_number() {
    let cedis = wallet(10_000);
    let dollars = Account {
        id: id(),
        currency: USD,
        opening_balance: Money::new(20_000, USD),
        ..account()
    };

    let totals = totals_by_currency(&[cedis, dollars], &[], BalanceBasis::Confirmed).unwrap();

    assert_eq!(totals.len(), 2);
    assert_eq!(totals.get(&GHS), Some(&Money::new(10_000, GHS)));
    assert_eq!(totals.get(&USD), Some(&Money::new(20_000, USD)));
}

// -------------------------------------------------------------------------------- flows ---

#[test]
fn flows_separate_what_arrived_from_what_left() {
    let account = wallet(0);
    let rows = vec![
        tx_in(account.id, Direction::In, 100_000, NOW),
        tx_in(account.id, Direction::Out, 30_000, NOW),
        tx_in(account.id, Direction::Out, 5_000, NOW),
    ];

    let totals = flows(GHS, &rows, BalanceBasis::Confirmed).unwrap();

    assert_eq!(totals.income, Money::new(100_000, GHS));
    assert_eq!(totals.spending, Money::new(35_000, GHS));
    assert_eq!(totals.net, Money::new(65_000, GHS));
}

#[test]
fn moving_your_own_money_between_your_own_accounts_is_neither_income_nor_spending() {
    // The single most common way a personal-finance report ends up wrong.
    let account = wallet(0);
    let transfer_id: TransferId = id();
    let rows = vec![
        Transaction {
            transfer_id: Some(transfer_id),
            ..tx_in(account.id, Direction::Out, 50_000, NOW)
        },
        Transaction {
            id: id(),
            transfer_id: Some(transfer_id),
            ..tx_in(id(), Direction::In, 50_000, NOW)
        },
    ];

    let totals = flows(GHS, &rows, BalanceBasis::Confirmed).unwrap();

    assert_eq!(totals.income, Money::new(0, GHS));
    assert_eq!(totals.spending, Money::new(0, GHS));
    assert_eq!(totals.net, Money::new(0, GHS));
}

#[test]
fn flows_count_one_currency_at_a_time() {
    let rows = vec![
        Transaction {
            amount: Money::new(10_000, USD),
            ..transaction()
        },
        Transaction {
            id: id(),
            amount: Money::new(2_000, GHS),
            ..transaction()
        },
    ];

    let cedis = flows(GHS, &rows, BalanceBasis::Confirmed).unwrap();

    assert_eq!(cedis.spending, Money::new(2_000, GHS));
}

#[test]
fn flows_ignore_deleted_rows_and_respect_the_basis() {
    let account = wallet(0);
    let mut deleted = tx_in(account.id, Direction::Out, 1_000, NOW);
    deleted.sync.deleted_at = Some(NOW);
    let pending = Transaction {
        status: TxStatus::Pending,
        sync: sync(),
        ..tx_in(account.id, Direction::Out, 4_000, days_after(10))
    };
    let rows = vec![deleted, pending];

    let settled = flows(GHS, &rows, BalanceBasis::Confirmed).unwrap();
    let forecast = flows(GHS, &rows, BalanceBasis::Projected).unwrap();

    assert_eq!(settled.spending, Money::new(0, GHS));
    assert_eq!(forecast.spending, Money::new(4_000, GHS));
}
