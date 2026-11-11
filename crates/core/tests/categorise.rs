//! The categorisation ladder: which rung answers, in what order, and what it refuses.

// Sealed like the library it tests: an integration test is its own crate root, and the
// workspace only denies unsafe, which an `allow` in this file would switch off.
#![forbid(unsafe_code)]

#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

mod common;

use common::{NOW, candidate, category_rule, days_after, id, transaction};
use mt_core::categorise::{
    Basis, CONFIDENCE_EXPLICIT, CONFIDENCE_HISTORY_COUNTERPARTY, CONFIDENCE_HISTORY_LABEL,
    CONFIDENCE_PACK_RULE, CONFIDENCE_USER_RULE, HistoryIndex, RuleError, RuleSet, Subject, apply,
    categorise,
};
use mt_core::enums::{MatchedField, RuleOrigin};
use mt_core::ids::{CategoryId, CategoryRuleId, RulePackId};
use mt_core::model::{CategoryRule, Transaction};

/// A rule that assigns `category` when `pattern` is found in the chosen field.
fn rule(
    pattern: &str,
    matched_field: MatchedField,
    category: CategoryId,
    priority: i32,
) -> CategoryRule {
    CategoryRule {
        pattern: pattern.to_owned(),
        matched_field,
        priority,
        ..category_rule(category)
    }
}

/// A categorised row in the user's history.
fn past(label: &str, counterparty: Option<&str>, category: CategoryId, days: i64) -> Transaction {
    Transaction {
        id: id(),
        label: label.to_owned(),
        counterparty: counterparty.map(str::to_owned),
        category_id: Some(category),
        booked_at: days_after(days),
        ..transaction()
    }
}

// ------------------------------------------------------------------------ rung 1: explicit ---

#[test]
fn a_category_the_source_named_outright_wins_without_consulting_anything() {
    let stated: CategoryId = id();
    let other: CategoryId = id();
    // A rule that would match, and history that would match, both pointing elsewhere.
    let rules = RuleSet::compile(&[rule("trotro", MatchedField::Label, other, 0)]).unwrap();
    let history = HistoryIndex::build(&[past("Trotro", None, other, -1)]);

    let subject = Subject {
        label: "Trotro",
        explicit: Some(stated),
        ..Subject::default()
    };
    let verdict = categorise(&subject, &rules, &history);

    assert_eq!(verdict.category_id, Some(stated));
    assert_eq!(verdict.basis, Basis::Explicit);
    assert_eq!(verdict.confidence_bps, CONFIDENCE_EXPLICIT);
}

// --------------------------------------------------------------------------- rung 2: rules ---

#[test]
fn a_matching_rule_assigns_its_category() {
    let transport: CategoryId = id();
    let rules = RuleSet::compile(&[rule("trotro", MatchedField::Label, transport, 0)]).unwrap();

    let subject = Subject {
        label: "Morning TROTRO to Circle",
        ..Subject::default()
    };
    let verdict = categorise(&subject, &rules, &HistoryIndex::default());

    assert_eq!(verdict.category_id, Some(transport));
    assert!(matches!(verdict.basis, Basis::Rule(_)));
    assert_eq!(verdict.confidence_bps, CONFIDENCE_USER_RULE);
}

#[test]
fn a_substring_rule_ignores_case_in_both_directions() {
    let transport: CategoryId = id();
    let rules = RuleSet::compile(&[rule("TroTro", MatchedField::Label, transport, 0)]).unwrap();

    for label in ["trotro", "TROTRO", "TroTro fare"] {
        let subject = Subject {
            label,
            ..Subject::default()
        };
        assert_eq!(
            categorise(&subject, &rules, &HistoryIndex::default()).category_id,
            Some(transport),
            "`{label}` should have matched"
        );
    }
}

