//! Duplicate detection: what it suspects, how strongly, and what it refuses to be sure of.

// Sealed like the library it tests: an integration test is its own crate root, and the
// workspace only denies unsafe, which an `allow` in this file would switch off.
#![forbid(unsafe_code)]

#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

mod common;

use common::{GHS, NOW, USD, candidate, id, transaction};
use mt_core::dedup::{
    DuplicateIndex, MatchRules, SCORE_CERTAIN, SCORE_EXTERNAL_ID_UNPLACED, SCORE_WORTH_REVIEWING,
    Signal, suspects,
};
use mt_core::enums::Direction;
use mt_core::ids::AccountId;
use mt_core::model::{Candidate, Transaction};
use mt_core::money::Money;
use time::{Duration, OffsetDateTime};

/// An existing ledger row in a named account.
fn existing(
    account_id: AccountId,
    minor: i64,
    booked_at: OffsetDateTime,
    counterparty: Option<&str>,
) -> Transaction {
    Transaction {
        id: id(),
        account_id,
        direction: Direction::Out,
        amount: Money::new(minor, GHS),
        booked_at,
        label: "Card payment".to_owned(),
        counterparty: counterparty.map(str::to_owned),
        ..transaction()
    }
}

/// A parsed candidate for the same kind of movement.
fn parsed(
    account_id: Option<AccountId>,
    minor: i64,
    booked_at: OffsetDateTime,
    counterparty: Option<&str>,
) -> Candidate {
    Candidate {
        account_id,
        direction: Direction::Out,
        amount: Money::new(minor, GHS),
        booked_at,
        label: "Card payment".to_owned(),
        counterparty: counterparty.map(str::to_owned),
        ..candidate()
    }
}

// ------------------------------------------------------------------------- the hard gate ---

#[test]
fn a_different_amount_is_never_the_same_purchase() {
    let account = id();
    let rows = vec![existing(account, 2_000, NOW, Some("SHOPRITE"))];
    let candidate = parsed(Some(account), 2_001, NOW, Some("SHOPRITE"));

    assert!(suspects(&candidate, &rows, &MatchRules::default()).is_empty());
}

#[test]
fn the_same_number_in_a_different_currency_is_not_a_duplicate() {
    let account = id();
    let rows = vec![Transaction {
        amount: Money::new(2_000, USD),
        ..existing(account, 2_000, NOW, Some("SHOPRITE"))
    }];
    let candidate = parsed(Some(account), 2_000, NOW, Some("SHOPRITE"));

    assert!(suspects(&candidate, &rows, &MatchRules::default()).is_empty());
}

#[test]
fn money_in_never_duplicates_money_out() {
    let account = id();
    let rows = vec![Transaction {
        direction: Direction::In,
        ..existing(account, 2_000, NOW, Some("SHOPRITE"))
    }];
    let candidate = parsed(Some(account), 2_000, NOW, Some("SHOPRITE"));

    assert!(suspects(&candidate, &rows, &MatchRules::default()).is_empty());
}

#[test]
fn a_row_outside_the_window_is_not_offered_however_well_it_otherwise_matches() {
    let account = id();
    let rows = vec![existing(
        account,
        2_000,
        NOW - Duration::days(9),
        Some("SHOPRITE"),
    )];
    let candidate = parsed(Some(account), 2_000, NOW, Some("SHOPRITE"));

    assert!(suspects(&candidate, &rows, &MatchRules::default()).is_empty());
}

#[test]
fn a_deleted_row_stops_flagging_new_ones_as_duplicates_of_it() {
    let account = id();
    let mut deleted = existing(account, 2_000, NOW, Some("SHOPRITE"));
    deleted.sync.deleted_at = Some(NOW);
    let rows = vec![deleted];
    let candidate = parsed(Some(account), 2_000, NOW, Some("SHOPRITE"));

    assert!(suspects(&candidate, &rows, &MatchRules::default()).is_empty());
}

// ------------------------------------------------------------------------ the strong case ---

#[test]
fn the_same_purchase_arriving_by_sms_and_by_statement_is_flagged() {
    let account = id();
    // The statement line is dated midnight; the SMS came in at two in the afternoon.
    let rows = vec![existing(
        account,
        12_500,
        NOW.replace_time(time::Time::MIDNIGHT),
        Some("SHOPRITE ACCRA MALL"),
    )];
    let candidate = parsed(
        Some(account),
        12_500,
        NOW.replace_time(time::macros::time!(14:00:00)),
        Some("SHOPRITE"),
    );

    let found = suspects(&candidate, &rows, &MatchRules::default());

    assert_eq!(found.len(), 1);
    assert!(found[0].score_bps >= SCORE_WORTH_REVIEWING);
    assert!(found[0].signals.contains(&Signal::OverlappingCounterparty));
    assert!(found[0].signals.contains(&Signal::CloseInTime));
}

