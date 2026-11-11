//! Validation and sign behaviour of the row types, and the text spellings the schema stores.

// Sealed like the library it tests: an integration test is its own crate root, and the
// workspace only denies unsafe, which an `allow` in this file would switch off.
#![forbid(unsafe_code)]

#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::str::FromStr as _;

use common::{
    GHS, NOW, USD, account, attachment, budget, candidate, category, category_rule, id, inbox_item,
    recurring, sync, transaction, transfer_pair,
};
use mt_core::enums::{
    AccountKind, BudgetPeriod, CategoryKind, Direction, InboxSource, InboxStatus, MatchedField,
    RecurrenceFreq, RecurrenceScope, RuleOrigin, TxSource, TxStatus,
};
use mt_core::ids::{CategoryId, TransferId, TxId};
use mt_core::model::{
    MAX_AMOUNT_MINOR, MAX_LABEL_LEN, MAX_NAME_LEN, ValidationError, validate_transfer_pair,
};
use mt_core::money::Money;
use time::Duration;
use time::macros::{date, datetime};

// ------------------------------------------------------------------------- text fields ----

#[test]
fn a_well_formed_account_validates() {
    assert_eq!(account().validate(), Ok(()));
}

#[test]
fn an_account_name_must_be_present_and_within_the_column_width() {
    let mut blank = account();
    blank.name = "   ".to_owned();
    assert_eq!(
        blank.validate(),
        Err(ValidationError::Empty {
            field: "account.name"
        })
    );

    let mut long = account();
    long.name = "a".repeat(MAX_NAME_LEN + 1);
    assert_eq!(
        long.validate(),
        Err(ValidationError::TooLong {
            field: "account.name",
            max: MAX_NAME_LEN,
            actual: MAX_NAME_LEN + 1
        })
    );
}

#[test]
fn a_name_is_trimmed_before_it_is_measured_just_as_both_databases_do() {
    // Both schemas measure `length(trim(name))` / `char_length(btrim(name))`. If this crate
    // measured the untrimmed value, a name accepted on the device would be refused by the
    // server on push, with nothing in the error to say why.
    let mut padded = account();
    padded.name = format!("{}  ", "a".repeat(MAX_NAME_LEN));
    assert_eq!(padded.validate(), Ok(()));
}

#[test]
fn a_name_is_measured_in_characters_not_bytes() {
    let mut named = account();
    // Each of these is three bytes and one character.
    named.name = "\u{20b5}".repeat(MAX_NAME_LEN);
    assert_eq!(named.validate(), Ok(()));
}

#[test]
fn transaction_text_fields_are_bounded() {
    let mut long_label = transaction();
    long_label.label = "x".repeat(MAX_LABEL_LEN + 1);
    assert!(matches!(
        long_label.validate(),
        Err(ValidationError::TooLong { .. })
    ));

    let mut blank_counterparty = transaction();
    blank_counterparty.counterparty = Some(String::new());
    assert_eq!(
        blank_counterparty.validate(),
        Err(ValidationError::Empty {
            field: "transaction.counterparty"
        })
    );

    let mut long_reference = transaction();
    long_reference.external_id = Some("x".repeat(500));
    assert!(matches!(
        long_reference.validate(),
        Err(ValidationError::TooLong {
            field: "transaction.external_id",
            ..
        })
    ));

    let mut absent = transaction();
    absent.counterparty = None;
    absent.note = None;
    assert_eq!(absent.validate(), Ok(()));
}

// ------------------------------------------------------------------------------- money ----

#[test]
fn direction_carries_the_sign_not_the_amount() {
    let mut spent = transaction();
    spent.direction = Direction::Out;
    assert_eq!(spent.signed_amount().unwrap(), Money::new(-2000, GHS));

    let mut received = transaction();
    received.direction = Direction::In;
    assert_eq!(received.signed_amount().unwrap(), Money::new(2000, GHS));
}

