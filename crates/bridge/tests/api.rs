//! The published surface, exercised the way Dart will call it.
//!
//! These tests do not re-prove the rules — `mt-core` has its own suite for the balance walk,
//! the ladder and the rest. What is proved here is that the surface hands the right values
//! in and the right values back: that the walk's shortfall survives the crossing, that a
//! generated row carries an id Dart can store, and that a refusal arrives as an error rather
//! than as a plausible wrong number.

#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

mod common;

use common::{
    NOW, TODAY, account, accra, budget, candidate, category, money, recurring, rule, transaction,
    uuid,
};
use mt_bridge::api::{
    MatchRulesDto, VerdictBasis, account_balances, categorise_candidate, compile_rules,
    currency_totals, evaluate_budgets, expand_recurrence, parse_amount, period_flows,
    score_duplicates, validate_transfer, walk_balance,
};
use mt_bridge::enums::{BalanceBasis, BudgetLevel, Direction, DuplicateSignal, TxStatus};
use mt_bridge::error::BridgeError;

// --------------------------------------------------------------------------- the walk ---

#[test]
fn a_walk_returns_the_running_balance_in_booked_order() {
    let wallet = account(1, 10_000);
    let rows = vec![
        transaction(3, &wallet, Direction::Out, 3_000, "2026-11-12T09:00:00Z"),
        transaction(2, &wallet, Direction::In, 5_000, "2026-11-11T09:00:00Z"),
    ];

    let walk = walk_balance(wallet, rows, BalanceBasis::Confirmed).unwrap();

    let balances: Vec<i64> = walk
        .entries
        .iter()
        .map(|entry| entry.balance_after.minor_units)
        .collect();
    assert_eq!(balances, vec![15_000, 12_000]);
    assert_eq!(walk.opening, money(10_000));
    assert_eq!(walk.closing, money(12_000));
    assert_eq!(walk.currency, "GHS");
    assert!(walk.first_shortfall.is_none());
}

#[test]
fn history_and_forecast_are_the_same_call() {
    let wallet = account(1, 10_000);
    let spent = transaction(2, &wallet, Direction::Out, 4_000, "2026-11-09T09:00:00Z");
    let mut expected = transaction(3, &wallet, Direction::Out, 9_000, "2026-11-28T09:00:00Z");
    expected.status = TxStatus::Pending;
    let rows = vec![spent, expected];

    let history = walk_balance(wallet.clone(), rows.clone(), BalanceBasis::Confirmed).unwrap();
    let outlook = walk_balance(wallet, rows, BalanceBasis::Projected).unwrap();

    // The confirmed walk stops at what has happened; the projected walk carries on into the
    // future-dated pending row and finds the day the wallet runs out.
    assert_eq!(history.entries.len(), 1);
    assert_eq!(history.closing, money(6_000));
    assert_eq!(outlook.entries.len(), 2);
    assert_eq!(outlook.closing, money(-3_000));

    let shortfall = outlook.first_shortfall.unwrap();
    assert_eq!(shortfall.transaction_id, uuid(3));
    assert_eq!(shortfall.booked_at, "2026-11-28T09:00:00Z");
    assert_eq!(shortfall.balance_after, money(-3_000));
}

#[test]
fn a_row_belonging_to_another_account_is_refused_rather_than_skipped() {
    let wallet = account(1, 10_000);
    let other = account(9, 0);
    let stray = transaction(2, &other, Direction::Out, 1_000, NOW);

    let error = walk_balance(wallet, vec![stray], BalanceBasis::Confirmed).unwrap_err();

    match error {
        BridgeError::Rule { rule, reason } => {
            assert_eq!(rule, "walk");
            assert!(reason.contains(&uuid(2)), "reason names no row: {reason}");
        }
        other => panic!("expected a refused walk, got {other:?}"),
    }
}

#[test]
fn balances_come_back_per_account_and_totals_per_currency() {
    let wallet = account(1, 10_000);
    let mut bank = account(2, 50_000);
    bank.name = "Bank".to_owned();
    let mut dollars = account(3, 20_000);
    dollars.name = "Dollars".to_owned();
    dollars.currency = "USD".to_owned();
    dollars.opening_balance.currency = "USD".to_owned();

    let rows = vec![transaction(4, &wallet, Direction::Out, 2_500, NOW)];
    let accounts = vec![wallet, bank, dollars];

    let balances = account_balances(accounts.clone(), rows.clone(), BalanceBasis::Confirmed)
        .unwrap()
        .into_iter()
        .map(|balance| (balance.account_id, balance.balance.minor_units))
        .collect::<Vec<_>>();
    let totals = currency_totals(accounts, rows, BalanceBasis::Confirmed).unwrap();

    assert!(balances.contains(&(uuid(1), 7_500)));
    assert!(balances.contains(&(uuid(2), 50_000)));
    assert!(balances.contains(&(uuid(3), 20_000)));

    // Two currencies, two figures. Never one converted number.
    assert_eq!(totals.len(), 2);
    assert!(
        totals
            .iter()
            .any(|total| total.currency == "GHS" && total.minor_units == 57_500)
    );
    assert!(
        totals
            .iter()
            .any(|total| total.currency == "USD" && total.minor_units == 20_000)
    );
}

