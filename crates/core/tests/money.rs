//! Behaviour of `Money` and `Currency`: construction, parsing, checked arithmetic, display.

// Sealed like the library it tests: an integration test is its own crate root, and the
// workspace only denies unsafe, which an `allow` in this file would switch off.
#![forbid(unsafe_code)]

#![allow(clippy::unwrap_used, clippy::panic, clippy::float_arithmetic)]

use mt_core::money::{Currency, Money, MoneyError, ParseRejection};

const GHS: Currency = Currency::GHS;
const USD: Currency = Currency::USD;
const JPY: Currency = Currency::JPY;

fn rejection(input: &str, currency: Currency) -> ParseRejection {
    match Money::parse(input, currency) {
        Err(MoneyError::Parse { reason, .. }) => reason,
        other => panic!("expected a parse rejection for {input:?}, got {other:?}"),
    }
}

#[test]
fn currency_parses_case_insensitively() {
    assert_eq!(Currency::parse("ghs").unwrap(), GHS);
    assert_eq!(Currency::parse("GhS").unwrap(), GHS);
    assert_eq!(Currency::parse("GHS").unwrap().code(), "GHS");
}

#[test]
fn unknown_or_malformed_currency_codes_are_rejected() {
    for code in ["", "GH", "GHSX", "GH1", "ZZZ", "€UR", "   "] {
        assert!(
            matches!(Currency::parse(code), Err(MoneyError::UnknownCurrency(_))),
            "expected {code:?} to be rejected"
        );
    }
}

#[test]
fn every_registry_currency_has_a_consistent_scale() {
    for currency in Currency::ALL {
        let scale = currency.scale();
        let expected = 10_i64.pow(u32::from(currency.exponent()));
        assert_eq!(scale, expected, "{currency} scale");
        assert_eq!(
            Currency::parse(currency.code()).unwrap(),
            *currency,
            "{currency} round-trips through its code"
        );
    }
}

#[test]
fn parses_decimal_strings_into_minor_units() {
    let cases = [
        ("12.50", 1250),
        ("12.5", 1250),
        ("12", 1200),
        (".50", 50),
        ("0.05", 5),
        ("0", 0),
        ("-7.05", -705),
        ("+3", 300),
        ("-0.01", -1),
    ];
    for (input, expected) in cases {
        let parsed = Money::parse(input, GHS).unwrap();
        assert_eq!(parsed.minor(), expected, "parsing {input:?}");
        assert_eq!(parsed.currency(), GHS);
    }
}

#[test]
fn zero_decimal_currencies_take_whole_units_only() {
    assert_eq!(Money::parse("1200", JPY).unwrap().minor(), 1200);
    assert_eq!(rejection("12.5", JPY), ParseRejection::TooPrecise);
}

#[test]
fn more_decimals_than_the_currency_has_are_refused_not_rounded() {
    assert_eq!(rejection("12.555", GHS), ParseRejection::TooPrecise);
    assert_eq!(rejection("0.001", GHS), ParseRejection::TooPrecise);
}

#[test]
fn junk_input_is_refused_with_a_specific_reason() {
    assert_eq!(rejection("", GHS), ParseRejection::Empty);
    assert_eq!(rejection("-", GHS), ParseRejection::Empty);
    assert_eq!(rejection(".", GHS), ParseRejection::Empty);
    assert_eq!(rejection("abc", GHS), ParseRejection::BadCharacter);
    assert_eq!(rejection("1 200", GHS), ParseRejection::BadCharacter);
    assert_eq!(rejection("1,200", GHS), ParseRejection::BadCharacter);
    assert_eq!(rejection("1e3", GHS), ParseRejection::BadCharacter);
    assert_eq!(rejection("12.50 ", GHS), ParseRejection::BadCharacter);
    assert_eq!(rejection("1.2.3", GHS), ParseRejection::BadCharacter);
    // Arabic-indic digits are digits to a human and not to `is_ascii_digit`; refusing is
    // right, silently reading them as something else would not be.
    assert_eq!(
        rejection("\u{660}\u{661}", GHS),
        ParseRejection::BadCharacter
    );
}

#[test]
fn values_too_large_for_minor_units_are_refused() {
    assert_eq!(
        rejection("92233720368547758070", GHS),
        ParseRejection::OutOfRange
    );
    assert_eq!(
        rejection("92233720368547758.08", GHS),
        ParseRejection::OutOfRange
    );
}

#[test]
fn addition_and_subtraction_stay_in_one_currency() {
    let ten = Money::new(1000, GHS);
    let three = Money::new(300, GHS);
    assert_eq!(ten.try_add(three).unwrap(), Money::new(1300, GHS));
    assert_eq!(ten.try_sub(three).unwrap(), Money::new(700, GHS));
    assert_eq!(three.try_sub(ten).unwrap(), Money::new(-700, GHS));
}

