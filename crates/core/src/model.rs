//! The rows, as the core sees them.
//!
//! These structs are the single description of the shape of the data. The Postgres schema in
//! `db/postgres/` and the SQLite schema in `db/sqlite/` are the same tables; `docs/SCHEMA.md`
//! maps one onto the other and records where they deliberately differ.
//!
//! Everything here is plain owned data with public fields, because these values cross the
//! `flutter_rust_bridge` boundary: values cross, handles do not.

use crate::enums::{
    AccountKind, BudgetPeriod, CategoryKind, Direction, InboxSource, InboxStatus, MatchedField,
    RecurrenceFreq, RuleOrigin, TxSource, TxStatus,
};
use crate::ids::{
    AccountId, AttachmentId, BudgetId, CategoryId, CategoryRuleId, DeviceId, InboxItemId,
    RecurringId, RulePackId, TransferId, TxId,
};
use crate::money::{Currency, Money};
use serde::{Deserialize, Serialize};
use time::macros::datetime;
use time::{Date, OffsetDateTime};

// Dates cross the bridge and the sync wire as `YYYY-MM-DD`, not as `time`'s internal
// component array. The wire format is a protocol commitment, so it is written down here and
// asserted in `tests/wire.rs` rather than inherited from a dependency's default.
time::serde::format_description!(iso_date, Date, "[year]-[month]-[day]");

/// Longest account, category or budget name.
pub const MAX_NAME_LEN: usize = 64;
/// Longest transaction label.
pub const MAX_LABEL_LEN: usize = 200;
/// Longest counterparty (merchant, sender, recipient).
pub const MAX_COUNTERPARTY_LEN: usize = 120;
/// Longest free-text note.
pub const MAX_NOTE_LEN: usize = 2_000;
/// Longest stored match pattern.
pub const MAX_PATTERN_LEN: usize = 500;
/// Longest source-specific reference (statement line, inbox item, receipt file).
pub const MAX_SOURCE_REF_LEN: usize = 200;
/// Longest institution-supplied reference.
pub const MAX_EXTERNAL_ID_LEN: usize = 128;
/// Longest inbox fingerprint. A hex digest, so this is generous.
pub const MAX_FINGERPRINT_LEN: usize = 128;
/// Longest IANA timezone name.
pub const MAX_TIMEZONE_LEN: usize = 64;
/// Longest attachment path, relative to the app's private attachment directory.
pub const MAX_PATH_LEN: usize = 512;
/// Longest IANA media type.
pub const MAX_MEDIA_TYPE_LEN: usize = 128;
/// Characters in a hex-encoded SHA-256 digest.
pub const SHA256_HEX_LEN: usize = 64;
/// One whole, as basis points. 8000 is 80%.
pub const BPS_SCALE: u16 = 10_000;

/// Largest amount any single row may carry, in minor units: a thousand million major units.
///
/// Not a currency limit — a guard. `i64` minor units reach 92 quadrillion, and one row near
/// that ceiling makes every balance walk over its account return `Overflow` with nothing in
/// the error to say which row caused it. A bad client fails loudly here instead.
pub const MAX_AMOUNT_MINOR: i64 = 1_000_000_000_000_000;

/// Earliest instant a row may be booked at.
pub const EARLIEST_BOOKED_AT: OffsetDateTime = datetime!(1900-01-01 00:00:00 UTC);
/// Latest instant a row may be booked at. Future-dated rows are the forecast, but a
/// forecast reaching the twenty-third century is a corrupt date, not a plan.
pub const LATEST_BOOKED_AT: OffsetDateTime = datetime!(2200-01-01 00:00:00 UTC);
/// Scale applied to stored FX rates: `rate_e8 = rate * 100_000_000`.
pub const FX_RATE_SCALE: i64 = 100_000_000;