#[test]
fn an_identical_counterparty_scores_higher_than_a_merely_overlapping_one() {
    let account = id();
    let exact = vec![existing(account, 12_500, NOW, Some("SHOPRITE"))];
    let partial = vec![existing(account, 12_500, NOW, Some("SHOPRITE ACCRA MALL"))];
    let candidate = parsed(Some(account), 12_500, NOW, Some("Shoprite"));

    let exact_score = suspects(&candidate, &exact, &MatchRules::default())[0].score_bps;
    let partial_score = suspects(&candidate, &partial, &MatchRules::default())[0].score_bps;

    assert!(
        exact_score > partial_score,
        "exact {exact_score} should beat partial {partial_score}"
    );
}

#[test]
fn a_candidate_that_could_not_work_out_its_account_still_matches_on_the_rest() {
    let account = id();
    let rows = vec![existing(account, 12_500, NOW, Some("SHOPRITE"))];
    let no_account = parsed(None, 12_500, NOW, Some("SHOPRITE"));
    let with_account = parsed(Some(account), 12_500, NOW, Some("SHOPRITE"));

    let without = suspects(&no_account, &rows, &MatchRules::default());
    let with = suspects(&with_account, &rows, &MatchRules::default());

    assert_eq!(without.len(), 1);
    assert!(
        with[0].score_bps > without[0].score_bps,
        "naming the account should strengthen the suspicion"
    );
}

// --------------------------------------------------------------------------- the trotro case ---

#[test]
fn two_identical_fares_on_the_same_day_are_offered_for_review_but_never_called_certain() {
    // The case the plan singles out. Expensave merges on amount plus calendar date, which
    // would silently delete one of two real GHS 20 journeys. Here the pair is surfaced and
    // the user decides.
    let account = id();
    let rows = vec![existing(account, 2_000, NOW, None)];
    let second_fare = parsed(Some(account), 2_000, NOW + Duration::hours(3), None);

    let found = suspects(&second_fare, &rows, &MatchRules::default());

    assert_eq!(found.len(), 1, "the pair should be surfaced");
    assert!(
        !found[0].is_certain(),
        "amount and date alone must never be certain"
    );
    assert!(found[0].score_bps < SCORE_CERTAIN);
}

#[test]
fn nothing_the_weights_can_add_up_to_reaches_certainty() {
    // Every signal firing at once: same account, same moment, same counterparty, same label.
    let account = id();
    let rows = vec![existing(account, 12_500, NOW, Some("SHOPRITE"))];
    let candidate = parsed(Some(account), 12_500, NOW, Some("SHOPRITE"));

    let found = suspects(&candidate, &rows, &MatchRules::default());

    assert!(
        !found[0].is_certain(),
        "a guess must not be promoted to a certainty, however many signals agree"
    );
}

// ----------------------------------------------------------------------- the certain case ---

#[test]
fn the_institutions_own_reference_matching_on_both_sides_is_decisive() {
    let account = id();
    let rows = vec![Transaction {
        external_id: Some("MP260312.1431.A94821".to_owned()),
        // Deliberately a different day and a different merchant name: the reference wins.
        ..existing(account, 12_500, NOW - Duration::days(30), Some("SOMETHING"))
    }];
    let candidate = Candidate {
        external_id: Some("MP260312.1431.A94821".to_owned()),
        ..parsed(Some(account), 12_500, NOW, Some("SHOPRITE"))
    };

    let found = suspects(&candidate, &rows, &MatchRules::default());

    assert_eq!(found.len(), 1);
    assert!(found[0].is_certain());
    assert_eq!(found[0].score_bps, SCORE_CERTAIN);
    assert_eq!(found[0].signals, vec![Signal::SameExternalId]);
}

#[test]
fn a_reference_that_matches_nothing_falls_back_to_the_ordinary_signals() {
    let account = id();
    let rows = vec![existing(account, 12_500, NOW, Some("SHOPRITE"))];
    let candidate = Candidate {
        external_id: Some("A-REFERENCE-NOBODY-HAS".to_owned()),
        ..parsed(Some(account), 12_500, NOW, Some("SHOPRITE"))
    };

    let found = suspects(&candidate, &rows, &MatchRules::default());

    assert_eq!(found.len(), 1);
    assert!(!found[0].is_certain());
}

