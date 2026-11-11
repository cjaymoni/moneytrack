//! Row builders shared by the integration tests.
//!
//! Each one returns a valid row, so a test can change the single field it is about and the
//! assertion says what that field does rather than what twelve unrelated fields are.

#![allow(dead_code, clippy::unwrap_used)]

use mt_core::enums::{
    AccountKind, BudgetPeriod, CategoryKind, Direction, InboxSource, InboxStatus, MatchedField,
    RecurrenceFreq, RuleOrigin, TxSource, TxStatus,
};
use mt_core::ids::{
    AccountId, AttachmentId, BudgetId, CategoryId, CategoryRuleId, DeviceId, InboxItemId,
    RecurringId, TxId,
};
use mt_core::model::{
    Account, Attachment, Budget, Candidate, Category, CategoryRule, InboxItem, RecurringRule,
    SyncMeta, Transaction,
};
use mt_core::money::{Currency, Money};
use mt_core::zone::Zone;
use time::macros::{date, datetime};
use time::{Date, Duration, OffsetDateTime, Time, UtcOffset};
use uuid::Uuid;

pub const GHS: Currency = Currency::GHS;
pub const USD: Currency = Currency::USD;
pub const NOW: OffsetDateTime = datetime!(2026-11-10 09:00:00 UTC);

/// A fixed device id, so two rows built here look like they came from the same handset.
pub fn device() -> DeviceId {
    DeviceId::from_uuid(Uuid::from_u128(0x0d00_0000_0000_7000_8000_0000_0000_0001))
}

pub fn id<T: From<Uuid>>() -> T {
    T::from(Uuid::now_v7())
}

pub fn sync() -> SyncMeta {
    SyncMeta {
        created_at: NOW,
        updated_at: NOW,
        deleted_at: None,
        device_id: device(),
    }
}

pub fn account() -> Account {
    Account {
        id: id(),
        name: "MTN MoMo".to_owned(),
        kind: AccountKind::Momo,
        currency: GHS,
        opening_balance: Money::new(0, GHS),
        archived_at: None,
        sort_order: 0,
        sync: sync(),
    }
}

pub fn transaction() -> Transaction {
    Transaction {
        id: id(),
        account_id: id(),
        direction: Direction::Out,
        amount: Money::new(2000, GHS),
        booked_at: NOW,
        category_id: None,
        label: "Trotro".to_owned(),
        counterparty: None,
        note: None,
        status: TxStatus::Confirmed,
        source: TxSource::Manual,
        source_ref: None,
        external_id: None,
        recurring_id: None,
        transfer_id: None,
        sync: sync(),
    }
}

pub fn category() -> Category {
    Category {
        id: id(),
        name: "Transport".to_owned(),
        parent_id: None,
        kind: CategoryKind::Expense,
        icon: None,
        is_system: false,
        sort_order: 0,
        sync: sync(),
    }
}

pub fn category_rule(category_id: CategoryId) -> CategoryRule {
    CategoryRule {
        id: id(),
        pattern: "TROTRO".to_owned(),
        matched_field: MatchedField::Label,
        category_id,
        priority: 100,
        is_regex: false,
        enabled: true,
        origin: RuleOrigin::User,
        pack_id: None,
        sync: sync(),
    }
}

pub fn recurring() -> RecurringRule {
    RecurringRule {
        id: id(),
        name: "Rent".to_owned(),
        account_id: id(),
        direction: Direction::Out,
        amount: Money::new(150_000, GHS),
        category_id: None,
        label: "Rent".to_owned(),
        counterparty: None,
        freq: RecurrenceFreq::Monthly,
        interval: 1,
        by_month_day: Some(1),
        by_weekday: None,
        starts_on: date!(2026 - 12 - 01),
        ends_on: None,
        generated_through: None,
        timezone: "Africa/Accra".to_owned(),
        sync: sync(),
    }
}

pub fn budget() -> Budget {
    Budget {
        id: id(),
        category_id: None,
        period: BudgetPeriod::Monthly,
        cap: Money::new(50_000, GHS),
        starts_on: date!(2026 - 01 - 01),
        ends_on: Some(date!(2026 - 12 - 31)),
        warn_at_bps: 8_000,
        sync: sync(),
    }
}

pub fn candidate() -> Candidate {
    Candidate {
        account_id: None,
        direction: Direction::Out,
        amount: Money::new(2000, GHS),
        booked_at: NOW,
        label: "Payment to SHOPRITE".to_owned(),
        counterparty: Some("SHOPRITE".to_owned()),
        category_id: None,
        category_confidence_bps: 0,
        external_id: None,
        reported_balance: None,
    }
}