/// A row that cannot be stored as it stands.
///
/// Every variant names the field, so the message a user or a log sees says what to fix.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    /// A required text field was empty or only whitespace.
    #[error("{field} must not be empty")]
    Empty {
        /// The offending field.
        field: &'static str,
    },

    /// A text field was longer than the schema allows.
    #[error("{field} is {actual} characters, limit is {max}")]
    TooLong {
        /// The offending field.
        field: &'static str,
        /// The limit.
        max: usize,
        /// What was supplied.
        actual: usize,
    },

    /// An amount was negative where the sign is carried by [`Direction`] instead.
    #[error("{field} must not be negative; direction carries the sign")]
    NegativeAmount {
        /// The offending field.
        field: &'static str,
    },

    /// An amount was zero or below where a positive value is required.
    #[error("{field} must be greater than zero")]
    NotPositive {
        /// The offending field.
        field: &'static str,
    },

    /// Two amounts on the same row disagreed about currency.
    #[error("{field} is in {found} but {expected} was expected")]
    CurrencyMismatch {
        /// The offending field.
        field: &'static str,
        /// The currency the row is denominated in.
        expected: Currency,
        /// The currency actually found.
        found: Currency,
    },

    /// A row pointed at itself where a different row was required.
    #[error("{field} must not point at the row itself")]
    SelfReference {
        /// The offending field.
        field: &'static str,
    },

    /// A transfer leg carried a category. Transfers are netted, so categorising one would
    /// double-count it as spending.
    #[error("a transfer leg must not carry a category")]
    CategorisedTransfer,

    /// A recurrence interval below one would never advance.
    #[error("recurrence interval must be at least 1")]
    InvalidInterval,

    /// A day-of-month outside 1..=31.
    #[error("day of month {0} is out of range")]
    InvalidMonthDay(u8),

    /// An end date before the start date.
    #[error("{field} ends before it starts")]
    EndsBeforeStart {
        /// The offending field.
        field: &'static str,
    },

    /// An FX rate between a currency and itself.
    #[error("an fx rate must be between two different currencies")]
    SameCurrencyPair,

    /// An amount beyond [`MAX_AMOUNT_MINOR`], which would overflow a balance walk.
    #[error("{field} of {minor} minor units exceeds the {max} limit")]
    AmountTooLarge {
        /// The offending field.
        field: &'static str,
        /// What was supplied.
        minor: i64,
        /// The limit.
        max: i64,
    },

    /// A timestamp outside [`EARLIEST_BOOKED_AT`]..=[`LATEST_BOOKED_AT`].
    #[error("{field} is outside the range the ledger accepts")]
    TimestampOutOfRange {
        /// The offending field.
        field: &'static str,
    },

    /// `updated_at` before `created_at`, which makes a last-write-wins conflict
    /// unexplainable.
    #[error("{field} is before the row was created")]
    TimestampBeforeCreation {
        /// The offending field.
        field: &'static str,
    },

    /// A basis-points field above [`BPS_SCALE`].
    #[error("{field} is {value} basis points; the maximum is {max}")]
    BasisPointsOutOfRange {
        /// The offending field.
        field: &'static str,
        /// What was supplied.
        value: u16,
        /// The limit.
        max: u16,
    },

    /// A day-of-week outside 0..=6.
    #[error("day of week {0} is out of range")]
    InvalidWeekday(u8),

    /// A recurrence whose cadence and day fields disagree — a weekly rule with a day of the
    /// month, or a monthly rule with no day at all.
    #[error("a {freq} recurrence {problem}")]
    CadenceMismatch {
        /// The cadence that was set.
        freq: RecurrenceFreq,
        /// What is wrong with the day fields.
        problem: &'static str,
    },

    /// Text that must be lowercase hexadecimal of an exact length was not.
    #[error("{field} must be {len} lowercase hex characters")]
    NotHex {
        /// The offending field.
        field: &'static str,
        /// The required length.
        len: usize,
    },

    /// A stored path that could escape the directory it is resolved against.
    #[error("{field} must be a relative path with no `..` segment: {reason}")]
    UnsafePath {
        /// The offending field.
        field: &'static str,
        /// Which rule it broke.
        reason: &'static str,
    },

    /// A category nested more than one level deep.
    #[error("categories nest one level only; `{parent}` already has a parent")]
    NestedTooDeep {
        /// The proposed parent, which is itself a child.
        parent: String,
    },

    /// An inbox row whose status and links disagree.
    #[error("an inbox item with status `{status}` {problem}")]
    InboxStateMismatch {
        /// The status that was set.
        status: InboxStatus,
        /// What is wrong with the links.
        problem: &'static str,
    },

    /// A set of transfer legs that is not a well-formed pair.
    #[error("a transfer must be exactly two legs: {0}")]
    MalformedTransfer(&'static str),
}

/// The columns every synced row carries.
///
/// Sync is row-level last-write-wins on `updated_at`, with `device_id` recording the last
/// writer so a conflict can be explained rather than merely resolved. Deletes are soft so a
/// delete on one device is not resurrected by another that has not pulled yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncMeta {
    /// When the row was first written, UTC.
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    /// When the row was last written, UTC. The tiebreak for last-write-wins.
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    /// Set when the row is deleted; the row itself stays.
    #[serde(with = "time::serde::rfc3339::option", default)]
    pub deleted_at: Option<OffsetDateTime>,
    /// The installation that last wrote the row.
    pub device_id: DeviceId,
}

impl SyncMeta {
    /// True once the row has been soft-deleted.
    #[must_use]
    pub const fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }

    /// Checks the bookkeeping columns.
    ///
    /// An `updated_at` before `created_at` is what makes a last-write-wins conflict
    /// impossible to explain afterwards, so it is refused rather than tolerated.
    ///
    /// # Errors
    /// [`ValidationError`] naming the first field that fails.
    pub fn validate(&self) -> Result<(), ValidationError> {
        check_instant("created_at", self.created_at)?;
        check_instant("updated_at", self.updated_at)?;
        if self.updated_at < self.created_at {
            return Err(ValidationError::TimestampBeforeCreation {
                field: "updated_at",
            });
        }
        if let Some(deleted_at) = self.deleted_at {
            check_instant("deleted_at", deleted_at)?;
            if deleted_at < self.created_at {
                return Err(ValidationError::TimestampBeforeCreation {
                    field: "deleted_at",
                });
            }
        }
        Ok(())
    }
}

