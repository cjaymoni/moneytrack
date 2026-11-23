//! Repeating rows: the templates, and the expected rows they write ahead of time.
//!
//! A template does not make a row appear when a screen asks for one. `mt-core`'s
//! `recurrence::generate` writes real pending rows forward, which is what lets the ledger,
//! the outlook and the budgets see a salary that has not arrived yet without knowing that
//! templates exist. This module keeps the templates and does the writing.
//!
//! ## Topping up
//!
//! Rows are kept written a year ahead. Every read of the ledger calls [`top_up`] first, which
//! finds the templates that have fallen behind — fewer than [`TOP_UP_BELOW_DAYS`] ahead, so a
//! 180-day outlook always has its rows — and writes their next stretch. A template is locked
//! while it is topped up, and its rows and its new `generated_through` are written in the
//! same transaction, which is what `generate`'s idempotency rests on. Two requests racing
//! for the same template wait for each other rather than both writing; the unique index on
//! `(recurring_id, booked_at)` is the backstop behind that.
//!
//! ## Zones
//!
//! The schema stores a zone name, and the server carries no zone database. A template is
//! stored either as `Africa/Accra` or as a fixed offset spelled `UTC+01:00`, taken from the
//! browser when the template is made. For Ghana that is exact. For a zone with daylight
//! saving, a bill reckoned at a fixed offset can land on the neighbouring day for the part of
//! the year the clocks have moved; a zone database on the server is the fix when someone
//! outside West Africa needs it.

use deadpool_postgres::{GenericClient, Pool};
use mt_core::enums::{Direction, RecurrenceFreq};
use mt_core::ids::{AccountId, CategoryId, DeviceId, RecurringId, TxId, UserId};
use mt_core::model::{RecurringRule, SyncMeta};
use mt_core::money::Money;
use mt_core::recurrence;
use time::{Date, Duration, OffsetDateTime, UtcOffset};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::db;
use crate::error::ApiError;
use crate::repo::rows::Reader;
use crate::repo::{accounts, categories};

/// The table, for error messages.
const ENTITY: &str = "repeating row";

/// How far ahead a template's rows are written.
pub const AHEAD_DAYS: i64 = 366;

/// A template with fewer days than this written ahead is topped up on the next read. Above
/// the longest outlook window, 180 days, so the outlook never runs past the written rows.
pub const TOP_UP_BELOW_DAYS: i64 = 190;

/// The columns every read selects.
const COLUMNS: &str = "id, name, account_id, direction, amount_minor::bigint AS amount_minor,
     currency::text AS currency, category_id, label, counterparty, freq, interval,
     by_month_day::integer AS by_month_day, by_weekday::integer AS by_weekday,
     starts_on, ends_on, generated_through, timezone,
     created_at::timestamptz AS created_at, updated_at::timestamptz AS updated_at,
     deleted_at, device_id";

/// What a caller supplies to start a repeating row.
#[derive(Debug, Clone)]
pub struct NewRecurring {
    /// The account its rows go into.
    pub account_id: AccountId,
    /// In or out.
    pub direction: Direction,
    /// Each row's amount, in the account's currency.
    pub amount: Money,
    /// What each row is.
    pub label: String,
    /// Merchant, sender or recipient.
    pub counterparty: Option<String>,
    /// The category each row is filed under.
    pub category_id: Option<CategoryId>,
    /// Daily, weekly, monthly or yearly.
    pub freq: RecurrenceFreq,
    /// Every this many periods.
    pub interval: u16,
    /// The first occurrence. Monthly and yearly rules fall on its day of the month, weekly
    /// ones on its day of the week.
    pub starts_on: Date,
    /// The last day it may fall on, if it ends.
    pub ends_on: Option<Date>,
    /// The reader's offset, which the template is reckoned in.
    pub zone: UtcOffset,
}