#[test]
fn the_lowest_priority_number_is_tried_first() {
    let groceries: CategoryId = id();
    let eating_out: CategoryId = id();
    let rules = RuleSet::compile(&[
        rule("shoprite", MatchedField::Counterparty, eating_out, 50),
        rule("shop", MatchedField::Counterparty, groceries, 10),
    ])
    .unwrap();

    let subject = Subject {
        label: "Card payment",
        counterparty: Some("SHOPRITE"),
        ..Subject::default()
    };

    assert_eq!(
        categorise(&subject, &rules, &HistoryIndex::default()).category_id,
        Some(groceries)
    );
}

#[test]
fn two_rules_at_the_same_priority_pick_the_same_winner_on_every_device() {
    // Ties break on the rule id. Without that, the same statement imports differently on a
    // phone and on a laptop, and sync sees a disagreement neither side can explain.
    let first_category: CategoryId = id();
    let second_category: CategoryId = id();
    let low_id = CategoryRuleId::from_uuid(uuid::Uuid::from_u128(1));
    let high_id = CategoryRuleId::from_uuid(uuid::Uuid::from_u128(2));

    let make = |order: [CategoryRuleId; 2]| {
        vec![
            CategoryRule {
                id: order[0],
                ..rule("shop", MatchedField::Counterparty, first_category, 10)
            },
            CategoryRule {
                id: order[1],
                ..rule("shop", MatchedField::Counterparty, second_category, 10)
            },
        ]
    };

    let subject = Subject {
        label: "Card payment",
        counterparty: Some("SHOPRITE"),
        ..Subject::default()
    };

    let one_way = RuleSet::compile(&make([low_id, high_id])).unwrap();
    let other_way = RuleSet::compile(&make([high_id, low_id])).unwrap();

    assert_eq!(
        categorise(&subject, &one_way, &HistoryIndex::default()).category_id,
        Some(first_category)
    );
    assert_eq!(
        categorise(&subject, &other_way, &HistoryIndex::default()).category_id,
        Some(second_category)
    );
}

#[test]
fn a_rule_matches_only_the_field_it_names() {
    let groceries: CategoryId = id();
    let rules =
        RuleSet::compile(&[rule("shoprite", MatchedField::Counterparty, groceries, 0)]).unwrap();

    // The word is in the label, but the rule looks at the counterparty.
    let wrong_field = Subject {
        label: "Shoprite run",
        counterparty: Some("UNKNOWN"),
        ..Subject::default()
    };
    assert_eq!(
        categorise(&wrong_field, &rules, &HistoryIndex::default()).category_id,
        None
    );
}

#[test]
fn a_raw_text_rule_matches_only_while_the_message_is_still_in_memory() {
    // Raw SMS bodies are never stored, so a `raw_text` rule can only ever fire during the
    // parse. A stored candidate has no body and must not match.
    let airtime: CategoryId = id();
    let rules = RuleSet::compile(&[rule("recharge", MatchedField::RawText, airtime, 0)]).unwrap();

    let during_parse = Subject {
        label: "MTN",
        ..Subject::default()
    }
    .with_raw_text("You have recharged GHS 10.00 of airtime");
    let after_storage = Subject {
        label: "MTN",
        ..Subject::default()
    };

    assert_eq!(
        categorise(&during_parse, &rules, &HistoryIndex::default()).category_id,
        Some(airtime)
    );
    assert_eq!(
        categorise(&after_storage, &rules, &HistoryIndex::default()).category_id,
        None
    );
}

#[test]
fn a_regular_expression_rule_matches_what_it_describes() {
    let fuel: CategoryId = id();
    let pattern = r"^(shell|total|goil)\b";
    let rules = RuleSet::compile(&[CategoryRule {
        is_regex: true,
        ..rule(pattern, MatchedField::Counterparty, fuel, 0)
    }])
    .unwrap();

    let matching = Subject {
        label: "Fuel",
        counterparty: Some("GOIL Spintex"),
        ..Subject::default()
    };
    let not_matching = Subject {
        label: "Fuel",
        counterparty: Some("Totally Unrelated Ltd"),
        ..Subject::default()
    };

    assert_eq!(
        categorise(&matching, &rules, &HistoryIndex::default()).category_id,
        Some(fuel)
    );
    assert_eq!(
        categorise(&not_matching, &rules, &HistoryIndex::default()).category_id,
        None
    );
}