#[test]
fn a_negative_amount_is_refused_because_the_sign_lives_in_direction() {
    let mut row = transaction();
    row.amount = Money::new(-1, GHS);
    assert_eq!(
        row.validate(),
        Err(ValidationError::NegativeAmount {
            field: "transaction.amount"
        })
    );
}

#[test]
fn a_zero_amount_is_allowed() {
    let mut row = transaction();
    row.amount = Money::zero(GHS);
    assert_eq!(row.validate(), Ok(()));
    assert_eq!(row.signed_amount().unwrap(), Money::zero(GHS));
}

#[test]
fn an_amount_that_would_overflow_a_balance_walk_is_refused() {
    // One row near the i64 ceiling makes every walk over its account return Overflow, with
    // nothing in the error to say which row caused it.
    let mut absurd = transaction();
    absurd.amount = Money::new(i64::MAX, GHS);
    assert_eq!(
        absurd.validate(),
        Err(ValidationError::AmountTooLarge {
            field: "transaction.amount",
            minor: i64::MAX,
            max: MAX_AMOUNT_MINOR,
        })
    );

    let mut at_the_limit = transaction();
    at_the_limit.amount = Money::new(MAX_AMOUNT_MINOR, GHS);
    assert_eq!(at_the_limit.validate(), Ok(()));
}

#[test]
fn an_opening_balance_must_be_in_the_accounts_own_currency() {
    let mut mixed = account();
    mixed.opening_balance = Money::new(1000, USD);
    assert_eq!(
        mixed.validate(),
        Err(ValidationError::CurrencyMismatch {
            field: "account.opening_balance",
            expected: GHS,
            found: USD
        })
    );
}

#[test]
fn a_row_must_be_denominated_in_its_accounts_currency() {
    // Both schemas enforce this with a composite foreign key. This is the check that runs
    // before the write, where the error can name the currencies.
    let account = account();
    let mut row = transaction();
    row.account_id = account.id;
    assert_eq!(row.validate_against(&account), Ok(()));

    row.amount = Money::new(2000, USD);
    assert_eq!(
        row.validate_against(&account),
        Err(ValidationError::CurrencyMismatch {
            field: "transaction.amount",
            expected: GHS,
            found: USD
        })
    );
}

// ---------------------------------------------------------------------------- timeline ----

#[test]
fn a_row_edited_before_it_was_created_is_refused() {
    // In a last-write-wins system an inverted updated_at is exactly what makes a conflict
    // impossible to explain afterwards.
    let mut backwards = account();
    backwards.sync.updated_at = NOW - Duration::hours(1);
    assert_eq!(
        backwards.validate(),
        Err(ValidationError::TimestampBeforeCreation {
            field: "updated_at"
        })
    );

    let mut deleted_too_early = account();
    deleted_too_early.sync.deleted_at = Some(NOW - Duration::hours(1));
    assert_eq!(
        deleted_too_early.validate(),
        Err(ValidationError::TimestampBeforeCreation {
            field: "deleted_at"
        })
    );
}

#[test]
fn a_booking_date_outside_the_ledgers_window_is_refused() {
    let mut far_future = transaction();
    far_future.booked_at = datetime!(2500-01-01 00:00:00 UTC);
    assert_eq!(
        far_future.validate(),
        Err(ValidationError::TimestampOutOfRange {
            field: "transaction.booked_at"
        })
    );

    // Future-dated rows are the forecast, so next year must still be fine.
    let mut next_year = transaction();
    next_year.booked_at = datetime!(2027-11-10 09:00:00 UTC);
    assert_eq!(next_year.validate(), Ok(()));
}

#[test]
fn a_soft_deleted_row_reports_itself_as_deleted() {
    let mut meta = sync();
    assert!(!meta.is_deleted());
    meta.deleted_at = Some(NOW);
    assert!(meta.is_deleted());
}

