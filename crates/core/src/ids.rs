//! Typed row identifiers.
//!
//! Every id is a UUIDv7: time-ordered, so index locality stays good as rows are appended,
//! and generated on whichever device created the row, so two offline devices can insert
//! without coordinating. Each table gets its own type so an `AccountId` cannot be passed
//! where a `CategoryId` is wanted.

use core::fmt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! typed_id {
    ($($name:ident, $doc:literal;)*) => {
        $(
            #[doc = $doc]
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
            #[serde(transparent)]
            pub struct $name(Uuid);

            impl $name {
                /// Wraps an existing UUID.
                #[must_use]
                pub const fn from_uuid(id: Uuid) -> Self { Self(id) }

                /// The underlying UUID.
                #[must_use]
                pub const fn as_uuid(&self) -> &Uuid { &self.0 }

                /// Parses the canonical hyphenated form.
                ///
                /// # Errors
                /// [`uuid::Error`] if the text is not a UUID.
                pub fn parse(text: &str) -> Result<Self, uuid::Error> {
                    Uuid::parse_str(text).map(Self)
                }
            }

            impl fmt::Display for $name {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    fmt::Display::fmt(&self.0, f)
                }
            }

            impl From<Uuid> for $name {
                fn from(id: Uuid) -> Self { Self(id) }
            }
        )*
    };
}

typed_id! {
    UserId, "A person's account on the server. Absent from the device schema, which holds exactly one person's data.";
    DeviceId, "One installation. Carried on every synced row as the last writer.";
    AccountId, "A cash, mobile-money, bank or card account.";
    TxId, "A transaction row.";
    TransferId, "Pairs the two transaction rows that make up a transfer.";
    CategoryId, "A spending or income category.";
    CategoryRuleId, "A stored pattern that assigns a category.";
    RecurringId, "A recurring template.";
    BudgetId, "A cap on one category in one currency.";
    InboxItemId, "A parsed-but-unconfirmed candidate awaiting review.";
    AttachmentId, "A receipt image or imported file.";
    RulePackId, "A versioned pack of SMS patterns and default categories.";
}