// ------------------------------------------------------------------------------- ranking ---

#[test]
fn suspicions_come_back_strongest_first() {
    let account = id();
    let rows = vec![
        // Weaker: nothing in the counterparty to go on.
        existing(account, 12_500, NOW, None),
        // Stronger: the merchant agrees too.
        existing(account, 12_500, NOW, Some("SHOPRITE")),
    ];
    let candidate = parsed(Some(account), 12_500, NOW, Some("SHOPRITE"));

    let found = suspects(&candidate, &rows, &MatchRules::default());

    assert_eq!(found.len(), 2);
    assert!(found[0].score_bps > found[1].score_bps);
    assert_eq!(found[0].transaction, rows[1].id);
}

#[test]
fn a_coincidence_too_weak_to_be_worth_asking_about_is_not_reported() {
    // Same amount, a couple of days apart, in someone else's account, with no merchant in
    // common. Technically possible, not worth interrupting the user over.
    let rows = vec![existing(
        id(),
        12_500,
        NOW - Duration::days(2),
        Some("SOMETHING ELSE"),
    )];
    let candidate = parsed(Some(id()), 12_500, NOW, Some("SHOPRITE"));

    assert!(suspects(&candidate, &rows, &MatchRules::default()).is_empty());
}

#[test]
fn a_raised_threshold_reports_only_the_strongest() {
    let account = id();
    let rows = vec![existing(account, 2_000, NOW, None)];
    let candidate = parsed(Some(account), 2_000, NOW, None);
    let strict = MatchRules {
        report_at_or_above: SCORE_CERTAIN - 1,
        ..MatchRules::default()
    };

    assert!(!suspects(&candidate, &rows, &MatchRules::default()).is_empty());
    assert!(suspects(&candidate, &rows, &strict).is_empty());
}

#[test]
fn a_widened_window_reaches_rows_the_default_would_not() {
    let account = id();
    let rows = vec![existing(
        account,
        12_500,
        NOW - Duration::days(5),
        Some("SHOPRITE"),
    )];
    let candidate = parsed(Some(account), 12_500, NOW, Some("SHOPRITE"));
    let generous = MatchRules {
        window: Duration::days(7),
        ..MatchRules::default()
    };

    assert!(suspects(&candidate, &rows, &MatchRules::default()).is_empty());
    assert_eq!(suspects(&candidate, &rows, &generous).len(), 1);
}

// --------------------------------------------------------------------------------- index ---

#[test]
fn one_index_serves_a_whole_import() {
    let account = id();
    let rows: Vec<Transaction> = (0..50)
        .map(|n| existing(account, 1_000 + n, NOW, Some("SHOPRITE")))
        .collect();

    let index = DuplicateIndex::build(&rows);
    assert_eq!(index.len(), 50);
    assert!(!index.is_empty());

    // Each candidate touches only the bucket for its own amount, not all fifty rows.
    let found = index.suspects(
        &parsed(Some(account), 1_007, NOW, Some("SHOPRITE")),
        &MatchRules::default(),
    );

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].transaction, rows[7].id);
}

#[test]
fn an_empty_ledger_suspects_nothing() {
    let index = DuplicateIndex::build(&[]);

    assert!(index.is_empty());
    assert!(
        index
            .suspects(&candidate(), &MatchRules::default())
            .is_empty()
    );
}

#[test]
fn a_counterparty_of_only_short_words_does_not_pair_everything_with_everything() {
    // Matching on "TO" would make every transfer a suspected duplicate of every other.
    let account = id();
    let rows = vec![existing(account, 2_000, NOW, Some("to AB"))];
    let candidate = parsed(Some(account), 2_000, NOW, Some("to CD"));

    let found = suspects(&candidate, &rows, &MatchRules::default());

    assert!(
        found.iter().all(|suspect| {
            !suspect.signals.contains(&Signal::SameCounterparty)
                && !suspect.signals.contains(&Signal::OverlappingCounterparty)
        }),
        "two-letter words must not count as agreement"
    );
}

