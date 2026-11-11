//! Money as integer minor units with a currency attached.
//!
//! There is no floating point in this module and none anywhere else in the crate: the
//! workspace lint config denies `clippy::float_arithmetic`. A transaction keeps the currency
//! it happened in forever, so arithmetic across currencies is a compile-time-visible error
//! path rather than a silent conversion.

use core::fmt;
use serde::Deserialize;
use serde::de::{self, Deserializer};
use serde::ser::{Serialize, SerializeStruct, Serializer};

/// Anything that can go wrong doing arithmetic on, or constructing, [`Money`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MoneyError {
    /// The code was not three ASCII letters, or is not in the supported registry.
    ///
    /// Unknown codes are rejected rather than defaulted to two decimal places, because a
    /// wrong exponent silently multiplies or divides someone's balance by a hundred.
    #[error("unsupported currency code `{0}` (add it to the registry in money.rs)")]
    UnknownCurrency(String),

    /// Two amounts in different currencies were combined.
    #[error("currency mismatch: {left} and {right}")]
    CurrencyMismatch {
        /// Currency of the left-hand amount.
        left: Currency,
        /// Currency of the right-hand amount.
        right: Currency,
    },

    /// The result does not fit in `i64` minor units.
    #[error("money arithmetic overflowed i64 minor units")]
    Overflow,

    /// A decimal string could not be read as an amount in the given currency.
    #[error("cannot parse `{input}` as {currency}: {reason}")]
    Parse {
        /// The string that was offered.
        input: String,
        /// The currency it was being parsed into.
        currency: Currency,
        /// Why it was rejected.
        reason: ParseRejection,
    },
}

/// Why a decimal string was not accepted as an amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ParseRejection {
    /// No digits at all, or only a sign.
    #[error("no digits")]
    Empty,
    /// A character that is not an ASCII digit, a single leading sign, or one decimal point.
    #[error("unexpected character")]
    BadCharacter,
    /// More decimal places than the currency has. Rounding here would lose money quietly.
    #[error("too many decimal places for this currency")]
    TooPrecise,
    /// The value does not fit in `i64` minor units.
    #[error("out of range")]
    OutOfRange,
}

/// An ISO 4217 currency code from the supported registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Currency([u8; 3]);

macro_rules! currency_registry {
    ($($konst:ident => $code:literal, $exponent:literal, $name:literal;)*) => {
        impl Currency {
            $(
                #[doc = $name]
                pub const $konst: Currency = Currency(*$code);
            )*

            /// Every currency the crate knows how to scale, in registry order.
            pub const ALL: &'static [Currency] = &[$(Currency::$konst),*];

            /// Number of decimal places this currency subdivides into (2 for GHS, 0 for XOF).
            #[must_use]
            pub const fn exponent(self) -> u8 {
                match &self.0 {
                    $($code => $exponent,)*
                    // Unreachable for a value built through `parse`, which is the only
                    // public constructor; a const fn cannot assert, so fall back to the
                    // most common exponent rather than silently returning zero.
                    _ => 2,
                }
            }

            const fn is_known(code: &[u8; 3]) -> bool {
                match code {
                    $($code => true,)*
                    _ => false,
                }
            }
        }

        // The one bound on an exponent, checked where an entry enters rather than where it
        // is used: 10^19 leaves `i64`, so `Currency::scale` would overflow. Eighteen is
        // far past any real currency — three decimal places is the most anyone uses — and
        // the point is that the limit is stated once, at the registry, instead of being
        // implied by a list of match arms somewhere else that can silently fall out of
        // step with it.
        $(
            const _: () = assert!(
                $exponent <= 18,
                concat!("currency ", $name, " has an exponent that overflows `Currency::scale`"),
            );
        )*
    };
}

currency_registry! {
    GHS => b"GHS", 2, "Ghanaian cedi";
    NGN => b"NGN", 2, "Nigerian naira";
    XOF => b"XOF", 0, "West African CFA franc";
    XAF => b"XAF", 0, "Central African CFA franc";
    KES => b"KES", 2, "Kenyan shilling";
    TZS => b"TZS", 2, "Tanzanian shilling";
    UGX => b"UGX", 0, "Ugandan shilling";
    RWF => b"RWF", 0, "Rwandan franc";
    ZAR => b"ZAR", 2, "South African rand";
    EGP => b"EGP", 2, "Egyptian pound";
    MAD => b"MAD", 2, "Moroccan dirham";
    USD => b"USD", 2, "United States dollar";
    EUR => b"EUR", 2, "Euro";
    GBP => b"GBP", 2, "Pound sterling";
    CAD => b"CAD", 2, "Canadian dollar";
    AUD => b"AUD", 2, "Australian dollar";
    CHF => b"CHF", 2, "Swiss franc";
    CNY => b"CNY", 2, "Chinese yuan";
    INR => b"INR", 2, "Indian rupee";
    JPY => b"JPY", 0, "Japanese yen";
    AED => b"AED", 2, "UAE dirham";
}

