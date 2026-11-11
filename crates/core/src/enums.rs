//! The closed sets the schema stores as text.
//!
//! Both databases keep these as `TEXT` with a `CHECK` constraint rather than a native
//! Postgres enum: the same spelling then appears in SQLite on the device, in Postgres on the
//! server and in sync payloads, and adding a variant is a check-constraint change rather
//! than a type migration that locks the table.

use core::fmt;
use core::str::FromStr;
use serde::{Deserialize, Serialize};

/// A value read from the database or a sync payload was not one of the known variants.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{value}` is not a valid {type_name}; expected one of: {expected}")]
pub struct UnknownVariant {
    /// The rejected text.
    pub value: String,
    /// Which enum it was being read as.
    pub type_name: &'static str,
    /// The accepted spellings, comma separated.
    pub expected: &'static str,
}

macro_rules! db_enum {
    (
        $(#[$meta:meta])*
        $name:ident, $expected:literal {
            $( $(#[$vmeta:meta])* $variant:ident => $text:literal; )*
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub enum $name {
            $( $(#[$vmeta])* #[serde(rename = $text)] $variant, )*
        }

        impl $name {
            /// Every variant, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$variant),*];

            /// The exact spelling stored in the database and sent over sync.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $($name::$variant => $text,)* }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = UnknownVariant;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($text => Ok($name::$variant),)*
                    other => Err(UnknownVariant {
                        value: other.to_owned(),
                        type_name: stringify!($name),
                        expected: $expected,
                    }),
                }
            }
        }
    };
}

db_enum! {
    /// What kind of place the money sits in. Each account holds exactly one currency.
    AccountKind, "cash, momo, bank, card" {
        /// Physical cash. Reconciled by counting, not by statement.
        Cash => "cash";
        /// A mobile money wallet — MTN MoMo, Telecel Cash, AT Money.
        Momo => "momo";
        /// A current or savings account at a bank.
        Bank => "bank";
        /// A credit or prepaid card settled separately from the bank account behind it.
        Card => "card";
    }
}

db_enum! {
    /// Which way the money moved, from the point of view of the row's own account.
    ///
    /// There is no `transfer` direction. A transfer is two rows — an `out` and an `in` —
    /// sharing a `transfer_id`; the balance walk nets them so moving money between your own
    /// accounts is neither income nor spending.
    Direction, "in, out" {
        /// Money arrived in this account.
        In => "in";
        /// Money left this account.
        Out => "out";
    }
}

db_enum! {
    /// Whether a row has actually happened.
    ///
    /// Future-dated `pending` rows *are* the forecast: the balance walk sorts by date and
    /// accumulates without caring which side of today a row falls on, so there is no second
    /// projection engine to keep in agreement with the first.
    TxStatus, "pending, confirmed" {
        /// Expected, or parsed but not yet reviewed into the ledger.
        Pending => "pending";
        /// Known to have happened.
        Confirmed => "confirmed";
    }
}

db_enum! {
    /// Where a row came from. Kept for ever: it is what makes a bad rule pack traceable.
    TxSource, "manual, sms, csv, ofx, receipt, recurring, adjustment" {
        /// Typed in by hand. The only path that must work on every platform.
        Manual => "manual";
        /// Parsed on-device from a MoMo or bank alert, then confirmed in the inbox.
        Sms => "sms";
        /// Imported from a CSV statement with a stored column mapping.
        Csv => "csv";
        /// Imported from an OFX statement.
        Ofx => "ofx";
        /// Read off a photographed receipt.
        Receipt => "receipt";
        /// Generated from a recurring template.
        Recurring => "recurring";
        /// A reconciliation entry: the single row that makes the ledger agree with a count.
        Adjustment => "adjustment";
    }
}

db_enum! {
    /// Whether a category describes money coming in or going out.
    CategoryKind, "income, expense" {
        /// Salary, transfers received, refunds.
        Income => "income";
        /// Everything spent.
        Expense => "expense";
    }
}

db_enum! {
    /// Which field of a candidate a stored pattern is matched against.
    ///
    /// `raw_text` only ever matches in memory during parsing, because raw SMS bodies are
    /// never written to storage.
    MatchedField, "label, counterparty, raw_text" {
        /// The human-readable description.
        Label => "label";
        /// The merchant, sender or recipient.
        Counterparty => "counterparty";
        /// The message body, available only while parsing, never persisted.
        RawText => "raw_text";
    }
}

db_enum! {
    /// The window a budget cap applies over.
    BudgetPeriod, "weekly, monthly, yearly" {
        /// Seven days from `starts_on`.
        Weekly => "weekly";
        /// Calendar month.
        Monthly => "monthly";
        /// Calendar year.
        Yearly => "yearly";
    }
}

db_enum! {
    /// How often a recurring template fires.
    RecurrenceFreq, "daily, weekly, monthly, yearly" {
        /// Every `interval` days.
        Daily => "daily";
        /// Every `interval` weeks, on `by_weekday`.
        Weekly => "weekly";
        /// Every `interval` months, on `by_month_day`.
        Monthly => "monthly";
        /// Every `interval` years.
        Yearly => "yearly";
    }
}

db_enum! {
    /// Which generated instances an edit to a recurring template applies to.
    ///
    /// Asked explicitly at edit time rather than guessed. This is the detail recurring
    /// transactions are usually got wrong on.
    RecurrenceScope, "this, future, past, all" {
        /// Only the instance being edited; it detaches from the template.
        This => "this";
        /// This instance and every later one.
        Future => "future";
        /// This instance and every earlier one.
        Past => "past";
        /// Every instance the template has generated.
        All => "all";
    }
}

db_enum! {
    /// Where an inbox candidate came from.
    InboxSource, "sms, csv, ofx, receipt" {
        /// Parsed on-device from a message whose sender is on the allow-list.
        Sms => "sms";
        /// A row of an imported CSV statement.
        Csv => "csv";
        /// A row of an imported OFX statement.
        Ofx => "ofx";
        /// A photographed receipt.
        Receipt => "receipt";
    }
}

db_enum! {
    /// How far through review a candidate is.
    InboxStatus, "pending, accepted, rejected, merged" {
        /// Waiting for the user.
        Pending => "pending";
        /// Confirmed into the ledger; `transaction_id` points at the row it became.
        Accepted => "accepted";
        /// Dismissed. Kept so the same message is not offered twice.
        Rejected => "rejected";
        /// Judged the same event as an existing row; `duplicate_of` points at it.
        Merged => "merged";
    }
}

db_enum! {
    /// Who authored a stored pattern.
    RuleOrigin, "user, pack" {
        /// Written by the user, usually by correcting a category in the inbox.
        User => "user";
        /// Shipped in a versioned rule pack. Replaced wholesale when the pack updates.
        Pack => "pack";
    }
}