// --------------------------------------------------------------------------- transfers ----

#[test]
fn a_transfer_leg_is_recognised_and_may_not_be_categorised() {
    let mut leg = transaction();
    leg.transfer_id = Some(TransferId::from_uuid(*id::<TxId>().as_uuid()));
    assert!(leg.is_transfer());
    assert_eq!(leg.validate(), Ok(()));

    leg.category_id = Some(id::<CategoryId>());
    assert_eq!(leg.validate(), Err(ValidationError::CategorisedTransfer));
}

#[test]
fn a_well_formed_transfer_pair_validates() {
    assert_eq!(validate_transfer_pair(&transfer_pair()), Ok(()));
}

#[test]
fn a_transfer_must_be_exactly_two_legs() {
    // Three outbound legs sharing a transfer_id would net to a single move and under-report
    // the spending in every report that uses the walk.
    let [out, into] = transfer_pair();
    let third = out.clone();
    assert!(matches!(
        validate_transfer_pair(&[out.clone(), into.clone(), third]),
        Err(ValidationError::MalformedTransfer(_))
    ));
    assert!(matches!(
        validate_transfer_pair(&[out]),
        Err(ValidationError::MalformedTransfer(_))
    ));
    assert!(matches!(
        validate_transfer_pair(&[]),
        Err(ValidationError::MalformedTransfer(_))
    ));
    let _ = into;
}

#[test]
fn both_legs_of_a_transfer_must_agree_on_everything_that_makes_it_one_move() {
    let cases: Vec<(&str, [_; 2])> = vec![
        ("same direction", {
            let [mut out, into] = transfer_pair();
            out.direction = into.direction;
            [out, into]
        }),
        ("same account", {
            let [out, mut into] = transfer_pair();
            into.account_id = out.account_id;
            [out, into]
        }),
        ("different amounts", {
            let [out, mut into] = transfer_pair();
            into.amount = Money::new(999, GHS);
            [out, into]
        }),
        ("different instants", {
            let [out, mut into] = transfer_pair();
            into.booked_at = NOW + Duration::days(1);
            [out, into]
        }),
        ("different transfer ids", {
            let [out, mut into] = transfer_pair();
            into.transfer_id = Some(TransferId::from_uuid(*id::<TxId>().as_uuid()));
            [out, into]
        }),
        ("one leg not marked as a transfer", {
            let [out, mut into] = transfer_pair();
            into.transfer_id = None;
            [out, into]
        }),
    ];

    for (what, legs) in cases {
        assert!(
            matches!(
                validate_transfer_pair(&legs),
                Err(ValidationError::MalformedTransfer(_))
            ),
            "a transfer with {what} should be refused"
        );
    }
}

#[test]
fn a_cross_currency_transfer_is_refused_rather_than_netted_at_an_invented_rate() {
    let [out, mut into] = transfer_pair();
    into.amount = Money::new(2000, USD);
    assert!(matches!(
        validate_transfer_pair(&[out, into]),
        Err(ValidationError::MalformedTransfer(_))
    ));
}

// -------------------------------------------------------------------------- categories ----

#[test]
fn a_category_may_not_be_its_own_parent() {
    let mut category = category();
    category.parent_id = Some(category.id);
    assert_eq!(
        category.validate(),
        Err(ValidationError::SelfReference {
            field: "category.parent_id"
        })
    );
}

#[test]
fn categories_nest_one_level_only() {
    let root = category();
    let mut child = category();
    child.parent_id = Some(root.id);
    assert_eq!(child.validate_under(Some(&root)), Ok(()));

    let mut grandchild = category();
    grandchild.parent_id = Some(child.id);
    assert!(matches!(
        grandchild.validate_under(Some(&child)),
        Err(ValidationError::NestedTooDeep { .. })
    ));
}