/// A place money sits: cash, a mobile money wallet, a bank account or a card.
///
/// One currency per account, fixed at creation. An account holding two currencies is two
/// accounts; conflating account, container and currency is what blocks multi-currency
/// reporting later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// Row id.
    pub id: AccountId,
    /// What the user calls it.
    pub name: String,
    /// Cash, MoMo, bank or card.
    pub kind: AccountKind,
    /// The one currency this account is denominated in.
    pub currency: Currency,
    /// The balance the ledger starts from, before any transaction.
    pub opening_balance: Money,
    /// Hidden from pickers and totals once set, without deleting history.
    #[serde(with = "time::serde::rfc3339::option", default)]
    pub archived_at: Option<OffsetDateTime>,
    /// Ordering in the account list.
    pub sort_order: i32,
    /// Sync bookkeeping.
    #[serde(flatten)]
    pub sync: SyncMeta,
}

impl Account {
    /// Checks the row against the schema's constraints.
    ///
    /// # Errors
    /// [`ValidationError`] naming the first field that fails.
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.sync.validate()?;
        check_text("account.name", &self.name, MAX_NAME_LEN)?;
        check_amount("account.opening_balance", self.opening_balance)?;
        check_currency(
            "account.opening_balance",
            self.opening_balance,
            self.currency,
        )
    }

    /// True once archived.
    #[must_use]
    pub const fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }
}

/// One movement of money in or out of one account.
///
/// The amount is always non-negative; [`Direction`] carries the sign. Use
/// [`Transaction::signed_amount`] for anything that accumulates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    /// Row id.
    pub id: TxId,
    /// The account the money moved in or out of.
    pub account_id: AccountId,
    /// Which way it moved.
    pub direction: Direction,
    /// How much, non-negative, in the account's currency.
    pub amount: Money,
    /// When it happened, UTC. Sorting key for the balance walk; may be in the future.
    #[serde(with = "time::serde::rfc3339")]
    pub booked_at: OffsetDateTime,
    /// Assigned by the categorisation ladder, or `None` for uncategorised and for transfers.
    pub category_id: Option<CategoryId>,
    /// Short human-readable description.
    pub label: String,
    /// Merchant, sender or recipient, where known.
    pub counterparty: Option<String>,
    /// Free text from the user.
    pub note: Option<String>,
    /// Pending rows in the future are the forecast.
    pub status: TxStatus,
    /// Where the row came from.
    pub source: TxSource,
    /// Source-specific reference: a statement line number, an inbox item, a receipt file.
    pub source_ref: Option<String>,
    /// A stable id from the source institution, used to avoid re-importing the same row.
    pub external_id: Option<String>,
    /// Set when generated from a recurring template.
    pub recurring_id: Option<RecurringId>,
    /// Shared by the two legs of a transfer, which the balance walk nets against each other.
    pub transfer_id: Option<TransferId>,
    /// Sync bookkeeping.
    #[serde(flatten)]
    pub sync: SyncMeta,
}

impl Transaction {
    /// The amount with [`Direction`] applied: positive for money in, negative for money out.
    ///
    /// # Errors
    /// [`crate::money::MoneyError::Overflow`] only for `i64::MIN`, which validation rejects.
    pub fn signed_amount(&self) -> Result<Money, crate::money::MoneyError> {
        match self.direction {
            Direction::In => Ok(self.amount),
            Direction::Out => self.amount.try_neg(),
        }
    }

    /// True for one leg of a transfer between the user's own accounts.
    #[must_use]
    pub const fn is_transfer(&self) -> bool {
        self.transfer_id.is_some()
    }

    /// Checks the row against the schema's constraints.
    ///
    /// Does not check that `amount.currency` matches the account's currency: that needs the
    /// account, and is enforced where the two are written together.
    ///
    /// # Errors
    /// [`ValidationError`] naming the first field that fails.
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.sync.validate()?;
        check_text("transaction.label", &self.label, MAX_LABEL_LEN)?;
        check_optional_text(
            "transaction.counterparty",
            self.counterparty.as_deref(),
            MAX_COUNTERPARTY_LEN,
        )?;
        check_optional_text("transaction.note", self.note.as_deref(), MAX_NOTE_LEN)?;
        check_optional_text(
            "transaction.source_ref",
            self.source_ref.as_deref(),
            MAX_SOURCE_REF_LEN,
        )?;
        check_optional_text(
            "transaction.external_id",
            self.external_id.as_deref(),
            MAX_EXTERNAL_ID_LEN,
        )?;
        check_instant("transaction.booked_at", self.booked_at)?;
        if self.amount.is_negative() {
            return Err(ValidationError::NegativeAmount {
                field: "transaction.amount",
            });
        }
        check_amount("transaction.amount", self.amount)?;
        if self.is_transfer() && self.category_id.is_some() {
            return Err(ValidationError::CategorisedTransfer);
        }
        Ok(())
    }

    /// Checks the row against the account it belongs to.
    ///
    /// Separate from [`Transaction::validate`] because it needs the account. Both databases
    /// enforce the same thing with a composite foreign key; this is the check that runs
    /// before the write, where the error can name the fields.
    ///
    /// # Errors
    /// [`ValidationError::CurrencyMismatch`] if the row is not denominated in the account's
    /// currency, or [`ValidationError::SelfReference`] if it belongs to a different account.
    pub fn validate_against(&self, account: &Account) -> Result<(), ValidationError> {
        if self.account_id != account.id {
            return Err(ValidationError::SelfReference {
                field: "transaction.account_id",
            });
        }
        check_currency("transaction.amount", self.amount, account.currency)
    }
}