#[test]
fn flows_drop_both_legs_of_a_transfer() {
    let wallet = account(1, 0);
    let mut out_leg = transaction(2, &wallet, Direction::Out, 5_000, NOW);
    let mut in_leg = transaction(3, &wallet, Direction::In, 5_000, NOW);
    out_leg.transfer_id = Some(uuid(0xaa));
    out_leg.category_id = None;
    in_leg.transfer_id = Some(uuid(0xaa));
    let salary = transaction(4, &wallet, Direction::In, 200_000, NOW);
    let lunch = transaction(5, &wallet, Direction::Out, 3_000, NOW);

    let flows = period_flows(
        "GHS".to_owned(),
        vec![out_leg, in_leg, salary, lunch],
        BalanceBasis::Confirmed,
    )
    .unwrap();

    assert_eq!(flows.income, money(200_000));
    assert_eq!(flows.spending, money(3_000));
    assert_eq!(flows.net, money(197_000));
}

// ------------------------------------------------------------------------- the budget ---

#[test]
fn a_budget_reports_where_spending_stands_against_its_cap() {
    let wallet = account(1, 100_000);
    let transport = category(2, "Transport");
    let mut fare = transaction(3, &wallet, Direction::Out, 45_000, "2026-11-05T09:00:00Z");
    fare.category_id = Some(transport.id.clone());

    let statuses = evaluate_budgets(
        vec![budget(4, 50_000, Some(&transport))],
        vec![transport],
        vec![fare],
        TODAY.to_owned(),
        BalanceBasis::Confirmed,
        accra(),
    )
    .unwrap();

    assert_eq!(statuses.len(), 1);
    let status = &statuses[0];
    assert_eq!(status.budget_id, uuid(4));
    assert_eq!(status.period_starts_on, "2026-11-01");
    assert_eq!(status.period_ends_on, "2026-11-30");
    assert_eq!(status.spent, money(45_000));
    assert_eq!(status.remaining, money(5_000));
    assert_eq!(status.used_bps, 9_000);
    assert_eq!(status.level, BudgetLevel::Warning);
}

#[test]
fn spending_the_whole_cap_counts_as_over_it() {
    let wallet = account(1, 100_000);
    let transport = category(2, "Transport");
    let mut fare = transaction(3, &wallet, Direction::Out, 50_000, "2026-11-05T09:00:00Z");
    fare.category_id = Some(transport.id.clone());

    let statuses = evaluate_budgets(
        vec![budget(4, 50_000, Some(&transport))],
        vec![transport],
        vec![fare],
        TODAY.to_owned(),
        BalanceBasis::Confirmed,
        accra(),
    )
    .unwrap();

    assert_eq!(statuses[0].level, BudgetLevel::Over);
    assert_eq!(statuses[0].remaining, money(0));
}

#[test]
fn a_budget_that_has_not_started_is_left_out_rather_than_reported_empty() {
    let next_year = mt_bridge::dto::BudgetDto {
        starts_on: "2027-01-01".to_owned(),
        ..budget(4, 50_000, None)
    };

    let statuses = evaluate_budgets(
        vec![next_year],
        Vec::new(),
        Vec::new(),
        TODAY.to_owned(),
        BalanceBasis::Confirmed,
        accra(),
    )
    .unwrap();

    assert!(statuses.is_empty());
}

// --------------------------------------------------------------------- the recurrence ---

#[test]
fn a_template_expands_into_pending_rows_with_fresh_ids() {
    let wallet = account(1, 500_000);
    let rent = recurring(2, &wallet, 120_000);

    let generation =
        expand_recurrence(rent, "2026-12-31".to_owned(), NOW.to_owned(), accra()).unwrap();

    assert_eq!(generation.generated_through, "2026-12-31");
    let booked: Vec<&str> = generation
        .rows
        .iter()
        .map(|row| row.booked_at.as_str())
        .collect();
    assert_eq!(
        booked,
        vec!["2026-11-15T00:00:00Z", "2026-12-15T00:00:00Z"],
        "rows land at local midnight in the rule's own zone"
    );

    for row in &generation.rows {
        assert_eq!(row.status, TxStatus::Pending);
        assert_eq!(row.recurring_id, Some(uuid(2)));
        assert_eq!(row.amount, money(120_000));
        assert!(row.to_core().is_ok(), "a generated row must be storable");
    }
    assert_ne!(
        generation.rows[0].id, generation.rows[1].id,
        "every generated row needs its own id"
    );
}