#[test]
fn a_child_category_must_be_checked_against_the_parent_it_names() {
    let root = category();
    let other = category();
    let mut child = category();
    child.parent_id = Some(root.id);

    assert!(child.validate_under(Some(&other)).is_err());
    assert!(child.validate_under(None).is_err());

    let orphan = category();
    assert_eq!(orphan.validate_under(None), Ok(()));
}

#[test]
fn a_pack_rule_must_name_its_pack() {
    let category = category();
    let mut rule = category_rule(category.id);
    assert_eq!(rule.validate(), Ok(()));

    rule.origin = RuleOrigin::Pack;
    assert_eq!(
        rule.validate(),
        Err(ValidationError::SelfReference {
            field: "category_rule.pack_id"
        })
    );

    let mut blank = category_rule(category.id);
    blank.pattern = "  ".to_owned();
    assert_eq!(
        blank.validate(),
        Err(ValidationError::Empty {
            field: "category_rule.pattern"
        })
    );
}

// -------------------------------------------------------------------------- recurrence ----

#[test]
fn a_recurring_template_needs_an_interval_and_a_real_day_of_month() {
    let base = recurring();
    assert_eq!(base.validate(), Ok(()));

    let mut never_advances = base.clone();
    never_advances.interval = 0;
    assert_eq!(
        never_advances.validate(),
        Err(ValidationError::InvalidInterval)
    );

    for day in [0_u8, 32] {
        let mut bad_day = base.clone();
        bad_day.by_month_day = Some(day);
        assert_eq!(
            bad_day.validate(),
            Err(ValidationError::InvalidMonthDay(day))
        );
    }

    let mut backwards = base;
    backwards.ends_on = Some(date!(2026 - 11 - 01));
    assert_eq!(
        backwards.validate(),
        Err(ValidationError::EndsBeforeStart { field: "recurring" })
    );
}

#[test]
fn a_day_of_the_week_outside_the_week_is_refused() {
    let mut rule = recurring();
    rule.freq = RecurrenceFreq::Weekly;
    rule.by_month_day = None;
    rule.by_weekday = Some(9);
    assert_eq!(rule.validate(), Err(ValidationError::InvalidWeekday(9)));
}

#[test]
fn a_cadence_and_its_day_fields_must_agree() {
    // A weekly rule carrying a day of the month, or a monthly rule with no day at all,
    // generates rows on a date nobody asked for.
    let weekly_with_month_day = RecurringRuleCase::new(RecurrenceFreq::Weekly, Some(15), Some(1));
    let weekly_without_weekday = RecurringRuleCase::new(RecurrenceFreq::Weekly, None, None);
    let monthly_without_day = RecurringRuleCase::new(RecurrenceFreq::Monthly, None, None);
    let monthly_with_weekday = RecurringRuleCase::new(RecurrenceFreq::Monthly, Some(1), Some(3));
    let daily_with_a_day = RecurringRuleCase::new(RecurrenceFreq::Daily, Some(1), None);

    for case in [
        weekly_with_month_day,
        weekly_without_weekday,
        monthly_without_day,
        monthly_with_weekday,
        daily_with_a_day,
    ] {
        assert!(
            matches!(
                case.0.validate(),
                Err(ValidationError::CadenceMismatch { .. })
            ),
            "{:?} with month day {:?} and weekday {:?} should be refused",
            case.0.freq,
            case.0.by_month_day,
            case.0.by_weekday
        );
    }

    let mut daily = recurring();
    daily.freq = RecurrenceFreq::Daily;
    daily.by_month_day = None;
    assert_eq!(daily.validate(), Ok(()));
}

struct RecurringRuleCase(mt_core::model::RecurringRule);

impl RecurringRuleCase {
    fn new(freq: RecurrenceFreq, by_month_day: Option<u8>, by_weekday: Option<u8>) -> Self {
        let mut rule = recurring();
        rule.freq = freq;
        rule.by_month_day = by_month_day;
        rule.by_weekday = by_weekday;
        Self(rule)
    }
}