#[test]
fn the_same_reference_in_two_different_accounts_is_not_the_same_purchase() {
    // Both schemas make `external_id` unique per account, not globally — statement lines are
    // commonly numbered from one, so a bank and a wallet can each hold a row called 0000412.
    // Treating those as certainly the same movement would have the inbox offer a pre-ticked
    // box that confirms away a real expense.
    let bank = id();
    let momo = id();
    let rows = vec![Transaction {
        external_id: Some("0000412".to_owned()),
        ..existing(bank, 12_500, NOW, Some("GCB"))
    }];
    let from_momo = Candidate {
        external_id: Some("0000412".to_owned()),
        ..parsed(Some(momo), 12_500, NOW, Some("SHOPRITE"))
    };

    let found = suspects(&from_momo, &rows, &MatchRules::default());

    assert!(
        found.iter().all(|suspect| !suspect.is_certain()),
        "a reference from another account must not be decisive"
    );
}

#[test]
fn a_reference_is_decisive_only_once_the_parser_knows_which_account_it_is_in() {
    let account = id();
    let rows = vec![Transaction {
        external_id: Some("MP260312.1431.A94821".to_owned()),
        ..existing(account, 12_500, NOW - Duration::days(30), Some("SOMETHING"))
    }];
    let reference = Some("MP260312.1431.A94821".to_owned());

    let account_unknown = Candidate {
        external_id: reference.clone(),
        ..parsed(None, 12_500, NOW, Some("SHOPRITE"))
    };
    let account_known = Candidate {
        external_id: reference,
        ..parsed(Some(account), 12_500, NOW, Some("SHOPRITE"))
    };

    // Knowing the account makes it decisive. Not knowing it must not throw the evidence
    // away: the reference is the strongest signal the system gets, and the scored path would
    // discard this pair outright on the thirty-day gap.
    let unplaced = suspects(&account_unknown, &rows, &MatchRules::default());
    assert_eq!(unplaced.len(), 1);
    assert!(!unplaced[0].is_certain());
    assert_eq!(unplaced[0].score_bps, SCORE_EXTERNAL_ID_UNPLACED);
    assert_eq!(unplaced[0].signals, vec![Signal::SameExternalId]);

    assert!(suspects(&account_known, &rows, &MatchRules::default())[0].is_certain());
}

#[test]
fn an_unplaced_reference_still_outranks_anything_the_parsed_fields_can_agree_on() {
    // A reference from the bank beats a label and a date that happen to line up, so when
    // both are on offer the reference is the one the inbox puts first.
    let account = id();
    let rows = vec![
        Transaction {
            external_id: Some("MP260312.1431.A94821".to_owned()),
            ..existing(account, 12_500, NOW - Duration::days(30), Some("SOMETHING"))
        },
        existing(account, 12_500, NOW, Some("SHOPRITE")),
    ];
    let candidate = Candidate {
        external_id: Some("MP260312.1431.A94821".to_owned()),
        ..parsed(None, 12_500, NOW, Some("SHOPRITE"))
    };

    let found = suspects(&candidate, &rows, &MatchRules::default());

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].transaction, rows[0].id);
    assert_eq!(found[0].score_bps, SCORE_EXTERNAL_ID_UNPLACED);
}

#[test]
fn a_reference_match_is_offered_however_high_the_threshold_is_set() {
    // It is not a guess that can be tuned out; a caller raising the bar was raising it
    // against the scored signals.
    let account = id();
    let rows = vec![Transaction {
        external_id: Some("MP260312.1431.A94821".to_owned()),
        ..existing(account, 12_500, NOW, Some("SHOPRITE"))
    }];
    let candidate = Candidate {
        external_id: Some("MP260312.1431.A94821".to_owned()),
        ..parsed(Some(account), 12_500, NOW, Some("SHOPRITE"))
    };
    let strict = MatchRules {
        report_at_or_above: u16::MAX,
        ..MatchRules::default()
    };

    assert_eq!(suspects(&candidate, &rows, &strict).len(), 1);
}

#[test]
fn a_reference_the_ledger_already_holds_twice_offers_both_rows() {
    // Two rows carrying one reference is exactly the mess the user needs shown to them, so
    // offering only whichever was indexed last would hide half of it.
    let account = id();
    let reference = "0000412";
    let rows = vec![
        Transaction {
            external_id: Some(reference.to_owned()),
            ..existing(account, 12_500, NOW, Some("GCB"))
        },
        Transaction {
            external_id: Some(reference.to_owned()),
            ..existing(account, 12_500, NOW, Some("GCB"))
        },
    ];
    let candidate = Candidate {
        external_id: Some(reference.to_owned()),
        ..parsed(Some(account), 12_500, NOW, Some("GCB"))
    };

    let found = suspects(&candidate, &rows, &MatchRules::default());

    assert_eq!(found.len(), 2);
    assert!(found.iter().all(mt_core::dedup::Suspect::is_certain));
}
