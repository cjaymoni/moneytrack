//! The rows, as they cross the bridge.
//!
//! One struct per core row, with the same fields in the same order and the four scalars in
//! their wire spelling (see [`crate::scalar`]). Nothing here holds a borrow, a handle or a
//! lifetime: values cross, handles do not.
//!
//! Every conversion into the core validates. A row arriving from Dart has been through a
//! SQLite column, a drift mapper and possibly a sync merge written by a client one release
//! ahead, so it is untrusted input in the ordinary sense — the boundary checks shape, range
//! and referential sanity here, and the rules downstream may then assume a valid row.

use mt_core::dedup;
use mt_core::model::{
    Account, BPS_SCALE, Budget, Candidate, Category, CategoryRule, RecurringRule, SyncMeta,
    Transaction,
};
use time::Duration;

use crate::enums::{
    AccountKind, BudgetPeriod, CategoryKind, Direction, MatchedField, RecurrenceFreq, RuleOrigin,
    TxSource, TxStatus,
};
use crate::error::BridgeError;
use crate::scalar::{self, MoneyDto};

/// The columns every synced row carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncMetaDto {
    /// When the row was first written, RFC 3339.
    pub created_at: String,
    /// When it was last written, RFC 3339. The merge point for last-write-wins.
    pub updated_at: String,
    /// When it was soft-deleted, if it was. RFC 3339.
    pub deleted_at: Option<String>,
    /// The installation that last wrote it.
    pub device_id: String,
}

impl SyncMetaDto {
    /// Converts and validates.
    ///
    /// # Errors
    /// [`BridgeError`] naming the field at fault.
    pub fn to_core(&self) -> Result<SyncMeta, BridgeError> {
        let meta = SyncMeta {
            created_at: scalar::instant("created_at", &self.created_at)?,
            updated_at: scalar::instant("updated_at", &self.updated_at)?,
            deleted_at: scalar::optional_instant("deleted_at", self.deleted_at.as_ref())?,
            device_id: scalar::id("device_id", &self.device_id)?,
        };
        meta.validate()
            .map_err(|reason| BridgeError::invalid_row("sync", reason))?;
        Ok(meta)
    }

    /// Renders a core value back.
    ///
    /// # Errors
    /// [`BridgeError::Unrepresentable`] for an instant RFC 3339 cannot spell.
    pub fn from_core(meta: &SyncMeta) -> Result<Self, BridgeError> {
        Ok(Self {
            created_at: scalar::format_instant("created_at", meta.created_at)?,
            updated_at: scalar::format_instant("updated_at", meta.updated_at)?,
            deleted_at: match meta.deleted_at {
                Some(at) => Some(scalar::format_instant("deleted_at", at)?),
                None => None,
            },
            device_id: meta.device_id.to_string(),
        })
    }
}

/// An account: one place money sits, in exactly one currency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountDto {
    /// Row id.
    pub id: String,
    /// What the user calls it.
    pub name: String,
    /// Cash, mobile money, bank or card.
    pub kind: AccountKind,
    /// ISO 4217 code. Every row in this account is denominated in it.
    pub currency: String,
    /// The balance before any transaction.
    pub opening_balance: MoneyDto,
    /// When it was archived, if it was. RFC 3339.
    pub archived_at: Option<String>,
    /// Display order.
    pub sort_order: i32,
    /// Sync columns.
    pub sync: SyncMetaDto,
}

impl AccountDto {
    /// Converts and validates.
    ///
    /// # Errors
    /// [`BridgeError`] naming the field at fault.
    pub fn to_core(&self) -> Result<Account, BridgeError> {
        let account = Account {
            id: scalar::id("account.id", &self.id)?,
            name: self.name.clone(),
            kind: self.kind.into(),
            currency: scalar::currency("account.currency", &self.currency)?,
            opening_balance: self.opening_balance.parse("account.opening_balance")?,
            archived_at: scalar::optional_instant(
                "account.archived_at",
                self.archived_at.as_ref(),
            )?,
            sort_order: self.sort_order,
            sync: self.sync.to_core()?,
        };
        account
            .validate()
            .map_err(|reason| BridgeError::invalid_row("account", reason))?;
        Ok(account)
    }
}

