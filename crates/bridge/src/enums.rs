//! The core's enums, mirrored for the bridge.
//!
//! These exist so Dart gets sealed enums rather than strings. `mt-core`'s own enums carry
//! `serde` spellings and a `FromStr`, and the temptation is to pass those strings straight
//! through; that would move an exhaustive match in Rust into a `switch` in Dart with a
//! `default:` branch, which is precisely the place a new `TxSource` would be silently
//! ignored. Mirroring costs one line per variant and makes adding one a compile error in
//! both languages.
//!
//! Each mirror converts both ways and cannot lose a variant: the macro writes the match
//! arms, so a variant added to the core without one here does not compile.

use mt_core::enums as core_enums;
use mt_core::{budget as core_budget, dedup as core_dedup, ledger as core_ledger};

macro_rules! mirror {
    (
        $(#[$meta:meta])*
        $name:ident <=> $core:path { $( $(#[$vmeta:meta])* $variant:ident ),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name {
            $( $(#[$vmeta])* $variant, )+
        }

        impl From<$core> for $name {
            fn from(value: $core) -> Self {
                match value { $( <$core>::$variant => Self::$variant, )+ }
            }
        }

        impl From<$name> for $core {
            fn from(value: $name) -> Self {
                match value { $( $name::$variant => Self::$variant, )+ }
            }
        }
    };
}

mirror! {
    /// What kind of place the money sits in.
    AccountKind <=> core_enums::AccountKind {
        /// Physical cash.
        Cash,
        /// A mobile money wallet.
        Momo,
        /// A bank account.
        Bank,
        /// A card settled separately from the account behind it.
        Card,
    }
}

mirror! {
    /// Which way the money moved, from the point of view of the row's own account.
    Direction <=> core_enums::Direction {
        /// Money arrived.
        In,
        /// Money left.
        Out,
    }
}

mirror! {
    /// Whether a row has actually happened. Future-dated pending rows are the forecast.
    TxStatus <=> core_enums::TxStatus {
        /// Expected, or parsed but not yet reviewed into the ledger.
        Pending,
        /// Known to have happened.
        Confirmed,
    }
}

mirror! {
    /// Where a row came from.
    TxSource <=> core_enums::TxSource {
        /// Typed by hand.
        Manual,
        /// Parsed from a message.
        Sms,
        /// Imported from a delimited statement.
        Csv,
        /// Imported from an OFX statement.
        Ofx,
        /// Scanned from a receipt.
        Receipt,
        /// Generated from a recurring template.
        Recurring,
        /// A reconciliation entry.
        Adjustment,
    }
}

mirror! {
    /// Whether a category counts as money in or money out.
    CategoryKind <=> core_enums::CategoryKind {
        /// Money in.
        Income,
        /// Money out.
        Expense,
    }
}

mirror! {
    /// The field a stored pattern is tried against.
    MatchedField <=> core_enums::MatchedField {
        /// The human-readable description.
        Label,
        /// The merchant, sender or recipient.
        Counterparty,
        /// The message body, for the length of one parse only.
        RawText,
    }
}

mirror! {
    /// How often a cap renews.
    BudgetPeriod <=> core_enums::BudgetPeriod {
        /// Every week.
        Weekly,
        /// Every month.
        Monthly,
        /// Every year.
        Yearly,
    }
}

mirror! {
    /// How often a template fires.
    RecurrenceFreq <=> core_enums::RecurrenceFreq {
        /// Every day.
        Daily,
        /// Every week.
        Weekly,
        /// Every month.
        Monthly,
        /// Every year.
        Yearly,
    }
}

mirror! {
    /// Which instances an edit to a recurring template applies to.
    RecurrenceScope <=> core_enums::RecurrenceScope {
        /// This occurrence only.
        This,
        /// This one and everything after it.
        Future,
        /// Everything before it.
        Past,
        /// Every instance.
        All,
    }
}

mirror! {
    /// Who wrote a categorisation rule.
    RuleOrigin <=> core_enums::RuleOrigin {
        /// The user.
        User,
        /// A shipped rule pack.
        Pack,
    }
}

mirror! {
    /// Which rows a balance counts.
    BalanceBasis <=> core_ledger::BalanceBasis {
        /// Only rows known to have happened.
        Confirmed,
        /// Confirmed and pending alike — the forecast.
        Projected,
    }
}

mirror! {
    /// Where spending stands against a cap.
    BudgetLevel <=> core_budget::Level {
        /// Below the warning threshold.
        Under,
        /// Past the warning threshold, inside the cap.
        Warning,
        /// At or past the cap.
        Over,
    }
}

mirror! {
    /// One reason a pair looks like a duplicate.
    DuplicateSignal <=> core_dedup::Signal {
        /// Both carry the same reference from the bank or wallet.
        SameExternalId,
        /// Same amount, currency and direction, inside the window.
        SameAmount,
        /// The candidate names the account the existing row is in.
        SameAccount,
        /// Close together in time.
        CloseInTime,
        /// The counterparties normalise to the same string.
        SameCounterparty,
        /// The counterparties share a significant word.
        OverlappingCounterparty,
        /// The labels normalise to the same string.
        SameLabel,
    }
}

/// Which rung of the categorisation ladder produced a category.
///
/// Not a mirror: the core carries the rule or row that decided it inside the variant, and
/// this crate splits that into a `source_id` beside the basis rather than crossing the
/// bridge as an enum with a payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VerdictBasis {
    /// The source named the category itself.
    Explicit,
    /// A stored pattern matched; the source id is the rule.
    Rule,
    /// Copied from one of the user's own earlier rows; the source id is that row.
    History,
    /// Nothing matched.
    Unmatched,
}