#[test]
fn a_recurrence_without_a_timezone_is_refused() {
    // A monthly bill reckoned in an empty zone lands on the wrong local day.
    let mut rule = recurring();
    rule.timezone = String::new();
    assert_eq!(
        rule.validate(),
        Err(ValidationError::Empty {
            field: "recurring.timezone"
        })
    );
}

// ----------------------------------------------------------------------------- budgets ----

#[test]
fn a_budget_needs_a_positive_cap_and_a_sane_window() {
    let base = budget();
    assert_eq!(base.validate(), Ok(()));

    let mut zero_cap = base.clone();
    zero_cap.cap = Money::zero(GHS);
    assert_eq!(
        zero_cap.validate(),
        Err(ValidationError::NotPositive {
            field: "budget.cap"
        })
    );

    let mut backwards = base.clone();
    backwards.ends_on = Some(date!(2025 - 10 - 01));
    assert_eq!(
        backwards.validate(),
        Err(ValidationError::EndsBeforeStart { field: "budget" })
    );

    let mut impossible_warning = base;
    impossible_warning.warn_at_bps = 10_001;
    assert_eq!(
        impossible_warning.validate(),
        Err(ValidationError::BasisPointsOutOfRange {
            field: "budget.warn_at_bps",
            value: 10_001,
            max: 10_000,
        })
    );
}

#[test]
fn two_live_budgets_cannot_answer_for_the_same_spending() {
    let existing = budget();
    let mut overlapping = budget();
    overlapping.starts_on = date!(2026 - 06 - 01);
    overlapping.ends_on = None;

    assert!(overlapping.conflicts_with(&existing));
    assert!(matches!(
        overlapping.validate_against(std::slice::from_ref(&existing)),
        Err(ValidationError::SelfReference {
            field: "budget.starts_on"
        })
    ));
}

#[test]
fn next_years_cap_can_be_set_while_this_years_is_still_live() {
    // The reason this is an overlap rule and not a unique key: raising a cap must not mean
    // deleting the one last quarter's verdict was measured against.
    let existing = budget();
    let mut next_year = budget();
    next_year.starts_on = date!(2027 - 01 - 01);
    next_year.ends_on = None;

    assert!(!next_year.conflicts_with(&existing));
    assert_eq!(
        next_year.validate_against(std::slice::from_ref(&existing)),
        Ok(())
    );
}

#[test]
fn a_cap_in_another_currency_is_a_separate_budget_not_a_conflict() {
    let existing = budget();
    let mut in_dollars = budget();
    in_dollars.cap = Money::new(10_000, USD);
    assert!(!in_dollars.conflicts_with(&existing));
}

#[test]
fn a_deleted_budget_no_longer_occupies_its_scope() {
    let mut deleted = budget();
    deleted.sync.deleted_at = Some(NOW);
    let replacement = budget();
    assert!(!replacement.conflicts_with(&deleted));
}

// ------------------------------------------------------------------------------- inbox ----

#[test]
fn a_pending_inbox_item_validates() {
    assert_eq!(inbox_item().validate(), Ok(()));
}

#[test]
fn an_inbox_items_status_and_its_links_must_agree() {
    let mut accepted = inbox_item();
    accepted.status = InboxStatus::Accepted;
    assert!(matches!(
        accepted.validate(),
        Err(ValidationError::InboxStateMismatch { .. })
    ));
    accepted.transaction_id = Some(id());
    assert_eq!(accepted.validate(), Ok(()));

    let mut merged = inbox_item();
    merged.status = InboxStatus::Merged;
    assert!(matches!(
        merged.validate(),
        Err(ValidationError::InboxStateMismatch { .. })
    ));
    merged.duplicate_of = Some(id());
    assert_eq!(merged.validate(), Ok(()));

    let mut pending_but_linked = inbox_item();
    pending_but_linked.transaction_id = Some(id());
    assert!(matches!(
        pending_but_linked.validate(),
        Err(ValidationError::InboxStateMismatch { .. })
    ));
}