/// A transaction: the row everything else is derived from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionDto {
    /// Row id.
    pub id: String,
    /// The account it moves.
    pub account_id: String,
    /// In or out, from that account's point of view.
    pub direction: Direction,
    /// The amount, always positive; `direction` carries the sign.
    pub amount: MoneyDto,
    /// When it happened, or is expected to. RFC 3339.
    pub booked_at: String,
    /// The category, absent on a transfer leg and on anything uncategorised.
    pub category_id: Option<String>,
    /// What it was.
    pub label: String,
    /// Merchant, sender or recipient.
    pub counterparty: Option<String>,
    /// Free text.
    pub note: Option<String>,
    /// Pending or confirmed.
    pub status: TxStatus,
    /// Where the row came from.
    pub source: TxSource,
    /// A reference within that source.
    pub source_ref: Option<String>,
    /// The institution's own reference, unique per account.
    pub external_id: Option<String>,
    /// The template that generated it.
    pub recurring_id: Option<String>,
    /// Pairs the two legs of a transfer.
    pub transfer_id: Option<String>,
    /// Sync columns.
    pub sync: SyncMetaDto,
}

impl TransactionDto {
    /// Converts and validates.
    ///
    /// # Errors
    /// [`BridgeError`] naming the field at fault.
    pub fn to_core(&self) -> Result<Transaction, BridgeError> {
        let transaction = Transaction {
            id: scalar::id("transaction.id", &self.id)?,
            account_id: scalar::id("transaction.account_id", &self.account_id)?,
            direction: self.direction.into(),
            amount: self.amount.parse("transaction.amount")?,
            booked_at: scalar::instant("transaction.booked_at", &self.booked_at)?,
            category_id: scalar::optional_id("transaction.category_id", self.category_id.as_ref())?,
            label: self.label.clone(),
            counterparty: self.counterparty.clone(),
            note: self.note.clone(),
            status: self.status.into(),
            source: self.source.into(),
            source_ref: self.source_ref.clone(),
            external_id: self.external_id.clone(),
            recurring_id: scalar::optional_id(
                "transaction.recurring_id",
                self.recurring_id.as_ref(),
            )?,
            transfer_id: scalar::optional_id("transaction.transfer_id", self.transfer_id.as_ref())?,
            sync: self.sync.to_core()?,
        };
        transaction
            .validate()
            .map_err(|reason| BridgeError::invalid_row("transaction", reason))?;
        Ok(transaction)
    }

    /// Renders a core row back — the path generated rows take out of [`crate::api`].
    ///
    /// # Errors
    /// [`BridgeError::Unrepresentable`] for an instant RFC 3339 cannot spell.
    pub fn from_core(transaction: &Transaction) -> Result<Self, BridgeError> {
        Ok(Self {
            id: transaction.id.to_string(),
            account_id: transaction.account_id.to_string(),
            direction: transaction.direction.into(),
            amount: transaction.amount.into(),
            booked_at: scalar::format_instant("transaction.booked_at", transaction.booked_at)?,
            category_id: transaction.category_id.map(|id| id.to_string()),
            label: transaction.label.clone(),
            counterparty: transaction.counterparty.clone(),
            note: transaction.note.clone(),
            status: transaction.status.into(),
            source: transaction.source.into(),
            source_ref: transaction.source_ref.clone(),
            external_id: transaction.external_id.clone(),
            recurring_id: transaction.recurring_id.map(|id| id.to_string()),
            transfer_id: transaction.transfer_id.map(|id| id.to_string()),
            sync: SyncMetaDto::from_core(&transaction.sync)?,
        })
    }
}