#[test]
fn a_template_already_generated_this_far_produces_nothing_a_second_time() {
    let wallet = account(1, 500_000);
    let rent = mt_bridge::dto::RecurringRuleDto {
        generated_through: Some("2026-12-31".to_owned()),
        ..recurring(2, &wallet, 120_000)
    };

    let generation =
        expand_recurrence(rent, "2026-12-31".to_owned(), NOW.to_owned(), accra()).unwrap();

    assert!(generation.rows.is_empty());
    assert_eq!(generation.generated_through, "2026-12-31");
}

// ----------------------------------------------------------------------- the ladder ---

#[test]
fn a_stored_pattern_categorises_a_candidate_and_says_which_rule_did_it() {
    let groceries = category(2, "Groceries");
    let melcom = rule(3, "melcom", &groceries);

    let verdict =
        categorise_candidate(candidate(12_000, NOW), vec![melcom], Vec::new(), None).unwrap();

    assert_eq!(verdict.category_id, Some(uuid(2)));
    assert_eq!(verdict.basis, VerdictBasis::Rule);
    assert_eq!(verdict.source_id, Some(uuid(3)));
    assert!(verdict.confidence_bps > 0);
    assert!(verdict.skipped_rules.is_empty());
}

#[test]
fn the_users_own_last_row_for_a_counterparty_categorises_the_next_one() {
    let wallet = account(1, 0);
    let groceries = category(2, "Groceries");
    let mut earlier = transaction(3, &wallet, Direction::Out, 8_000, "2026-10-01T09:00:00Z");
    earlier.counterparty = Some("Melcom".to_owned());
    earlier.category_id = Some(groceries.id.clone());

    let verdict =
        categorise_candidate(candidate(12_000, NOW), Vec::new(), vec![earlier], None).unwrap();

    assert_eq!(verdict.category_id, Some(uuid(2)));
    assert_eq!(verdict.basis, VerdictBasis::History);
    assert_eq!(verdict.source_id, Some(uuid(3)));
}

#[test]
fn nothing_matching_is_a_verdict_and_not_an_error() {
    let verdict =
        categorise_candidate(candidate(12_000, NOW), Vec::new(), Vec::new(), None).unwrap();

    assert_eq!(verdict.category_id, None);
    assert_eq!(verdict.basis, VerdictBasis::Unmatched);
    assert_eq!(verdict.confidence_bps, 0);
}

#[test]
fn a_stored_rule_that_no_longer_compiles_is_reported_rather_than_swallowed() {
    let groceries = category(2, "Groceries");
    let broken = mt_bridge::dto::CategoryRuleDto {
        is_regex: true,
        pattern: "melcom(".to_owned(),
        ..rule(3, "melcom(", &groceries)
    };

    let verdict =
        categorise_candidate(candidate(12_000, NOW), vec![broken], Vec::new(), None).unwrap();

    // The ladder still answers — it just answers without that rung, and says so, because a
    // rule that quietly stopped matching changes the answer with no sign on the screen.
    assert_eq!(verdict.basis, VerdictBasis::Unmatched);
    assert_eq!(verdict.skipped_rules.len(), 1);
    assert!(verdict.skipped_rules[0].contains(&uuid(3)));
}

#[test]
fn a_rule_that_will_not_compile_is_refused_at_save_time() {
    let groceries = category(2, "Groceries");
    let broken = mt_bridge::dto::CategoryRuleDto {
        is_regex: true,
        pattern: "melcom(".to_owned(),
        ..rule(3, "melcom(", &groceries)
    };

    let error = compile_rules(vec![broken]).unwrap_err();

    assert!(matches!(error, BridgeError::Rule { ref rule, .. } if rule == "category_rule"));
    assert!(compile_rules(vec![rule(4, "melcom", &groceries)]).is_ok());
}

// ------------------------------------------------------------------------- duplicates ---

#[test]
fn a_candidate_matching_an_existing_row_is_reported_and_never_merged() {
    let wallet = account(1, 0);
    let mut existing = transaction(2, &wallet, Direction::Out, 12_000, NOW);
    existing.counterparty = Some("Melcom".to_owned());
    let mut arriving = candidate(12_000, NOW);
    arriving.account_id = Some(wallet.id.clone());

    let reports = score_duplicates(vec![arriving], vec![existing], None).unwrap();

    assert_eq!(reports.len(), 1);
    let suspects = &reports[0].suspects;
    assert_eq!(suspects.len(), 1);
    assert_eq!(suspects[0].transaction_id, uuid(2));
    assert!(suspects[0].signals.contains(&DuplicateSignal::SameAmount));
    assert!(suspects[0].signals.contains(&DuplicateSignal::SameAccount));
    assert!(suspects[0].score_bps > 0);
}