/// Starts a repeating row and writes its first year of expected rows.
///
/// # Errors
/// [`ApiError::NotFound`] if the account or category is not this user's,
/// [`ApiError::Invalid`] for a template the model refuses, or [`ApiError::Internal`].
pub async fn create(
    pool: &Pool,
    user: UserId,
    device: DeviceId,
    new: &NewRecurring,
) -> Result<RecurringRule, ApiError> {
    let account = accounts::get(pool, user, new.account_id).await?;
    if let Some(category) = new.category_id {
        categories::get(pool, user, category).await?;
    }
    if new.amount.currency() != account.currency {
        return Err(ApiError::Invalid {
            field: "amount".to_owned(),
            reason: format!(
                "it is in {} and the account holds {}",
                new.amount.currency().code(),
                account.currency.code()
            ),
        });
    }
    if new.amount.is_zero() || new.amount.is_negative() {
        return Err(ApiError::Invalid {
            field: "amount".to_owned(),
            reason: "it must be more than nothing".to_owned(),
        });
    }

    // The day fields follow from the first occurrence: the form asks "starting when", and a
    // rent that starts on the 1st falls on the 1st.
    let (by_month_day, by_weekday) = match new.freq {
        RecurrenceFreq::Daily => (None, None),
        RecurrenceFreq::Weekly => (
            None,
            Some(new.starts_on.weekday().number_days_from_monday()),
        ),
        RecurrenceFreq::Monthly | RecurrenceFreq::Yearly => (Some(new.starts_on.day()), None),
    };

    let now = OffsetDateTime::now_utc();
    let rule = RecurringRule {
        id: RecurringId::from(Uuid::now_v7()),
        name: short_name(&new.label),
        account_id: new.account_id,
        direction: new.direction,
        amount: new.amount,
        category_id: new.category_id,
        label: new.label.trim().to_owned(),
        counterparty: new.counterparty.clone(),
        freq: new.freq,
        interval: new.interval,
        by_month_day,
        by_weekday,
        starts_on: new.starts_on,
        ends_on: new.ends_on,
        generated_through: None,
        timezone: zone_name(new.zone),
        sync: SyncMeta {
            created_at: now,
            updated_at: now,
            deleted_at: None,
            device_id: device,
        },
    };
    rule.validate().map_err(|reason| ApiError::Invalid {
        field: "repeating".to_owned(),
        reason: reason.to_string(),
    })?;

    let category = rule.category_id.map(|id| *id.as_uuid());
    let client = db::client(pool).await?;
    let row = client
        .query_one(
            &format!(
                "INSERT INTO recurring (
                     user_id, id, name, account_id, direction, amount_minor, currency,
                     category_id, label, counterparty, freq, interval, by_month_day, by_weekday,
                     starts_on, ends_on, timezone, created_at, updated_at, device_id
                 ) VALUES (
                     $1, $2, $3, $4, $5, $6::bigint::amount_minor, $7::text::currency_code,
                     $8, $9, $10, $11, $12, $13::integer::smallint, $14::integer::smallint,
                     $15, $16, $17,
                     $18::timestamptz::ledger_instant, $18::timestamptz::ledger_instant, $19
                 )
                 RETURNING {COLUMNS}"
            ),
            &[
                user.as_uuid(),
                rule.id.as_uuid(),
                &rule.name,
                rule.account_id.as_uuid(),
                &rule.direction.as_str(),
                &rule.amount.minor(),
                &rule.amount.currency().code(),
                &category,
                &rule.label,
                &rule.counterparty,
                &rule.freq.as_str(),
                &i32::from(rule.interval),
                &rule.by_month_day.map(i32::from),
                &rule.by_weekday.map(i32::from),
                &rule.starts_on,
                &rule.ends_on,
                &rule.timezone,
                &now,
                device.as_uuid(),
            ],
        )
        .await
        .map_err(|error| ApiError::database("insert a repeating row", &error))?;
    let written = read(&row)?;
    drop(client);

    top_up(pool, user).await?;
    get(pool, user, written.id).await
}