/// A category, possibly nested under a parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryDto {
    /// Row id.
    pub id: String,
    /// What the user calls it.
    pub name: String,
    /// The category it sits under.
    pub parent_id: Option<String>,
    /// Income or expense.
    pub kind: CategoryKind,
    /// Icon name, resolved by the UI.
    pub icon: Option<String>,
    /// Whether it was shipped rather than created by the user.
    pub is_system: bool,
    /// Display order.
    pub sort_order: i32,
    /// Sync columns.
    pub sync: SyncMetaDto,
}

impl CategoryDto {
    /// Converts and validates.
    ///
    /// # Errors
    /// [`BridgeError`] naming the field at fault.
    pub fn to_core(&self) -> Result<Category, BridgeError> {
        let category = Category {
            id: scalar::id("category.id", &self.id)?,
            name: self.name.clone(),
            parent_id: scalar::optional_id("category.parent_id", self.parent_id.as_ref())?,
            kind: self.kind.into(),
            icon: self.icon.clone(),
            is_system: self.is_system,
            sort_order: self.sort_order,
            sync: self.sync.to_core()?,
        };
        category
            .validate()
            .map_err(|reason| BridgeError::invalid_row("category", reason))?;
        Ok(category)
    }
}

/// A stored pattern that assigns a category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryRuleDto {
    /// Row id.
    pub id: String,
    /// The pattern: a substring, or a regular expression when `is_regex`.
    pub pattern: String,
    /// The field it is tried against.
    pub matched_field: MatchedField,
    /// The category it assigns.
    pub category_id: String,
    /// Lower runs first.
    pub priority: i32,
    /// Whether `pattern` is a regular expression.
    pub is_regex: bool,
    /// Disabled rules are kept but never compiled.
    pub enabled: bool,
    /// User-written or shipped in a pack.
    pub origin: RuleOrigin,
    /// The pack it came from.
    pub pack_id: Option<String>,
    /// Sync columns.
    pub sync: SyncMetaDto,
}

impl CategoryRuleDto {
    /// Converts and validates.
    ///
    /// # Errors
    /// [`BridgeError`] naming the field at fault.
    pub fn to_core(&self) -> Result<CategoryRule, BridgeError> {
        let rule = CategoryRule {
            id: scalar::id("category_rule.id", &self.id)?,
            pattern: self.pattern.clone(),
            matched_field: self.matched_field.into(),
            category_id: scalar::id("category_rule.category_id", &self.category_id)?,
            priority: self.priority,
            is_regex: self.is_regex,
            enabled: self.enabled,
            origin: self.origin.into(),
            pack_id: scalar::optional_id("category_rule.pack_id", self.pack_id.as_ref())?,
            sync: self.sync.to_core()?,
        };
        rule.validate()
            .map_err(|reason| BridgeError::invalid_row("category_rule", reason))?;
        Ok(rule)
    }
}

/// A recurring template. It generates pending rows; it is never evaluated on the fly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecurringRuleDto {
    /// Row id.
    pub id: String,
    /// What the user calls it.
    pub name: String,
    /// The account the generated rows land in.
    pub account_id: String,
    /// In or out.
    pub direction: Direction,
    /// The amount each occurrence carries.
    pub amount: MoneyDto,
    /// The category each occurrence carries.
    pub category_id: Option<String>,
    /// The label each occurrence carries.
    pub label: String,
    /// The counterparty each occurrence carries.
    pub counterparty: Option<String>,
    /// Daily, weekly, monthly or yearly.
    pub freq: RecurrenceFreq,
    /// Every `interval` periods. At least 1.
    pub interval: u16,
    /// Day of month, 1..=31, for a monthly or yearly template.
    pub by_month_day: Option<u8>,
    /// Day of week for a weekly template, Monday = 0, through 6 for Sunday.
    ///
    /// Zero-based, like the core and both schemas. The temptation to make it one-based for
    /// Dart is the sort of convenience that puts every weekly bill a day late for ever.
    pub by_weekday: Option<u8>,
    /// First local date it fires, `YYYY-MM-DD`.
    pub starts_on: String,
    /// Last local date it may fire, `YYYY-MM-DD`.
    pub ends_on: Option<String>,
    /// How far rows have already been generated, `YYYY-MM-DD`.
    pub generated_through: Option<String>,
    /// The IANA zone the local dates are read in.
    pub timezone: String,
    /// Sync columns.
    pub sync: SyncMetaDto,
}