impl Currency {
    /// Reads a three-letter ISO code, case-insensitively.
    ///
    /// # Errors
    /// [`MoneyError::UnknownCurrency`] if the code is not three ASCII letters or is not in
    /// the registry above.
    pub fn parse(code: &str) -> Result<Self, MoneyError> {
        let unknown = || MoneyError::UnknownCurrency(code.to_owned());
        let bytes: [u8; 3] = code.as_bytes().try_into().map_err(|_| unknown())?;
        if !bytes.iter().all(u8::is_ascii_alphabetic) {
            return Err(unknown());
        }
        let upper = [
            bytes[0].to_ascii_uppercase(),
            bytes[1].to_ascii_uppercase(),
            bytes[2].to_ascii_uppercase(),
        ];
        if Self::is_known(&upper) {
            Ok(Currency(upper))
        } else {
            Err(unknown())
        }
    }

    /// The uppercase ISO code.
    #[must_use]
    pub fn code(&self) -> &str {
        core::str::from_utf8(&self.0).unwrap_or("???")
    }

    /// Minor units in one major unit: 100 for GHS, 1 for XOF.
    #[must_use]
    pub const fn scale(self) -> i64 {
        // Computed rather than tabulated. A match arm per exponent was a second list that
        // had to agree with the registry and nothing made it: deleting an arm would have
        // fallen through to a plausible wrong factor instead of failing. The exponent is
        // bounded where it enters, beside the registry, so this cannot overflow.
        let mut factor: i64 = 1;
        let mut places = self.exponent();
        while places > 0 {
            factor *= 10;
            places -= 1;
        }
        factor
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl Serialize for Currency {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.code())
    }
}

/// Reads a currency from a string, borrowed or owned.
///
/// Going through a visitor rather than `<&str>::deserialize` matters: a deserializer that
/// owns its buffer — a streaming reader, or whatever codec the bridge ends up using — cannot
/// hand out a borrowed `&str`, and would fail with a confusing type error on valid input.
struct CurrencyVisitor;

impl de::Visitor<'_> for CurrencyVisitor {
    type Value = Currency;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a three-letter ISO 4217 currency code")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Currency, E> {
        Currency::parse(value).map_err(E::custom)
    }
}

impl<'de> Deserialize<'de> for Currency {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(CurrencyVisitor)
    }
}

/// An amount of money: integer minor units plus the currency it is denominated in.
///
/// Construct with [`Money::new`] or [`Money::parse`]. All arithmetic is checked and returns
/// [`Result`]; there is no operator overloading, so a currency mismatch cannot be written by
/// accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Money {
    minor: i64,
    currency: Currency,
}

impl Money {
    /// An amount of `minor` minor units in `currency`.
    #[must_use]
    pub const fn new(minor: i64, currency: Currency) -> Self {
        Self { minor, currency }
    }

    /// Zero in `currency`.
    #[must_use]
    pub const fn zero(currency: Currency) -> Self {
        Self::new(0, currency)
    }

    /// The signed amount in minor units.
    #[must_use]
    pub const fn minor(self) -> i64 {
        self.minor
    }

    /// The currency this amount is denominated in.
    #[must_use]
    pub const fn currency(self) -> Currency {
        self.currency
    }