#[test]
fn mixing_currencies_is_an_error_not_a_conversion() {
    let cedis = Money::new(1000, GHS);
    let dollars = Money::new(1000, USD);
    assert_eq!(
        cedis.try_add(dollars),
        Err(MoneyError::CurrencyMismatch {
            left: GHS,
            right: USD
        })
    );
    assert!(cedis.try_sub(dollars).is_err());
}

#[test]
fn arithmetic_overflow_is_reported_rather_than_wrapping() {
    let big = Money::new(i64::MAX, GHS);
    assert_eq!(big.try_add(Money::new(1, GHS)), Err(MoneyError::Overflow));
    let small = Money::new(i64::MIN, GHS);
    assert_eq!(small.try_sub(Money::new(1, GHS)), Err(MoneyError::Overflow));
    assert_eq!(small.try_neg(), Err(MoneyError::Overflow));
    assert_eq!(small.abs(), Err(MoneyError::Overflow));
}

#[test]
fn totalling_an_empty_list_gives_a_typed_zero() {
    let total = Money::total(GHS, []).unwrap();
    assert_eq!(total, Money::zero(GHS));
    assert!(total.is_zero());
}

#[test]
fn totalling_accumulates_and_still_catches_a_stray_currency() {
    let rows = [
        Money::new(1000, GHS),
        Money::new(-250, GHS),
        Money::new(5, GHS),
    ];
    assert_eq!(Money::total(GHS, rows).unwrap(), Money::new(755, GHS));
    assert!(Money::total(GHS, [Money::new(1, GHS), Money::new(1, USD)]).is_err());
}

#[test]
fn display_shows_the_currencys_own_decimal_places_and_no_symbol() {
    assert_eq!(Money::new(1250, GHS).to_string(), "12.50");
    assert_eq!(Money::new(0, GHS).to_string(), "0.00");
    assert_eq!(Money::new(5, GHS).to_string(), "0.05");
    assert_eq!(Money::new(-5, GHS).to_string(), "-0.05");
    assert_eq!(Money::new(-1250, GHS).to_string(), "-12.50");
    assert_eq!(Money::new(1200, JPY).to_string(), "1200");
    assert_eq!(Money::new(-1200, JPY).to_string(), "-1200");
}

#[test]
fn display_handles_the_extreme_values_without_panicking() {
    assert_eq!(
        Money::new(i64::MIN, GHS).to_string(),
        "-92233720368547758.08"
    );
    assert_eq!(
        Money::new(i64::MAX, GHS).to_string(),
        "92233720368547758.07"
    );
}

#[test]
fn display_round_trips_through_parse() {
    // i64::MIN is the interesting one: its magnitude has no positive counterpart, so a
    // parser that builds the magnitude before applying the sign cannot read back what
    // Display just wrote.
    for minor in [
        0_i64,
        1,
        -1,
        5,
        -99,
        1250,
        -1250,
        99_999_999,
        i64::MAX,
        i64::MIN,
    ] {
        let amount = Money::new(minor, GHS);
        assert_eq!(
            Money::parse(&amount.to_string(), GHS).unwrap(),
            amount,
            "{minor} did not survive a trip through Display"
        );
    }

    for minor in [0_i64, -1, i64::MAX, i64::MIN] {
        let amount = Money::new(minor, JPY);
        assert_eq!(Money::parse(&amount.to_string(), JPY).unwrap(), amount);
    }
}

#[test]
fn one_more_than_the_largest_amount_is_still_out_of_range() {
    // The widening that makes i64::MIN parse must not let anything past the real ceiling.
    assert_eq!(
        rejection("92233720368547758.08", GHS),
        ParseRejection::OutOfRange
    );
    assert_eq!(
        rejection("-92233720368547758.09", GHS),
        ParseRejection::OutOfRange
    );
    assert_eq!(
        Money::parse("-92233720368547758.08", GHS).unwrap(),
        Money::new(i64::MIN, GHS)
    );
}

#[test]
fn serialises_as_minor_units_plus_a_code() {
    let amount = Money::new(1250, GHS);
    let json = serde_json::to_string(&amount).unwrap();
    assert_eq!(json, r#"{"minor":1250,"currency":"GHS"}"#);
    assert_eq!(serde_json::from_str::<Money>(&json).unwrap(), amount);
}

#[test]
fn deserialising_an_unknown_currency_fails_rather_than_defaulting() {
    let result = serde_json::from_str::<Money>(r#"{"minor":1,"currency":"ZZZ"}"#);
    assert!(result.is_err(), "unknown currency must not deserialise");
}