impl RecurringRuleDto {
    /// Converts and validates.
    ///
    /// # Errors
    /// [`BridgeError`] naming the field at fault.
    pub fn to_core(&self) -> Result<RecurringRule, BridgeError> {
        let rule = RecurringRule {
            id: scalar::id("recurring.id", &self.id)?,
            name: self.name.clone(),
            account_id: scalar::id("recurring.account_id", &self.account_id)?,
            direction: self.direction.into(),
            amount: self.amount.parse("recurring.amount")?,
            category_id: scalar::optional_id("recurring.category_id", self.category_id.as_ref())?,
            label: self.label.clone(),
            counterparty: self.counterparty.clone(),
            freq: self.freq.into(),
            interval: self.interval,
            by_month_day: self.by_month_day,
            by_weekday: self.by_weekday,
            starts_on: scalar::date("recurring.starts_on", &self.starts_on)?,
            ends_on: scalar::optional_date("recurring.ends_on", self.ends_on.as_ref())?,
            generated_through: scalar::optional_date(
                "recurring.generated_through",
                self.generated_through.as_ref(),
            )?,
            timezone: self.timezone.clone(),
            sync: self.sync.to_core()?,
        };
        rule.validate()
            .map_err(|reason| BridgeError::invalid_row("recurring", reason))?;
        Ok(rule)
    }
}

/// A cap on one category, in one currency, over a repeating window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetDto {
    /// Row id.
    pub id: String,
    /// The category capped, or absent for a cap on everything.
    pub category_id: Option<String>,
    /// How often it renews.
    pub period: BudgetPeriod,
    /// The cap. Its currency is the only one counted against it.
    pub cap: MoneyDto,
    /// First local date it applies, `YYYY-MM-DD`.
    pub starts_on: String,
    /// Last local date it applies, `YYYY-MM-DD`.
    pub ends_on: Option<String>,
    /// Warn at this many basis points of the cap. 8000 is 80%; 0 means never warn.
    pub warn_at_bps: u16,
    /// Sync columns.
    pub sync: SyncMetaDto,
}

impl BudgetDto {
    /// Converts and validates.
    ///
    /// # Errors
    /// [`BridgeError`] naming the field at fault.
    pub fn to_core(&self) -> Result<Budget, BridgeError> {
        let budget = Budget {
            id: scalar::id("budget.id", &self.id)?,
            category_id: scalar::optional_id("budget.category_id", self.category_id.as_ref())?,
            period: self.period.into(),
            cap: self.cap.parse("budget.cap")?,
            starts_on: scalar::date("budget.starts_on", &self.starts_on)?,
            ends_on: scalar::optional_date("budget.ends_on", self.ends_on.as_ref())?,
            warn_at_bps: self.warn_at_bps,
            sync: self.sync.to_core()?,
        };
        budget
            .validate()
            .map_err(|reason| BridgeError::invalid_row("budget", reason))?;
        Ok(budget)
    }
}

/// A parsed-but-unconfirmed row, on its way to the inbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateDto {
    /// The account it is thought to belong to.
    pub account_id: Option<String>,
    /// In or out.
    pub direction: Direction,
    /// The amount, always positive.
    pub amount: MoneyDto,
    /// When it happened. RFC 3339.
    pub booked_at: String,
    /// What it was.
    pub label: String,
    /// Merchant, sender or recipient.
    pub counterparty: Option<String>,
    /// The category proposed so far.
    pub category_id: Option<String>,
    /// How much to trust that category, in basis points.
    pub category_confidence_bps: u16,
    /// The institution's own reference.
    pub external_id: Option<String>,
    /// A balance the source stated, for reconciliation.
    pub reported_balance: Option<MoneyDto>,
}