/// Checks that a set of rows is a well-formed transfer.
///
/// The databases cannot enforce this: the legs arrive separately over sync, so a deferred
/// constraint would not hold, and nothing in SQL counts rows sharing a column at write time.
/// So the invariant lives here and is checked where a pair is written.
///
/// A transfer is exactly two legs, in opposite directions, between two different accounts,
/// for the same amount in the same currency, at the same instant, sharing one `transfer_id`.
///
/// Cross-currency moves are refused deliberately. Netting two legs of different currencies
/// has no defined meaning without a rate, and inventing one here would put an invented
/// number into the balance walk. Record the two sides as ordinary rows until the product
/// decides how to carry the rate.
///
/// # Errors
/// [`ValidationError::MalformedTransfer`] naming the rule that was broken, or whatever
/// [`Transaction::validate`] rejects on either leg.
pub fn validate_transfer_pair(legs: &[Transaction]) -> Result<(), ValidationError> {
    let [first, second] = legs else {
        return Err(ValidationError::MalformedTransfer(
            "exactly two legs are required",
        ));
    };

    first.validate()?;
    second.validate()?;

    let (Some(left), Some(right)) = (first.transfer_id, second.transfer_id) else {
        return Err(ValidationError::MalformedTransfer(
            "both legs must carry a transfer_id",
        ));
    };
    if left != right {
        return Err(ValidationError::MalformedTransfer(
            "the two legs must share one transfer_id",
        ));
    }
    if first.id == second.id {
        return Err(ValidationError::MalformedTransfer(
            "the two legs must be different rows",
        ));
    }
    if first.direction == second.direction {
        return Err(ValidationError::MalformedTransfer(
            "one leg must be out and the other in",
        ));
    }
    if first.account_id == second.account_id {
        return Err(ValidationError::MalformedTransfer(
            "the two legs must be in different accounts",
        ));
    }
    if first.amount.currency() != second.amount.currency() {
        return Err(ValidationError::MalformedTransfer(
            "cross-currency transfers are not supported; record the two sides separately",
        ));
    }
    if first.amount != second.amount {
        return Err(ValidationError::MalformedTransfer(
            "both legs must be for the same amount",
        ));
    }
    if first.booked_at != second.booked_at {
        return Err(ValidationError::MalformedTransfer(
            "both legs must be booked at the same instant",
        ));
    }
    Ok(())
}

/// A spending or income category, optionally nested one level under a parent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Category {
    /// Row id.
    pub id: CategoryId,
    /// Display name.
    pub name: String,
    /// Parent category, for one level of nesting.
    pub parent_id: Option<CategoryId>,
    /// Income or expense.
    pub kind: CategoryKind,
    /// Icon name from the app's icon set.
    pub icon: Option<String>,
    /// Shipped by a rule pack and not deletable by the user.
    pub is_system: bool,
    /// Ordering within its level.
    pub sort_order: i32,
    /// Sync bookkeeping.
    #[serde(flatten)]
    pub sync: SyncMeta,
}

impl Category {
    /// Checks the row against the schema's constraints.
    ///
    /// # Errors
    /// [`ValidationError`] naming the first field that fails.
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.sync.validate()?;
        check_text("category.name", &self.name, MAX_NAME_LEN)?;
        check_optional_text("category.icon", self.icon.as_deref(), MAX_NAME_LEN)?;
        if self.parent_id == Some(self.id) {
            return Err(ValidationError::SelfReference {
                field: "category.parent_id",
            });
        }
        Ok(())
    }

    /// Checks the row against the parent it names.
    ///
    /// Categories nest one level. A grandparent chain is refused here rather than left for
    /// the breakdown to decide what to do with at report time; nothing in SQL can express
    /// "the parent must itself be a root" without recursion.
    ///
    /// # Errors
    /// [`ValidationError`] if the parent is the wrong row, or is itself a child.
    pub fn validate_under(&self, parent: Option<&Category>) -> Result<(), ValidationError> {
        match (self.parent_id, parent) {
            (None, _) => Ok(()),
            (Some(_), None) => Err(ValidationError::SelfReference {
                field: "category.parent_id",
            }),
            (Some(expected), Some(parent)) if parent.id != expected => {
                Err(ValidationError::SelfReference {
                    field: "category.parent_id",
                })
            }
            (Some(_), Some(parent)) => {
                if parent.parent_id.is_some() {
                    return Err(ValidationError::NestedTooDeep {
                        parent: parent.name.clone(),
                    });
                }
                Ok(())
            }
        }
    }
}