#[test]
fn a_reported_balance_must_match_the_candidates_currency() {
    let mut parsed = candidate();
    parsed.reported_balance = Some(Money::new(10_000, USD));
    assert_eq!(
        parsed.validate(),
        Err(ValidationError::CurrencyMismatch {
            field: "candidate.reported_balance",
            expected: GHS,
            found: USD
        })
    );

    parsed.reported_balance = Some(Money::new(10_000, GHS));
    assert_eq!(parsed.validate(), Ok(()));
}

#[test]
fn a_confidence_above_certainty_is_refused() {
    let mut parsed = candidate();
    parsed.category_confidence_bps = 65_535;
    assert!(matches!(
        parsed.validate(),
        Err(ValidationError::BasisPointsOutOfRange { .. })
    ));
}

// ------------------------------------------------------------------------- attachments ----

#[test]
fn a_receipt_with_a_relative_path_and_a_hex_digest_validates() {
    assert_eq!(attachment(id()).validate(), Ok(()));
}

#[test]
fn an_attachment_path_that_could_escape_its_directory_is_refused() {
    // The value arrives over sync from another device, and the app joins it to a root it
    // owns and opens it.
    for path in [
        "../../etc/passwd",
        "/etc/passwd",
        "receipts/../../secrets",
        "receipts\\windows",
        " receipts/a.jpg",
    ] {
        let mut row = attachment(id());
        row.relative_path = path.to_owned();
        assert!(
            matches!(
                row.validate(),
                Err(ValidationError::UnsafePath { .. } | ValidationError::Empty { .. })
            ),
            "`{path}` should be refused"
        );
    }

    let mut deep_but_safe = attachment(id());
    deep_but_safe.relative_path = "receipts/2026/11/photo..jpg".to_owned();
    assert_eq!(deep_but_safe.validate(), Ok(()));
}

#[test]
fn an_attachment_digest_must_be_lowercase_hex_of_the_right_length() {
    for digest in [
        "ZZ23456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "0123456789ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef",
        "0123456789abcdef",
    ] {
        let mut row = attachment(id());
        row.sha256 = digest.to_owned();
        assert!(
            matches!(row.validate(), Err(ValidationError::NotHex { .. })),
            "`{digest}` should be refused"
        );
    }
}

// ------------------------------------------------------------------------------- enums ----

macro_rules! assert_enum_round_trips {
    ($($ty:ty),* $(,)?) => {
        $(
            for variant in <$ty>::ALL {
                let text = variant.as_str();
                assert_eq!(
                    <$ty>::from_str(text).unwrap(),
                    *variant,
                    "{} did not round-trip through {text:?}",
                    stringify!($ty)
                );
                assert_eq!(variant.to_string(), text);
                assert_eq!(
                    serde_json::to_string(variant).unwrap(),
                    format!("\"{text}\""),
                    "{} serialises as its stored spelling",
                    stringify!($ty)
                );
            }
            assert!(
                <$ty>::from_str("not-a-variant").is_err(),
                "{} accepted an unknown value",
                stringify!($ty)
            );
        )*
    };
}

#[test]
fn stored_enum_spellings_round_trip_and_reject_unknown_values() {
    assert_enum_round_trips!(
        AccountKind,
        Direction,
        TxStatus,
        TxSource,
        CategoryKind,
        MatchedField,
        BudgetPeriod,
        RecurrenceFreq,
        RecurrenceScope,
        InboxSource,
        InboxStatus,
        RuleOrigin,
    );
}

#[test]
fn an_unknown_enum_value_says_what_was_expected() {
    let error = Direction::from_str("transfer").unwrap_err();
    assert_eq!(error.value, "transfer");
    assert_eq!(error.type_name, "Direction");
    assert!(error.to_string().contains("in, out"));
}