    /// True for exactly zero.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.minor == 0
    }

    /// True for amounts below zero.
    #[must_use]
    pub const fn is_negative(self) -> bool {
        self.minor < 0
    }

    /// Adds two amounts.
    ///
    /// # Errors
    /// [`MoneyError::CurrencyMismatch`] if the currencies differ, [`MoneyError::Overflow`] if
    /// the result leaves `i64`.
    pub fn try_add(self, other: Self) -> Result<Self, MoneyError> {
        self.combine(other, i64::checked_add)
    }

    /// Subtracts `other` from `self`.
    ///
    /// # Errors
    /// As [`Money::try_add`].
    pub fn try_sub(self, other: Self) -> Result<Self, MoneyError> {
        self.combine(other, i64::checked_sub)
    }

    /// Flips the sign.
    ///
    /// # Errors
    /// [`MoneyError::Overflow`] for `i64::MIN`.
    pub fn try_neg(self) -> Result<Self, MoneyError> {
        self.minor
            .checked_neg()
            .map(|minor| Self::new(minor, self.currency))
            .ok_or(MoneyError::Overflow)
    }

    /// The magnitude, dropping the sign.
    ///
    /// # Errors
    /// [`MoneyError::Overflow`] for `i64::MIN`.
    pub fn abs(self) -> Result<Self, MoneyError> {
        self.minor
            .checked_abs()
            .map(|minor| Self::new(minor, self.currency))
            .ok_or(MoneyError::Overflow)
    }

    fn combine(self, other: Self, op: fn(i64, i64) -> Option<i64>) -> Result<Self, MoneyError> {
        if self.currency != other.currency {
            return Err(MoneyError::CurrencyMismatch {
                left: self.currency,
                right: other.currency,
            });
        }
        op(self.minor, other.minor)
            .map(|minor| Self::new(minor, self.currency))
            .ok_or(MoneyError::Overflow)
    }

    /// Totals an iterator of amounts, all of which must share `currency`.
    ///
    /// Takes the currency explicitly so that an empty iterator still yields a typed zero
    /// rather than `None`.
    ///
    /// # Errors
    /// As [`Money::try_add`].
    pub fn total<I: IntoIterator<Item = Self>>(
        currency: Currency,
        amounts: I,
    ) -> Result<Self, MoneyError> {
        amounts
            .into_iter()
            .try_fold(Self::zero(currency), Money::try_add)
    }

    /// Reads a decimal string such as `"12.50"`, `"-7"` or `"0.05"` in `currency`.
    ///
    /// Strict on purpose: no thousands separators, no whitespace, no exponent notation, and
    /// never more decimal places than the currency has — rounding user input silently is how
    /// a ledger starts disagreeing with reality. Formatting for display is the UI's job.
    ///
    /// # Errors
    /// [`MoneyError::Parse`] with the specific [`ParseRejection`].
    pub fn parse(input: &str, currency: Currency) -> Result<Self, MoneyError> {
        Self::parse_inner(input, currency).map_err(|reason| MoneyError::Parse {
            input: input.to_owned(),
            currency,
            reason,
        })
    }

    fn parse_inner(input: &str, currency: Currency) -> Result<Self, ParseRejection> {
        let (negative, digits) = match input.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, input.strip_prefix('+').unwrap_or(input)),
        };

        let (whole, fraction) = match digits.split_once('.') {
            Some((whole, fraction)) => (whole, fraction),
            None => (digits, ""),
        };

        if whole.is_empty() && fraction.is_empty() {
            return Err(ParseRejection::Empty);
        }
        if !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
        {
            return Err(ParseRejection::BadCharacter);
        }
        if fraction.len() > usize::from(currency.exponent()) {
            return Err(ParseRejection::TooPrecise);
        }

        // Accumulated in i128 and narrowed at the end, so the one magnitude that has no
        // positive counterpart — i64::MIN — parses back from what Display wrote.
        let whole_units: i128 = if whole.is_empty() {
            0
        } else {
            whole.parse().map_err(|_| ParseRejection::OutOfRange)?
        };

        // Pad "5" to "50" for a two-decimal currency so the digits land in the right place.
        let mut fraction_minor: i128 = 0;
        let mut place = 0;
        for byte in fraction.bytes() {
            let digit = i128::from(byte - b'0');
            fraction_minor = fraction_minor
                .checked_mul(10)
                .and_then(|v| v.checked_add(digit))
                .ok_or(ParseRejection::OutOfRange)?;
            place += 1;
        }
        while place < currency.exponent() {
            fraction_minor = fraction_minor
                .checked_mul(10)
                .ok_or(ParseRejection::OutOfRange)?;
            place += 1;
        }

        let magnitude = whole_units
            .checked_mul(i128::from(currency.scale()))
            .and_then(|v| v.checked_add(fraction_minor))
            .ok_or(ParseRejection::OutOfRange)?;

        let signed = if negative { -magnitude } else { magnitude };
        let minor = i64::try_from(signed).map_err(|_| ParseRejection::OutOfRange)?;
        Ok(Self::new(minor, currency))
    }
}

/// Renders the bare number with the currency's decimal places and no symbol or grouping —
/// `1250` GHS prints as `12.50`. Symbols, grouping and locale are presentation concerns.
impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let exponent = usize::from(self.currency.exponent());
        if exponent == 0 {
            return write!(f, "{}", self.minor);
        }
        let sign = if self.minor < 0 { "-" } else { "" };
        // unsigned_abs avoids the i64::MIN overflow that `abs()` would hit.
        let magnitude = self.minor.unsigned_abs();
        let scale = self.currency.scale().unsigned_abs();
        let whole = magnitude / scale;
        let fraction = magnitude % scale;
        write!(f, "{sign}{whole}.{fraction:0width$}", width = exponent)
    }
}

impl Serialize for Money {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("Money", 2)?;
        state.serialize_field("minor", &self.minor)?;
        state.serialize_field("currency", &self.currency)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Money {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            minor: i64,
            currency: Currency,
        }
        let wire = Wire::deserialize(deserializer)?;
        Ok(Money::new(wire.minor, wire.currency))
    }
}