/// A stored pattern that assigns a category — step 2 of the categorisation ladder.
///
/// Patterns are data, not code. A malformed pattern is rejected when it is saved, loudly;
/// it is never compiled with errors suppressed and quietly skipped at match time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategoryRule {
    /// Row id.
    pub id: CategoryRuleId,
    /// Substring, or regular expression when `is_regex`.
    pub pattern: String,
    /// Which field of the candidate the pattern is tried against.
    pub matched_field: MatchedField,
    /// The category to assign on a hit.
    pub category_id: CategoryId,
    /// Lower numbers are tried first. Ties break on `id` so matching is deterministic.
    pub priority: i32,
    /// Treat `pattern` as a regular expression rather than a case-insensitive substring.
    pub is_regex: bool,
    /// Disabled rules stay for reference but never match.
    pub enabled: bool,
    /// Who wrote it.
    pub origin: RuleOrigin,
    /// The pack it came from, when `origin` is `pack`.
    pub pack_id: Option<RulePackId>,
    /// Sync bookkeeping.
    #[serde(flatten)]
    pub sync: SyncMeta,
}

impl CategoryRule {
    /// Checks the row against the schema's constraints.
    ///
    /// Does not compile `pattern`; that happens in the matcher, which reports a bad regular
    /// expression as a save-time failure.
    ///
    /// # Errors
    /// [`ValidationError`] naming the first field that fails.
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.sync.validate()?;
        check_text("category_rule.pattern", &self.pattern, MAX_PATTERN_LEN)?;
        if (self.origin == RuleOrigin::Pack) != self.pack_id.is_some() {
            return Err(ValidationError::SelfReference {
                field: "category_rule.pack_id",
            });
        }
        Ok(())
    }
}

/// A template that generates concrete pending transaction rows forward in time.
///
/// Generating rows, rather than evaluating a rule on the fly, is what lets the forecast and
/// the history be one list walked by one function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecurringRule {
    /// Row id.
    pub id: RecurringId,
    /// Display name for the template itself.
    pub name: String,
    /// The account rows are generated into.
    pub account_id: AccountId,
    /// Direction of the generated rows.
    pub direction: Direction,
    /// Amount of each generated row, non-negative.
    pub amount: Money,
    /// Category applied to generated rows.
    pub category_id: Option<CategoryId>,
    /// Label applied to generated rows.
    pub label: String,
    /// Counterparty applied to generated rows.
    pub counterparty: Option<String>,
    /// Daily, weekly, monthly or yearly.
    pub freq: RecurrenceFreq,
    /// Every `interval` periods. At least 1.
    pub interval: u16,
    /// Day of month for monthly and yearly cadences, 1..=31. A 31 in a short month clamps
    /// to the last day rather than skipping.
    pub by_month_day: Option<u8>,
    /// Day of week for weekly cadences, Monday = 0.
    pub by_weekday: Option<u8>,
    /// First occurrence.
    #[serde(with = "iso_date")]
    pub starts_on: Date,
    /// Last occurrence, if the series ends.
    #[serde(with = "iso_date::option", default)]
    pub ends_on: Option<Date>,
    /// How far ahead rows have already been generated, so generation is idempotent.
    #[serde(with = "iso_date::option", default)]
    pub generated_through: Option<Date>,
    /// IANA zone the cadence is reckoned in, so a monthly bill lands on the right local day.
    pub timezone: String,
    /// Sync bookkeeping.
    #[serde(flatten)]
    pub sync: SyncMeta,
}

impl RecurringRule {
    /// Checks the row against the schema's constraints.
    ///
    /// # Errors
    /// [`ValidationError`] naming the first field that fails.
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.sync.validate()?;
        check_text("recurring.name", &self.name, MAX_NAME_LEN)?;
        check_text("recurring.label", &self.label, MAX_LABEL_LEN)?;
        check_optional_text(
            "recurring.counterparty",
            self.counterparty.as_deref(),
            MAX_COUNTERPARTY_LEN,
        )?;
        check_text("recurring.timezone", &self.timezone, MAX_TIMEZONE_LEN)?;
        if self.amount.is_negative() {
            return Err(ValidationError::NegativeAmount {
                field: "recurring.amount",
            });
        }
        check_amount("recurring.amount", self.amount)?;
        if self.interval == 0 {
            return Err(ValidationError::InvalidInterval);
        }
        if let Some(day) = self.by_month_day
            && !(1..=31).contains(&day)
        {
            return Err(ValidationError::InvalidMonthDay(day));
        }
        if let Some(weekday) = self.by_weekday
            && weekday > 6
        {
            return Err(ValidationError::InvalidWeekday(weekday));
        }
        self.validate_cadence()?;
        if let Some(end) = self.ends_on
            && end < self.starts_on
        {
            return Err(ValidationError::EndsBeforeStart { field: "recurring" });
        }
        Ok(())
    }

    /// Refuses a cadence whose day fields contradict it — a weekly rule carrying a day of
    /// the month, or a monthly rule with no day at all. Either one silently generates rows
    /// on a date nobody asked for.
    fn validate_cadence(&self) -> Result<(), ValidationError> {
        let mismatch = |problem| {
            Err(ValidationError::CadenceMismatch {
                freq: self.freq,
                problem,
            })
        };
        match self.freq {
            RecurrenceFreq::Daily => {
                if self.by_month_day.is_some() || self.by_weekday.is_some() {
                    return mismatch("takes no day field");
                }
            }
            RecurrenceFreq::Weekly => {
                if self.by_weekday.is_none() {
                    return mismatch("needs a day of the week");
                }
                if self.by_month_day.is_some() {
                    return mismatch("must not carry a day of the month");
                }
            }
            RecurrenceFreq::Monthly | RecurrenceFreq::Yearly => {
                if self.by_month_day.is_none() {
                    return mismatch("needs a day of the month");
                }
                if self.by_weekday.is_some() {
                    return mismatch("must not carry a day of the week");
                }
            }
        }
        Ok(())
    }
}