pub fn inbox_item() -> InboxItem {
    InboxItem {
        id: id(),
        source: InboxSource::Sms,
        received_at: NOW,
        origin_ref: Some("MTNMobileMoney".to_owned()),
        fingerprint: "9f86d081884c7d65".to_owned(),
        candidate: candidate(),
        status: InboxStatus::Pending,
        transaction_id: None,
        duplicate_of: None,
        sync: sync(),
    }
}

pub fn attachment(transaction_id: TxId) -> Attachment {
    Attachment {
        id: id(),
        transaction_id,
        relative_path: "receipts/2026/11/a.jpg".to_owned(),
        media_type: "image/jpeg".to_owned(),
        byte_size: 1024,
        sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned(),
        sync: sync(),
    }
}

/// The two legs of a well-formed transfer between two accounts.
pub fn transfer_pair() -> [Transaction; 2] {
    let transfer_id = id();
    let out = Transaction {
        direction: Direction::Out,
        transfer_id: Some(transfer_id),
        label: "To GCB".to_owned(),
        ..transaction()
    };
    let into = Transaction {
        id: id::<TxId>(),
        account_id: id::<AccountId>(),
        direction: Direction::In,
        transfer_id: Some(transfer_id),
        label: "From MoMo".to_owned(),
        ..out.clone()
    };
    [out, into]
}

/// An instant `days` after [`NOW`], for rows that need to be ordered against each other.
pub fn days_after(days: i64) -> OffsetDateTime {
    NOW + Duration::days(days)
}

/// A transaction in a named account, for the walk tests where the account matters.
pub fn tx_in(
    account_id: AccountId,
    direction: Direction,
    minor: i64,
    booked_at: OffsetDateTime,
) -> Transaction {
    Transaction {
        account_id,
        direction,
        amount: Money::new(minor, GHS),
        booked_at,
        ..transaction()
    }
}

/// Accra, which is UTC+0 the whole year round. A fixed offset is itself a `Zone`.
pub const ACCRA: UtcOffset = UtcOffset::UTC;

/// A zone eleven hours behind, so that a local date and the UTC date disagree for most of
/// the day. Tests that pass [`ACCRA`] alone prove nothing about the zone handling, because
/// there it is the identity function.
pub const FAR_WEST: UtcOffset = match UtcOffset::from_hms(-11, 0, 0) {
    Ok(offset) => offset,
    Err(_) => UtcOffset::UTC,
};

/// A zone fourteen hours ahead, the far end of the real range.
pub const FAR_EAST: UtcOffset = match UtcOffset::from_hms(14, 0, 0) {
    Ok(offset) => offset,
    Err(_) => UtcOffset::UTC,
};

/// A zone that shifts its offset on a given local date, the way a real one does in spring
/// and autumn — and, for several real zones, on the first of a month.
///
/// This is the case where asking "what offset applies on this date" and "what offset applied
/// at this instant" give different answers, which is why [`Zone`] has two methods.
#[derive(Debug, Clone, Copy)]
pub struct Shifting {
    /// In force up to, but not including, `shifts_on`.
    pub before: UtcOffset,
    /// In force from `shifts_on` onwards.
    pub after: UtcOffset,
    /// The first local date on the new offset.
    pub shifts_on: Date,
}

impl Zone for Shifting {
    fn offset_for_instant(&self, at: OffsetDateTime) -> UtcOffset {
        // Resolved against the instant the shift happens, which is local midnight on
        // `shifts_on` reckoned in the old offset.
        let boundary = self
            .shifts_on
            .with_time(Time::MIDNIGHT)
            .assume_offset(self.before);
        if at < boundary {
            self.before
        } else {
            self.after
        }
    }

    fn offset_for_date(&self, date: Date) -> UtcOffset {
        if date < self.shifts_on {
            self.before
        } else {
            self.after
        }
    }
}

/// Local midnight on a date in Accra.
pub fn midnight(day: Date) -> OffsetDateTime {
    ACCRA.start_of_day(day)
}

/// Silences the unused-import warnings in test binaries that use only part of this module.
pub fn _type_anchors() -> (
    AttachmentId,
    BudgetId,
    CategoryRuleId,
    InboxItemId,
    RecurringId,
    TxId,
    AccountId,
    CategoryId,
) {
    (id(), id(), id(), id(), id(), id(), id(), id())
}