#[test]
fn a_rule_from_a_pack_is_trusted_a_little_less_than_the_users_own() {
    let category: CategoryId = id();
    let pack = CategoryRule {
        origin: RuleOrigin::Pack,
        pack_id: Some(RulePackId::from_uuid(uuid::Uuid::from_u128(7))),
        ..rule("shoprite", MatchedField::Counterparty, category, 0)
    };
    let rules = RuleSet::compile(&[pack]).unwrap();

    let subject = Subject {
        label: "Card payment",
        counterparty: Some("SHOPRITE"),
        ..Subject::default()
    };

    assert_eq!(
        categorise(&subject, &rules, &HistoryIndex::default()).confidence_bps,
        CONFIDENCE_PACK_RULE
    );
    const { assert!(CONFIDENCE_PACK_RULE < CONFIDENCE_USER_RULE) };
}

// ------------------------------------------------------------------- patterns fail loudly ---

#[test]
fn a_pattern_that_will_not_compile_is_refused_when_the_rule_is_saved() {
    // Never compiled-and-suppressed at match time. A rule that silently never fires shows up
    // a month later as a pile of uncategorised spending with no explanation.
    let bad = CategoryRule {
        is_regex: true,
        ..rule("(unclosed", MatchedField::Label, id(), 0)
    };
    let bad_id = bad.id;

    match RuleSet::compile(&[bad]) {
        Err(RuleError::BadPattern { rule, reason }) => {
            assert_eq!(rule, bad_id);
            assert!(!reason.is_empty(), "the error should quote the compiler");
        }
        other => panic!("expected a BadPattern error, got {other:?}"),
    }
}

#[test]
fn an_empty_pattern_is_refused_rather_than_matching_everything() {
    let blank = rule("   ", MatchedField::Label, id(), 0);

    assert!(matches!(
        RuleSet::compile(&[blank]),
        Err(RuleError::PatternLength { .. })
    ));
}

#[test]
fn a_disabled_rule_is_never_compiled_and_never_matches() {
    let category: CategoryId = id();
    let disabled = CategoryRule {
        enabled: false,
        is_regex: true,
        // Kept for reference, and no longer valid. Compiling it would refuse the whole set.
        ..rule("(unclosed", MatchedField::Label, category, 0)
    };

    let rules = RuleSet::compile(&[disabled]).unwrap();

    assert!(rules.is_empty());
}

#[test]
fn a_deleted_rule_is_dropped_too() {
    let category: CategoryId = id();
    let mut deleted = rule("trotro", MatchedField::Label, category, 0);
    deleted.sync.deleted_at = Some(NOW);

    let rules = RuleSet::compile(&[deleted]).unwrap();

    assert_eq!(rules.len(), 0);
}

// ------------------------------------------------------------------------- rung 3: history ---

#[test]
fn an_unmatched_row_takes_the_category_from_the_last_one_with_the_same_counterparty() {
    // The rung that makes this feel like it learns, without any of the machinery of learning.
    let groceries: CategoryId = id();
    let history = HistoryIndex::build(&[past("Card payment", Some("SHOPRITE"), groceries, -10)]);

    let subject = Subject {
        label: "Something else entirely",
        counterparty: Some("Shoprite"),
        ..Subject::default()
    };
    let verdict = categorise(&subject, &RuleSet::empty(), &history);

    assert_eq!(verdict.category_id, Some(groceries));
    assert!(matches!(verdict.basis, Basis::History(_)));
    assert_eq!(verdict.confidence_bps, CONFIDENCE_HISTORY_COUNTERPARTY);
}

#[test]
fn the_most_recent_row_wins_when_a_merchant_was_recategorised() {
    let old_category: CategoryId = id();
    let new_category: CategoryId = id();
    let history = HistoryIndex::build(&[
        past("Card payment", Some("SHOPRITE"), old_category, -30),
        past("Card payment", Some("SHOPRITE"), new_category, -2),
    ]);

    let subject = Subject {
        label: "Card payment",
        counterparty: Some("SHOPRITE"),
        ..Subject::default()
    };

    assert_eq!(
        categorise(&subject, &RuleSet::empty(), &history).category_id,
        Some(new_category)
    );
}