/// A cap on one category, in one currency, over one period.
///
/// Caps are per currency by design: a cedi cap and a dollar cap on the same category are two
/// budgets, not one converted number, because converting would make last month's verdict
/// move when the rate does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    /// Row id.
    pub id: BudgetId,
    /// The category being capped. `None` caps all spending in the currency.
    pub category_id: Option<CategoryId>,
    /// Weekly, monthly or yearly.
    pub period: BudgetPeriod,
    /// The cap. Its currency is the budget's currency.
    pub cap: Money,
    /// First period starts here.
    #[serde(with = "iso_date")]
    pub starts_on: Date,
    /// Last period ends here, if the budget ends.
    #[serde(with = "iso_date::option", default)]
    pub ends_on: Option<Date>,
    /// Fraction of the cap, in basis points, at which to warn. 8000 is 80%.
    pub warn_at_bps: u16,
    /// Sync bookkeeping.
    #[serde(flatten)]
    pub sync: SyncMeta,
}

impl Budget {
    /// Checks the row against the schema's constraints.
    ///
    /// # Errors
    /// [`ValidationError`] naming the first field that fails.
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.sync.validate()?;
        if self.cap.minor() <= 0 {
            return Err(ValidationError::NotPositive {
                field: "budget.cap",
            });
        }
        check_amount("budget.cap", self.cap)?;
        check_bps("budget.warn_at_bps", self.warn_at_bps)?;
        if let Some(end) = self.ends_on
            && end < self.starts_on
        {
            return Err(ValidationError::EndsBeforeStart { field: "budget" });
        }
        Ok(())
    }

    /// True when two budgets would both answer "am I over budget" for the same spending.
    ///
    /// Same scope — category, currency and period — and overlapping date ranges. Two caps on
    /// the same category in different currencies do not overlap: a cedi cap and a dollar cap
    /// are separate budgets by design.
    #[must_use]
    pub fn conflicts_with(&self, other: &Self) -> bool {
        if self.id == other.id || self.sync.is_deleted() || other.sync.is_deleted() {
            return false;
        }
        let same_scope = self.category_id == other.category_id
            && self.cap.currency() == other.cap.currency()
            && self.period == other.period;
        same_scope && self.overlaps_dates(other)
    }

    /// Whether the two date ranges intersect, treating an absent `ends_on` as open-ended.
    fn overlaps_dates(&self, other: &Self) -> bool {
        let starts_before_other_ends = other.ends_on.is_none_or(|end| self.starts_on <= end);
        let other_starts_before_self_ends = self.ends_on.is_none_or(|end| other.starts_on <= end);
        starts_before_other_ends && other_starts_before_self_ends
    }

    /// Checks one budget against the budgets already stored.
    ///
    /// Postgres enforces this with an exclusion constraint. SQLite cannot express one, so on
    /// the device this function is the enforcement, and it runs before the insert.
    ///
    /// # Errors
    /// [`ValidationError::EndsBeforeStart`] and the rest from [`Budget::validate`], or
    /// [`ValidationError::SelfReference`] naming `budget.starts_on` when another live budget
    /// already covers the same scope over an overlapping range.
    pub fn validate_against(&self, existing: &[Budget]) -> Result<(), ValidationError> {
        self.validate()?;
        if existing.iter().any(|other| self.conflicts_with(other)) {
            return Err(ValidationError::SelfReference {
                field: "budget.starts_on",
            });
        }
        Ok(())
    }
}

/// A conversion rate held for reporting only.
///
/// Reports convert at the rate for each transaction's own date, so a historical total does
/// not move when the cedi does. Stored as an integer scaled by [`FX_RATE_SCALE`], never a
/// float.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FxRate {
    /// The currency being converted from.
    pub base: Currency,
    /// The currency being converted to.
    pub quote: Currency,
    /// `base -> quote` rate multiplied by [`FX_RATE_SCALE`].
    pub rate_e8: i64,
    /// The date the rate applies to.
    #[serde(with = "iso_date")]
    pub as_of: Date,
}

impl FxRate {
    /// Checks the row against the schema's constraints.
    ///
    /// # Errors
    /// [`ValidationError`] if the pair is degenerate or the rate is not positive.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.base == self.quote {
            return Err(ValidationError::SameCurrencyPair);
        }
        if self.rate_e8 <= 0 {
            return Err(ValidationError::NotPositive {
                field: "fx_rate.rate_e8",
            });
        }
        Ok(())
    }
}