#[test]
fn two_separate_fares_for_the_same_amount_days_apart_are_not_suspected() {
    let wallet = account(1, 0);
    let mut existing = transaction(2, &wallet, Direction::Out, 2_000, "2026-11-01T09:00:00Z");
    existing.counterparty = Some("Melcom".to_owned());
    let mut arriving = candidate(2_000, "2026-11-10T09:00:00Z");
    arriving.account_id = Some(wallet.id.clone());

    let reports = score_duplicates(vec![arriving], vec![existing.clone()], None).unwrap();
    let mut hour_later = candidate(2_000, "2026-11-01T10:00:00Z");
    hour_later.account_id = Some(wallet.id.clone());
    let same_day = score_duplicates(vec![hour_later], vec![existing], None).unwrap();

    // Nine days apart is two fares; an hour apart is one purchase arriving twice.
    assert!(reports[0].suspects.is_empty());
    assert_eq!(same_day[0].suspects.len(), 1);
}

#[test]
fn reports_come_back_in_candidate_order() {
    let wallet = account(1, 0);
    let mut existing = transaction(2, &wallet, Direction::Out, 12_000, NOW);
    existing.counterparty = Some("Melcom".to_owned());
    let unrelated = candidate(777, "2026-11-10T10:00:00Z");
    let matching = candidate(12_000, NOW);

    let reports = score_duplicates(vec![unrelated, matching], vec![existing], None).unwrap();

    assert_eq!(reports.len(), 2);
    assert!(reports[0].suspects.is_empty());
    assert_eq!(reports[1].suspects.len(), 1);
}

#[test]
fn a_caller_may_tighten_the_thresholds() {
    let wallet = account(1, 0);
    let mut existing = transaction(2, &wallet, Direction::Out, 12_000, NOW);
    existing.counterparty = Some("Melcom".to_owned());
    let arriving = candidate(12_000, "2026-11-10T20:00:00Z");

    let strict = MatchRulesDto {
        window_seconds: 60,
        close_window_seconds: 60,
        report_at_or_above_bps: 9_000,
    };
    let loose = score_duplicates(vec![arriving.clone()], vec![existing.clone()], None).unwrap();
    let reports = score_duplicates(vec![arriving], vec![existing], Some(strict)).unwrap();

    // The pair is a suspect on the core's own defaults, so the empty report below is the
    // tightened window doing the work rather than the rows never having matched.
    assert_eq!(loose[0].suspects.len(), 1);
    assert!(reports[0].suspects.is_empty());
}

// ----------------------------------------------------------------- the two refusals ---

#[test]
fn a_transfer_whose_legs_disagree_is_refused_before_it_is_written() {
    let wallet = account(1, 0);
    let mut bank = account(2, 0);
    bank.name = "Bank".to_owned();
    let mut out_leg = transaction(3, &wallet, Direction::Out, 5_000, NOW);
    let mut in_leg = transaction(4, &bank, Direction::In, 4_000, NOW);
    out_leg.transfer_id = Some(uuid(0xaa));
    in_leg.transfer_id = Some(uuid(0xaa));

    let error = validate_transfer(vec![out_leg.clone(), in_leg]).unwrap_err();
    assert!(matches!(error, BridgeError::InvalidRow { ref row, .. } if row == "transfer"));

    let mut balanced = transaction(5, &bank, Direction::In, 5_000, NOW);
    balanced.transfer_id = Some(uuid(0xaa));
    assert!(validate_transfer(vec![out_leg, balanced]).is_ok());
}

#[test]
fn typed_text_becomes_minor_units_without_ever_being_a_float() {
    assert_eq!(
        parse_amount("12.50".to_owned(), "GHS".to_owned()).unwrap(),
        money(1_250)
    );
    assert_eq!(
        parse_amount("0.05".to_owned(), "GHS".to_owned()).unwrap(),
        money(5)
    );

    // Three decimal places in a two-decimal currency is a typo, not a rounding problem.
    assert!(parse_amount("12.555".to_owned(), "GHS".to_owned()).is_err());
    assert!(parse_amount("1,200".to_owned(), "GHS".to_owned()).is_err());
    assert!(parse_amount("1e3".to_owned(), "GHS".to_owned()).is_err());
    assert!(matches!(
        parse_amount("12.50".to_owned(), "cedi".to_owned()).unwrap_err(),
        BridgeError::InvalidCurrency { .. }
    ));
}