#[test]
fn a_counterparty_match_beats_a_label_match() {
    // "MTN" as a label covers airtime, data and sending money to a friend. The counterparty
    // is the merchant itself, so it is the stronger signal and is tried first.
    let by_label: CategoryId = id();
    let by_counterparty: CategoryId = id();
    let history = HistoryIndex::build(&[
        past("Payment", Some("SHOPRITE"), by_counterparty, -1),
        past("Payment", None, by_label, -1),
    ]);

    let subject = Subject {
        label: "Payment",
        counterparty: Some("SHOPRITE"),
        ..Subject::default()
    };
    let verdict = categorise(&subject, &RuleSet::empty(), &history);

    assert_eq!(verdict.category_id, Some(by_counterparty));
    assert_eq!(verdict.confidence_bps, CONFIDENCE_HISTORY_COUNTERPARTY);
}

#[test]
fn a_label_match_is_used_when_there_is_no_counterparty_to_go_on() {
    let transport: CategoryId = id();
    let history = HistoryIndex::build(&[past("Trotro", None, transport, -1)]);

    let subject = Subject {
        label: "trotro",
        ..Subject::default()
    };
    let verdict = categorise(&subject, &RuleSet::empty(), &history);

    assert_eq!(verdict.category_id, Some(transport));
    assert_eq!(verdict.confidence_bps, CONFIDENCE_HISTORY_LABEL);
    const { assert!(CONFIDENCE_HISTORY_LABEL < CONFIDENCE_HISTORY_COUNTERPARTY) };
}

#[test]
fn history_skips_rows_that_cannot_teach_anything() {
    let category: CategoryId = id();
    let mut deleted = past("Trotro", Some("DRIVER"), category, -1);
    deleted.sync.deleted_at = Some(NOW);
    let uncategorised = Transaction {
        category_id: None,
        ..past("Trotro", Some("DRIVER"), category, -1)
    };

    let history = HistoryIndex::build(&[deleted, uncategorised]);

    assert!(history.is_empty());
}

#[test]
fn a_transfer_leg_never_teaches_a_category_because_it_never_has_one() {
    let history = HistoryIndex::build(&[Transaction {
        transfer_id: Some(id()),
        category_id: None,
        ..past("To GCB", Some("GCB"), id(), -1)
    }]);

    assert!(history.is_empty());
}

// ------------------------------------------------------------------------ rung 4: nothing ---

#[test]
fn a_row_nothing_recognises_comes_back_uncategorised_with_no_confidence() {
    let verdict = categorise(
        &Subject {
            label: "Payment to a shop nobody has seen before",
            ..Subject::default()
        },
        &RuleSet::empty(),
        &HistoryIndex::default(),
    );

    assert_eq!(verdict.category_id, None);
    assert_eq!(verdict.basis, Basis::Unmatched);
    assert_eq!(verdict.confidence_bps, 0);
    assert!(!verdict.is_matched());
}

// ------------------------------------------------------------------------------ the whole ---

#[test]
fn the_ladder_stops_at_the_first_rung_that_answers() {
    let by_rule: CategoryId = id();
    let by_history: CategoryId = id();
    let rules =
        RuleSet::compile(&[rule("shoprite", MatchedField::Counterparty, by_rule, 0)]).unwrap();
    let history = HistoryIndex::build(&[past("Card payment", Some("SHOPRITE"), by_history, -1)]);

    let subject = Subject {
        label: "Card payment",
        counterparty: Some("SHOPRITE"),
        ..Subject::default()
    };

    // Rule beats history; history would have answered had the rule not.
    assert_eq!(
        categorise(&subject, &rules, &history).category_id,
        Some(by_rule)
    );
    assert_eq!(
        categorise(&subject, &RuleSet::empty(), &history).category_id,
        Some(by_history)
    );
}

