//! DTO builders shared by the bridge's tests.
//!
//! Each returns a row that converts and validates, so a test can change the single field it
//! is about. Everything is in the wire spellings a Dart caller would supply: ids as text,
//! amounts as minor units, instants as RFC 3339.

#![allow(dead_code, clippy::unwrap_used)]

use mt_bridge::dto::{
    AccountDto, BudgetDto, CandidateDto, CategoryDto, CategoryRuleDto, RecurringRuleDto,
    SyncMetaDto, TransactionDto,
};
use mt_bridge::enums::{
    AccountKind, BudgetPeriod, CategoryKind, Direction, MatchedField, RecurrenceFreq, RuleOrigin,
    TxSource, TxStatus,
};
use mt_bridge::scalar::MoneyDto;
use mt_bridge::zone::{ZoneDto, ZoneTransitionDto};

/// Ghana's offset: UTC, and it has never shifted.
pub const ACCRA_OFFSET_SECONDS: i32 = 0;
/// A fixed instant every builder hangs off, so expected values can be read off by hand.
pub const NOW: &str = "2026-11-10T09:00:00Z";
/// The local date [`NOW`] falls on in a zone at or near UTC.
pub const TODAY: &str = "2026-11-10";

/// A distinct, well-formed id. Deterministic per `n`, so a failing assertion names a row a
/// reader can find in the test above it.
pub fn uuid(n: u128) -> String {
    uuid::Uuid::from_u128(0x0000_0000_0000_7000_8000_0000_0000_0000 | n).to_string()
}

pub fn sync() -> SyncMetaDto {
    SyncMetaDto {
        created_at: NOW.to_owned(),
        updated_at: NOW.to_owned(),
        deleted_at: None,
        device_id: uuid(0xd1),
    }
}

/// A cedi wallet with a known opening balance.
pub fn account(id: u128, opening_minor: i64) -> AccountDto {
    AccountDto {
        id: uuid(id),
        name: "Wallet".to_owned(),
        kind: AccountKind::Momo,
        currency: "GHS".to_owned(),
        opening_balance: money(opening_minor),
        archived_at: None,
        sort_order: 0,
        sync: sync(),
    }
}

pub fn money(minor_units: i64) -> MoneyDto {
    MoneyDto {
        minor_units,
        currency: "GHS".to_owned(),
    }
}

/// A confirmed row in `account`, booked at `booked_at`.
pub fn transaction(
    id: u128,
    account: &AccountDto,
    direction: Direction,
    minor: i64,
    booked_at: &str,
) -> TransactionDto {
    TransactionDto {
        id: uuid(id),
        account_id: account.id.clone(),
        direction,
        amount: money(minor),
        booked_at: booked_at.to_owned(),
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

pub fn category(id: u128, name: &str) -> CategoryDto {
    CategoryDto {
        id: uuid(id),
        name: name.to_owned(),
        parent_id: None,
        kind: CategoryKind::Expense,
        icon: None,
        is_system: false,
        sort_order: 0,
        sync: sync(),
    }
}

pub fn rule(id: u128, pattern: &str, category: &CategoryDto) -> CategoryRuleDto {
    CategoryRuleDto {
        id: uuid(id),
        pattern: pattern.to_owned(),
        matched_field: MatchedField::Counterparty,
        category_id: category.id.clone(),
        priority: 100,
        is_regex: false,
        enabled: true,
        origin: RuleOrigin::User,
        pack_id: None,
        sync: sync(),
    }
}

pub fn candidate(minor: i64, booked_at: &str) -> CandidateDto {
    CandidateDto {
        account_id: None,
        direction: Direction::Out,
        amount: money(minor),
        booked_at: booked_at.to_owned(),
        label: "Payment".to_owned(),
        counterparty: Some("Melcom".to_owned()),
        category_id: None,
        category_confidence_bps: 0,
        external_id: None,
        reported_balance: None,
    }
}

/// A monthly budget starting today.
pub fn budget(id: u128, cap_minor: i64, category: Option<&CategoryDto>) -> BudgetDto {
    BudgetDto {
        id: uuid(id),
        category_id: category.map(|category| category.id.clone()),
        period: BudgetPeriod::Monthly,
        cap: money(cap_minor),
        starts_on: "2026-11-01".to_owned(),
        ends_on: None,
        warn_at_bps: 8_000,
        sync: sync(),
    }
}

/// A monthly template firing on the 15th.
pub fn recurring(id: u128, account: &AccountDto, minor: i64) -> RecurringRuleDto {
    RecurringRuleDto {
        id: uuid(id),
        name: "Rent".to_owned(),
        account_id: account.id.clone(),
        direction: Direction::Out,
        amount: money(minor),
        category_id: None,
        label: "Rent".to_owned(),
        counterparty: None,
        freq: RecurrenceFreq::Monthly,
        interval: 1,
        by_month_day: Some(15),
        by_weekday: None,
        starts_on: "2026-11-15".to_owned(),
        ends_on: None,
        generated_through: None,
        timezone: "Africa/Accra".to_owned(),
        sync: sync(),
    }
}

/// Accra: fixed at UTC, no transitions.
pub fn accra() -> ZoneDto {
    ZoneDto::fixed(ACCRA_OFFSET_SECONDS)
}

/// A zone that shifts, for the cases Accra cannot exercise: London in 2026, where the
/// clocks go forward on 29 March and back on 25 October.
///
/// Built for 2026 and says so. The claim runs a little past the year at each end, because
/// a question about a local date is answered from instants either side of its UTC midnight
/// and a table has to cover those too; London's next shift after October 2026 is in late
/// March 2027, so the list really is complete that far. A question about mid-2027 is
/// refused rather than answered from the base offset.
pub fn london() -> ZoneDto {
    ZoneDto {
        base_offset_seconds: 0,
        covers_from: Some("2025-12-30T00:00:00Z".to_owned()),
        covers_through: Some("2027-01-02T00:00:00Z".to_owned()),
        transitions: vec![
            ZoneTransitionDto {
                at: "2026-03-29T01:00:00Z".to_owned(),
                offset_seconds: 3_600,
            },
            ZoneTransitionDto {
                at: "2026-10-25T01:00:00Z".to_owned(),
                offset_seconds: 0,
            },
        ],
    }
}