/// A parsed-but-unconfirmed candidate waiting for review.
///
/// Note what is absent: the raw message. An SMS body is parsed in memory and discarded. What
/// is kept is the parsed fields, the sender (which was on the allow-list before the body was
/// read at all) and a fingerprint, so the same alert is not offered twice. This is the Play
/// policy requirement for the SMS-based money management exception, enforced by the schema
/// having nowhere to put the text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxItem {
    /// Row id.
    pub id: InboxItemId,
    /// Which ingestion path produced it.
    pub source: InboxSource,
    /// When it arrived, UTC.
    #[serde(with = "time::serde::rfc3339")]
    pub received_at: OffsetDateTime,
    /// The allow-listed sender id for SMS, the file name for an import.
    pub origin_ref: Option<String>,
    /// A non-reversible digest of the source message or statement line, for "seen this".
    pub fingerprint: String,
    /// The candidate the parser produced.
    pub candidate: Candidate,
    /// Where the review has got to.
    pub status: InboxStatus,
    /// The row it became, once accepted.
    pub transaction_id: Option<TxId>,
    /// The existing row it was judged the same event as, once merged.
    pub duplicate_of: Option<TxId>,
    /// Sync bookkeeping. Inbox items stay on the device; only confirmed rows ever sync.
    #[serde(flatten)]
    pub sync: SyncMeta,
}

impl InboxItem {
    /// Checks the row against the schema's constraints.
    ///
    /// The status and the links have to agree: an accepted item names the transaction it
    /// became, a merged one names the row it duplicates, and the other two name neither. The
    /// device database enforces the same two rules; this is where the error can say which.
    ///
    /// # Errors
    /// [`ValidationError`] naming the first field that fails.
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.sync.validate()?;
        check_text(
            "inbox_item.fingerprint",
            &self.fingerprint,
            MAX_FINGERPRINT_LEN,
        )?;
        check_optional_text(
            "inbox_item.origin_ref",
            self.origin_ref.as_deref(),
            MAX_SOURCE_REF_LEN,
        )?;
        check_instant("inbox_item.received_at", self.received_at)?;
        self.candidate.validate()?;

        let problem = match self.status {
            InboxStatus::Accepted if self.transaction_id.is_none() => {
                Some("must name the transaction it became")
            }
            InboxStatus::Merged if self.duplicate_of.is_none() => {
                Some("must name the row it duplicates")
            }
            InboxStatus::Accepted | InboxStatus::Merged => None,
            InboxStatus::Pending | InboxStatus::Rejected => {
                if self.transaction_id.is_some() {
                    Some("must not name a transaction")
                } else if self.duplicate_of.is_some() {
                    Some("must not name a duplicate")
                } else {
                    None
                }
            }
        };
        match problem {
            Some(problem) => Err(ValidationError::InboxStateMismatch {
                status: self.status,
                problem,
            }),
            None => Ok(()),
        }
    }
}

/// What a parser produces: a proposed transaction, not yet a row in the ledger.
///
/// Handlers never produce a [`Transaction`] directly. SMS formats change without notice and a
/// silent mis-parse corrupts the numbers quietly, which is worse than missing one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    /// Account the parser matched, if it could.
    pub account_id: Option<AccountId>,
    /// Which way the money moved.
    pub direction: Direction,
    /// How much, non-negative.
    pub amount: Money,
    /// When the source says it happened, UTC.
    #[serde(with = "time::serde::rfc3339")]
    pub booked_at: OffsetDateTime,
    /// Proposed label.
    pub label: String,
    /// Proposed counterparty.
    pub counterparty: Option<String>,
    /// Category proposed by the ladder, with [`Candidate::category_confidence_bps`] saying
    /// how it was reached.
    pub category_id: Option<CategoryId>,
    /// Confidence in the proposed category, basis points. 10000 is an explicit source
    /// category; a guess from the user's own history scores lower.
    pub category_confidence_bps: u16,
    /// The institution's own reference for the movement, when the source carries one.
    pub external_id: Option<String>,
    /// The balance the source reported after the movement, when it carries one. Useful for
    /// reconciliation; never written to the ledger as a balance.
    pub reported_balance: Option<Money>,
}

impl Candidate {
    /// Checks the candidate before it is offered for review.
    ///
    /// # Errors
    /// [`ValidationError`] naming the first field that fails.
    pub fn validate(&self) -> Result<(), ValidationError> {
        check_text("candidate.label", &self.label, MAX_LABEL_LEN)?;
        check_optional_text(
            "candidate.counterparty",
            self.counterparty.as_deref(),
            MAX_COUNTERPARTY_LEN,
        )?;
        check_optional_text(
            "candidate.external_id",
            self.external_id.as_deref(),
            MAX_EXTERNAL_ID_LEN,
        )?;
        check_instant("candidate.booked_at", self.booked_at)?;
        if self.amount.is_negative() {
            return Err(ValidationError::NegativeAmount {
                field: "candidate.amount",
            });
        }
        check_amount("candidate.amount", self.amount)?;
        check_bps(
            "candidate.category_confidence_bps",
            self.category_confidence_bps,
        )?;
        if let Some(balance) = self.reported_balance {
            check_amount("candidate.reported_balance", balance)?;
            check_currency(
                "candidate.reported_balance",
                balance,
                self.amount.currency(),
            )?;
        }
        Ok(())
    }
}