#[test]
fn a_verdict_can_be_written_onto_the_candidate_the_inbox_will_show() {
    let groceries: CategoryId = id();
    let rules =
        RuleSet::compile(&[rule("shoprite", MatchedField::Counterparty, groceries, 0)]).unwrap();
    let mut candidate = candidate();

    let subject = Subject::from_candidate(&candidate);
    let verdict = categorise(&subject, &rules, &HistoryIndex::default());
    apply(&mut candidate, verdict);

    assert_eq!(candidate.category_id, Some(groceries));
    assert_eq!(candidate.category_confidence_bps, CONFIDENCE_USER_RULE);
    assert_eq!(candidate.validate(), Ok(()));
}

#[test]
fn a_candidate_that_already_names_a_category_climbs_no_further() {
    let stated: CategoryId = id();
    let mut candidate = candidate();
    candidate.category_id = Some(stated);

    let subject = Subject::from_candidate(&candidate);
    let verdict = categorise(&subject, &RuleSet::empty(), &HistoryIndex::default());

    assert_eq!(verdict.basis, Basis::Explicit);
    assert_eq!(verdict.category_id, Some(stated));
}

#[test]
fn a_pattern_that_compiles_to_something_enormous_is_refused_by_the_size_cap() {
    // Well inside the 500-character column limit, and syntactically valid: the defence here
    // is the compiled-size cap, not the parser. Patterns arrive from rule packs and over
    // sync, so an unbounded compile is an allocation requested from outside the app.
    let huge = CategoryRule {
        is_regex: true,
        ..rule("(?:a{1000}){1000}", MatchedField::Label, id(), 0)
    };

    assert!(matches!(
        RuleSet::compile(&[huge]),
        Err(RuleError::BadPattern { .. })
    ));
}

#[test]
fn loading_the_stored_set_skips_a_bad_rule_rather_than_losing_every_rule() {
    // Patterns reach the store over sync and from packs, and this build does not accept
    // every pattern stock `regex` does — the Unicode class tables are left out to keep the
    // WASM bundle down. One poisoned row must cost the user that one rule, not all of them.
    let transport: CategoryId = id();
    let bad = CategoryRule {
        is_regex: true,
        ..rule("(unclosed", MatchedField::Label, id(), 0)
    };
    let bad_id = bad.id;
    let good = rule("trotro", MatchedField::Label, transport, 10);

    let (rules, rejected) = RuleSet::compile_lenient(&[bad, good]);

    assert_eq!(rules.len(), 1);
    assert_eq!(rejected.len(), 1);
    assert!(matches!(
        rejected.first(),
        Some(RuleError::BadPattern { rule, .. }) if *rule == bad_id
    ));

    let subject = Subject {
        label: "Morning trotro",
        ..Subject::default()
    };
    assert_eq!(
        categorise(&subject, &rules, &HistoryIndex::default()).category_id,
        Some(transport)
    );
}

#[test]
fn a_unicode_class_this_build_leaves_out_is_reported_rather_than_matching_nothing() {
    // `\p{L}` compiles in a stock `regex` build and not in this one. The failure has to be
    // visible, because a rule that silently never fires looks exactly like a rule that is
    // working until a month of spending has piled up uncategorised.
    let authored_elsewhere = CategoryRule {
        is_regex: true,
        ..rule(r"^\p{L}+$", MatchedField::Counterparty, id(), 0)
    };

    let (rules, rejected) = RuleSet::compile_lenient(&[authored_elsewhere]);

    assert!(rules.is_empty());
    assert_eq!(rejected.len(), 1);
}

#[test]
fn a_strict_compile_still_refuses_the_whole_set_so_a_bad_save_cannot_go_through() {
    let bad = CategoryRule {
        is_regex: true,
        ..rule("(unclosed", MatchedField::Label, id(), 0)
    };
    let good = rule("trotro", MatchedField::Label, id(), 10);

    assert!(RuleSet::compile(&[bad, good]).is_err());
}