/// Every live template of this user's, oldest first.
///
/// # Errors
/// [`ApiError::Internal`] if the query fails or a row will not read back.
pub async fn list(pool: &Pool, user: UserId) -> Result<Vec<RecurringRule>, ApiError> {
    let client = db::client(pool).await?;
    let rows = client
        .query(
            &format!(
                "SELECT {COLUMNS} FROM recurring
                  WHERE user_id = $1 AND deleted_at IS NULL
                  ORDER BY created_at, id"
            ),
            &[user.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("list repeating rows", &error))?;
    rows.iter().map(read).collect()
}

/// One live template of this user's.
///
/// # Errors
/// [`ApiError::NotFound`] if there is no such template or it belongs to someone else.
pub async fn get(pool: &Pool, user: UserId, id: RecurringId) -> Result<RecurringRule, ApiError> {
    let client = db::client(pool).await?;
    let found = client
        .query_opt(
            &format!(
                "SELECT {COLUMNS} FROM recurring
                  WHERE user_id = $1 AND id = $2 AND deleted_at IS NULL"
            ),
            &[user.as_uuid(), id.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("read a repeating row", &error))?;
    match found {
        Some(row) => read(&row),
        None => Err(ApiError::NotFound { entity: ENTITY }),
    }
}

/// Stops a repeating row: the template goes, and so do its expected rows still to come.
///
/// What has already happened stays — confirmed rows, and expected ones dated before now that
/// nobody has confirmed yet, which are the person's to confirm or delete. Removing the future
/// ones is the point: a stopped salary that went on forecasting would be the outlook lying.
/// One transaction.
///
/// # Errors
/// [`ApiError::NotFound`] if there is no such live template of this user's.
pub async fn stop(
    pool: &Pool,
    user: UserId,
    device: DeviceId,
    id: RecurringId,
) -> Result<u64, ApiError> {
    let now = OffsetDateTime::now_utc();
    let mut client = db::client(pool).await?;
    let transaction = client
        .transaction()
        .await
        .map_err(|error| ApiError::database("begin stopping a repeating row", &error))?;
    let stopped = transaction
        .execute(
            "UPDATE recurring
                SET deleted_at = greatest($3::timestamptz, created_at::timestamptz),
                    updated_at = greatest($3::timestamptz, created_at::timestamptz)
                                 ::ledger_instant,
                    device_id = $4
              WHERE user_id = $1 AND id = $2 AND deleted_at IS NULL",
            &[user.as_uuid(), id.as_uuid(), &now, device.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("stop a repeating row", &error))?;
    if stopped == 0 {
        return Err(ApiError::NotFound { entity: ENTITY });
    }
    let removed = transaction
        .execute(
            "UPDATE transactions
                SET deleted_at = greatest($3::timestamptz, created_at::timestamptz),
                    updated_at = greatest($3::timestamptz, created_at::timestamptz)
                                 ::ledger_instant,
                    device_id = $4
              WHERE user_id = $1 AND recurring_id = $2 AND deleted_at IS NULL
                AND status = 'pending' AND booked_at > $3",
            &[user.as_uuid(), id.as_uuid(), &now, device.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("remove a stopped series' rows", &error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ApiError::database("commit stopping a repeating row", &error))?;
    Ok(removed)
}

/// Writes the next stretch of rows for every template of this user's that has fallen behind.
///
/// Cheap when there is nothing to do — one query that returns no templates — which is what
/// lets every ledger read call it.
///
/// # Errors
/// [`ApiError::Rule`] if a template will not expand, or [`ApiError::Internal`].
pub async fn top_up(pool: &Pool, user: UserId) -> Result<(), ApiError> {
    let today = OffsetDateTime::now_utc().date();
    let low_water = today + Duration::days(TOP_UP_BELOW_DAYS);
    let client = db::client(pool).await?;
    let behind = client
        .query(
            "SELECT id FROM recurring
              WHERE user_id = $1 AND deleted_at IS NULL
                AND (generated_through IS NULL OR generated_through < $2)
                AND (ends_on IS NULL OR generated_through IS NULL OR generated_through < ends_on)",
            &[user.as_uuid(), &low_water],
        )
        .await
        .map_err(|error| ApiError::database("find repeating rows to top up", &error))?;
    drop(client);

    for row in behind {
        let id: Uuid = row
            .try_get("id")
            .map_err(|error| ApiError::database("read a repeating row id", &error))?;
        top_up_one(
            pool,
            user,
            RecurringId::from(id),
            today + Duration::days(AHEAD_DAYS),
        )
        .await?;
    }
    Ok(())
}

/// Tops up one template, locked, with its rows and watermark in one transaction.
async fn top_up_one(
    pool: &Pool,
    user: UserId,
    id: RecurringId,
    through: Date,
) -> Result<(), ApiError> {
    let mut client = db::client(pool).await?;
    let transaction = client
        .transaction()
        .await
        .map_err(|error| ApiError::database("begin topping up", &error))?;
    // Re-read under the lock: another request may have topped it up while this one waited.
    let locked = transaction
        .query_opt(
            &format!(
                "SELECT {COLUMNS} FROM recurring
                  WHERE user_id = $1 AND id = $2 AND deleted_at IS NULL
                  FOR UPDATE"
            ),
            &[user.as_uuid(), id.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("lock a repeating row", &error))?;
    let Some(locked) = locked else {
        return Ok(());
    };
    let rule = read(&locked)?;
    let zone = zone_of(&rule.timezone)?;

    let generation = recurrence::generate(&rule, through, OffsetDateTime::now_utc(), zone, || {
        TxId::from(Uuid::now_v7())
    })
    .map_err(|reason| ApiError::Rule {
        rule: "recurrence",
        reason: reason.to_string(),
    })?;

    for row in &generation.rows {
        let category = row.category_id.map(|id| *id.as_uuid());
        transaction
            .execute(
                "INSERT INTO transactions (
                     user_id, id, account_id, direction, amount_minor, currency, booked_at,
                     category_id, label, counterparty, status, source, recurring_id,
                     created_at, updated_at, device_id
                 ) VALUES (
                     $1, $2, $3, $4, $5::bigint::amount_minor, $6::text::currency_code,
                     $7::timestamptz::ledger_instant, $8, $9, $10, $11, $12, $13,
                     $14::timestamptz::ledger_instant, $14::timestamptz::ledger_instant, $15
                 )
                 ON CONFLICT DO NOTHING",
                &[
                    user.as_uuid(),
                    row.id.as_uuid(),
                    row.account_id.as_uuid(),
                    &row.direction.as_str(),
                    &row.amount.minor(),
                    &row.amount.currency().code(),
                    &row.booked_at,
                    &category,
                    &row.label,
                    &row.counterparty,
                    &row.status.as_str(),
                    &row.source.as_str(),
                    rule.id.as_uuid(),
                    &row.sync.created_at,
                    row.sync.device_id.as_uuid(),
                ],
            )
            .await
            .map_err(|error| ApiError::database("write a repeating row's occurrence", &error))?;
    }
    transaction
        .execute(
            "UPDATE recurring SET generated_through = $3 WHERE user_id = $1 AND id = $2",
            &[user.as_uuid(), id.as_uuid(), &generation.generated_through],
        )
        .await
        .map_err(|error| ApiError::database("move a repeating row's watermark", &error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ApiError::database("commit topping up", &error))
}

/// The next date a template falls on, from `today`, if any.
///
/// # Errors
/// [`ApiError::Rule`] if the template will not expand.
pub fn next_on(rule: &RecurringRule, today: Date) -> Result<Option<Date>, ApiError> {
    // A year ahead holds at least one occurrence of every cadence the schema allows, short
    // of an interval of more than a year, and the cap of a thousand rows bounds a daily rule.
    let window = recurrence::occurrences(rule, today, today + Duration::days(AHEAD_DAYS))
        .or_else(|_| recurrence::occurrences(rule, today, today + Duration::days(31)))
        .map_err(|reason| ApiError::Rule {
            rule: "recurrence",
            reason: reason.to_string(),
        })?;
    Ok(window.first().copied())
}

/// The zone a template is reckoned in, from the name stored on it.
///
/// # Errors
/// [`ApiError::Internal`] for a name this server cannot resolve: one it did not write.
pub fn zone_of(name: &str) -> Result<UtcOffset, ApiError> {
    if name == "Africa/Accra" {
        return Ok(UtcOffset::UTC);
    }
    let unreadable = || ApiError::Internal {
        operation: "resolve a repeating row's zone",
        reason: format!("{name} is not a zone this server can resolve"),
    };
    let rest = name.strip_prefix("UTC").ok_or_else(unreadable)?;
    let (sign, digits) = match rest.split_at_checked(1) {
        Some(("+", digits)) => (1, digits),
        Some(("-", digits)) => (-1, digits),
        _ => return Err(unreadable()),
    };
    let (hours, minutes) = digits.split_once(':').ok_or_else(unreadable)?;
    let hours: i8 = hours.parse().map_err(|_| unreadable())?;
    let minutes: i8 = minutes.parse().map_err(|_| unreadable())?;
    UtcOffset::from_hms(sign * hours, sign * minutes, 0).map_err(|_| unreadable())
}

/// The name a template is stored with for a reader at `zone`.
fn zone_name(zone: UtcOffset) -> String {
    if zone == UtcOffset::UTC {
        return "Africa/Accra".to_owned();
    }
    let (hours, minutes, _) = zone.as_hms();
    let sign = if zone.is_negative() { '-' } else { '+' };
    format!(
        "UTC{sign}{:02}:{:02}",
        hours.unsigned_abs(),
        minutes.unsigned_abs()
    )
}

/// A template's own name, from its label, within the schema's 64 characters.
fn short_name(label: &str) -> String {
    label.trim().chars().take(64).collect()
}

/// Reads one row.
fn read(row: &Row) -> Result<RecurringRule, ApiError> {
    let reader = Reader::new(ENTITY, row)?;
    let currency = reader.currency("currency")?;
    let small = |column: &'static str| -> Result<Option<u8>, ApiError> {
        reader
            .optional_integer(column)?
            .map(|value| {
                u8::try_from(value).map_err(|_| ApiError::Internal {
                    operation: "read a row",
                    reason: format!("recurring.{column} is out of range: {value}"),
                })
            })
            .transpose()
    };
    Ok(RecurringRule {
        id: reader.id(),
        name: reader.text("name")?,
        account_id: reader.uuid("account_id")?,
        direction: reader.variant("direction")?,
        amount: reader.money("amount_minor", currency)?,
        category_id: reader.optional_uuid("category_id")?,
        label: reader.text("label")?,
        counterparty: reader.optional_text("counterparty")?,
        freq: reader.variant("freq")?,
        interval: reader.narrowed("interval")?,
        by_month_day: small("by_month_day")?,
        by_weekday: small("by_weekday")?,
        starts_on: reader.date("starts_on")?,
        ends_on: reader.optional_date("ends_on")?,
        generated_through: reader.optional_date("generated_through")?,
        timezone: reader.text("timezone")?,
        sync: SyncMeta {
            created_at: reader.instant("created_at")?,
            updated_at: reader.instant("updated_at")?,
            deleted_at: reader.optional_instant("deleted_at")?,
            device_id: reader.uuid("device_id")?,
        },
    })
}