/// A receipt image or imported file kept alongside a transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    /// Row id.
    pub id: AttachmentId,
    /// The transaction it belongs to.
    pub transaction_id: TxId,
    /// Path relative to the app's private attachment directory. Never an absolute path.
    pub relative_path: String,
    /// IANA media type.
    pub media_type: String,
    /// Size on disk, bytes.
    pub byte_size: i64,
    /// Lowercase hex SHA-256 of the contents, for deduplication and integrity.
    pub sha256: String,
    /// Sync bookkeeping.
    #[serde(flatten)]
    pub sync: SyncMeta,
}

impl Attachment {
    /// Checks the row against the schema's constraints.
    ///
    /// # Errors
    /// [`ValidationError`] naming the first field that fails.
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.sync.validate()?;
        check_relative_path("attachment.relative_path", &self.relative_path)?;
        check_text(
            "attachment.media_type",
            &self.media_type,
            MAX_MEDIA_TYPE_LEN,
        )?;
        check_hex("attachment.sha256", &self.sha256, SHA256_HEX_LEN)?;
        if self.byte_size < 0 {
            return Err(ValidationError::NegativeAmount {
                field: "attachment.byte_size",
            });
        }
        Ok(())
    }
}

/// One definition of a valid text field, measured the same way in both databases.
///
/// Trim first, then count characters. Both SQL schemas do `length(trim(…))` /
/// `char_length(btrim(…))` against the same limits, so a value accepted here is accepted by
/// the store it is written to and by the store it later syncs to. Three different answers to
/// "is this name valid" is how a row becomes unwritable on the device that did not create it.
fn check_text(field: &'static str, value: &str, max: usize) -> Result<(), ValidationError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ValidationError::Empty { field });
    }
    let actual = trimmed.chars().count();
    if actual > max {
        return Err(ValidationError::TooLong { field, max, actual });
    }
    Ok(())
}

/// Refuses an amount large enough to overflow a balance walk. See [`MAX_AMOUNT_MINOR`].
fn check_amount(field: &'static str, amount: Money) -> Result<(), ValidationError> {
    if amount.minor().unsigned_abs() > MAX_AMOUNT_MINOR.unsigned_abs() {
        return Err(ValidationError::AmountTooLarge {
            field,
            minor: amount.minor(),
            max: MAX_AMOUNT_MINOR,
        });
    }
    Ok(())
}

/// Refuses a timestamp outside the window the ledger accepts.
fn check_instant(field: &'static str, at: OffsetDateTime) -> Result<(), ValidationError> {
    if at < EARLIEST_BOOKED_AT || at > LATEST_BOOKED_AT {
        return Err(ValidationError::TimestampOutOfRange { field });
    }
    Ok(())
}

/// Refuses a basis-points value above one whole.
fn check_bps(field: &'static str, value: u16) -> Result<(), ValidationError> {
    if value > BPS_SCALE {
        return Err(ValidationError::BasisPointsOutOfRange {
            field,
            value,
            max: BPS_SCALE,
        });
    }
    Ok(())
}

/// Refuses anything but lowercase hex of an exact length.
fn check_hex(field: &'static str, value: &str, len: usize) -> Result<(), ValidationError> {
    let valid = value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if valid {
        Ok(())
    } else {
        Err(ValidationError::NotHex { field, len })
    }
}

/// Refuses a path that could escape the directory it is resolved against.
///
/// The value arrives over sync from another device, and the app joins it to its private
/// attachment directory and opens it. An absolute path or a `..` segment turns that join
/// into a read or a write somewhere else on the filesystem.
fn check_relative_path(field: &'static str, value: &str) -> Result<(), ValidationError> {
    check_text(field, value, MAX_PATH_LEN)?;
    let reason = if value.starts_with('/') {
        Some("it is absolute")
    } else if value.split('/').any(|segment| segment == "..") {
        Some("it climbs out with `..`")
    } else if value.contains('\\') {
        Some("it contains a backslash")
    } else if value.contains('\0') {
        Some("it contains a NUL byte")
    } else if value != value.trim() {
        Some("it is padded with whitespace")
    } else {
        None
    };
    match reason {
        Some(reason) => Err(ValidationError::UnsafePath { field, reason }),
        None => Ok(()),
    }
}

fn check_optional_text(
    field: &'static str,
    value: Option<&str>,
    max: usize,
) -> Result<(), ValidationError> {
    match value {
        None => Ok(()),
        Some(text) => check_text(field, text, max),
    }
}

fn check_currency(
    field: &'static str,
    amount: Money,
    expected: Currency,
) -> Result<(), ValidationError> {
    if amount.currency() == expected {
        Ok(())
    } else {
        Err(ValidationError::CurrencyMismatch {
            field,
            expected,
            found: amount.currency(),
        })
    }
}