impl CandidateDto {
    /// Converts and validates.
    ///
    /// # Errors
    /// [`BridgeError`] naming the field at fault.
    pub fn to_core(&self) -> Result<Candidate, BridgeError> {
        let candidate = Candidate {
            account_id: scalar::optional_id("candidate.account_id", self.account_id.as_ref())?,
            direction: self.direction.into(),
            amount: self.amount.parse("candidate.amount")?,
            booked_at: scalar::instant("candidate.booked_at", &self.booked_at)?,
            label: self.label.clone(),
            counterparty: self.counterparty.clone(),
            category_id: scalar::optional_id("candidate.category_id", self.category_id.as_ref())?,
            category_confidence_bps: self.category_confidence_bps,
            external_id: self.external_id.clone(),
            reported_balance: match &self.reported_balance {
                Some(money) => Some(money.parse("candidate.reported_balance")?),
                None => None,
            },
        };
        candidate
            .validate()
            .map_err(|reason| BridgeError::invalid_row("candidate", reason))?;
        Ok(candidate)
    }
}

/// The thresholds duplicate matching uses. Absent means the core's defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchRulesDto {
    /// Outside this many seconds, two rows are never the same event.
    pub window_seconds: i64,
    /// Inside this many seconds, they are close enough to score for it.
    pub close_window_seconds: i64,
    /// Suspicions scoring below this many basis points are not reported.
    pub report_at_or_above_bps: u16,
}

impl MatchRulesDto {
    /// Checks the thresholds and converts them.
    ///
    /// Validated like every other value entering this crate, and for the same reason:
    /// none of these crashes anything, each just moves the answer. A negative window
    /// reports nothing at all. A close window wider than the window it sits inside scores
    /// every in-window pair as close, so it reports more, not fewer. And a report
    /// threshold above [`BPS_SCALE`] is past the top of the scale and could not be met by
    /// anything. [`BPS_SCALE`] itself stays usable, and reads as "only a match on the
    /// institution's own reference": scored suspects clamp one below it, and a reference
    /// match ignores the threshold entirely.
    ///
    /// # Errors
    /// [`BridgeError::OutOfRange`] naming the threshold at fault.
    pub fn to_core(&self) -> Result<dedup::MatchRules, BridgeError> {
        if self.window_seconds < 0 {
            return Err(BridgeError::OutOfRange {
                field: "match_rules.window_seconds".to_owned(),
                value: self.window_seconds.to_string(),
            });
        }
        if self.close_window_seconds < 0 || self.close_window_seconds > self.window_seconds {
            return Err(BridgeError::OutOfRange {
                field: "match_rules.close_window_seconds".to_owned(),
                value: self.close_window_seconds.to_string(),
            });
        }
        if self.report_at_or_above_bps > BPS_SCALE {
            return Err(BridgeError::OutOfRange {
                field: "match_rules.report_at_or_above_bps".to_owned(),
                value: self.report_at_or_above_bps.to_string(),
            });
        }
        Ok(dedup::MatchRules {
            window: Duration::seconds(self.window_seconds),
            close_window: Duration::seconds(self.close_window_seconds),
            report_at_or_above: self.report_at_or_above_bps,
        })
    }
}

/// Converts a whole batch, failing on the first bad row.
///
/// Batches are the normal case — a screen hands over every row for an account — and one
/// unconvertible row stops the batch rather than being dropped. A dropped row is a balance
/// that is wrong by an amount nobody can account for, which is the failure this project
/// spends most of its care avoiding.
///
/// # Errors
/// [`BridgeError`] from the first row that fails.
pub fn transactions_to_core(rows: &[TransactionDto]) -> Result<Vec<Transaction>, BridgeError> {
    batch(rows, TransactionDto::to_core)
}

/// Converts a batch of any row type, failing on the first that will not convert.
///
/// One place, so "a bad row stops the batch" is a rule rather than a habit repeated at
/// every call site.
pub(crate) fn batch<Row, Core>(
    rows: &[Row],
    convert: impl Fn(&Row) -> Result<Core, BridgeError>,
) -> Result<Vec<Core>, BridgeError> {
    rows.iter().map(convert).collect()
}
