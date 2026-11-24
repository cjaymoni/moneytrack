//! Repeating rows: starting one, listing them, stopping one.
//!
//! What a series writes is `mt-core`'s `recurrence::generate`; these handlers keep the
//! templates and say when each falls next.

use axum::Json;
use axum::extract::{Path, State};
use mt_core::enums::{Direction, RecurrenceFreq};
use mt_core::ids::{AccountId, CategoryId, RecurringId};
use mt_core::model::RecurringRule;
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};

use crate::AppState;
use crate::api::dto::{self, MoneyOut};
use crate::api::ledger::offset;
use crate::error::{ApiError, ApiResult};
use crate::repo::{accounts, recurring, users};
use crate::session::Session;

/// A repeating row, and when it falls next.
#[derive(Debug, Clone, Serialize)]
pub struct RecurringOut {
    /// Row id.
    pub id: String,
    /// The account its rows go into.
    pub account_id: String,
    /// That account's name.
    pub account_name: String,
    /// `in` or `out`.
    pub direction: &'static str,
    /// Each row's amount.
    pub amount: MoneyOut,
    /// What each row is.
    pub label: String,
    /// Merchant, sender or recipient.
    pub counterparty: Option<String>,
    /// The category each row is filed under.
    pub category_id: Option<String>,
    /// `daily`, `weekly`, `monthly` or `yearly`.
    pub freq: &'static str,
    /// Every this many periods.
    pub interval: u16,
    /// The first occurrence, `YYYY-MM-DD`.
    pub starts_on: String,
    /// The last day it may fall on, if it ends.
    pub ends_on: Option<String>,
    /// The next day it falls on, from today; absent once the series has ended.
    pub next_on: Option<String>,
}

/// What a browser sends to start a repeating row.
#[derive(Debug, Deserialize)]
pub struct NewRecurringIn {
    /// The account.
    pub account_id: String,
    /// `in` or `out`.
    pub direction: String,
    /// Each row's amount, as typed.
    pub amount: String,
    /// What each row is.
    pub label: String,
    /// Merchant, sender or recipient.
    pub counterparty: Option<String>,
    /// The category.
    pub category_id: Option<String>,
    /// `daily`, `weekly`, `monthly` or `yearly`.
    pub freq: String,
    /// Every this many periods. Absent means 1.
    pub interval: Option<u16>,
    /// The first occurrence, `YYYY-MM-DD`.
    pub starts_on: String,
    /// The last day it may fall on, `YYYY-MM-DD`.
    pub ends_on: Option<String>,
    /// The reader's offset, which the series is reckoned in.
    pub offset_minutes: Option<i32>,
}

/// How far back a series may start. A start this far back writes a year of overdue expected
/// rows for a daily series, which is the most anyone should be asked to confirm; further
/// back is history, and history is recorded, not repeated.
const EARLIEST_START_DAYS: i64 = 366;

/// Every repeating row.
///
/// # Errors
/// [`ApiError::Rule`] if a template will not expand, or [`ApiError::Internal`].
pub async fn list(
    State(state): State<AppState>,
    session: Session,
) -> ApiResult<Json<Vec<RecurringOut>>> {
    recurring::top_up(&state.pool, session.user).await?;
    let rules = recurring::list(&state.pool, session.user).await?;
    let held = accounts::list(&state.pool, session.user).await?;
    let today = OffsetDateTime::now_utc().date();
    rules
        .iter()
        .map(|rule| {
            let account_name = held
                .iter()
                .find(|account| account.id == rule.account_id)
                .map_or_else(String::new, |account| account.name.clone());
            render(rule, account_name, today)
        })
        .collect::<ApiResult<Vec<_>>>()
        .map(Json)
}

/// Starts a repeating row, and writes its first year of expected rows.
///
/// # Errors
/// [`ApiError::Invalid`], or [`ApiError::NotFound`] for an account or category not this
/// user's.
pub async fn create(
    State(state): State<AppState>,
    session: Session,
    Json(body): Json<NewRecurringIn>,
) -> ApiResult<Json<RecurringOut>> {
    let zone = offset(body.offset_minutes.unwrap_or(0))?;
    let account_id: AccountId = dto::id("account_id", &body.account_id)?;
    let account = accounts::get(&state.pool, session.user, account_id).await?;
    let starts_on = date("starts_on", &body.starts_on)?;
    let today = OffsetDateTime::now_utc().to_offset(zone).date();
    if starts_on < today - time::Duration::days(EARLIEST_START_DAYS) {
        return Err(ApiError::Invalid {
            field: "starts_on".to_owned(),
            reason: "it is more than a year ago; add past payments one at a time instead"
                .to_owned(),
        });
    }
    let interval = body.interval.unwrap_or(1);
    if interval == 0 || interval > 99 {
        return Err(ApiError::Invalid {
            field: "interval".to_owned(),
            reason: "expected every 1 to 99 periods".to_owned(),
        });
    }

    let new = recurring::NewRecurring {
        account_id,
        direction: dto::variant::<Direction>("direction", &body.direction)?,
        amount: dto::amount("amount", None, Some(&body.amount), account.currency)?.ok_or_else(
            || ApiError::Invalid {
                field: "amount".to_owned(),
                reason: "it is missing".to_owned(),
            },
        )?,
        label: body.label,
        counterparty: body
            .counterparty
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty()),
        category_id: body
            .category_id
            .as_deref()
            .map(|id| dto::id::<CategoryId>("category_id", id))
            .transpose()?,
        freq: dto::variant::<RecurrenceFreq>("freq", &body.freq)?,
        interval,
        starts_on,
        ends_on: body
            .ends_on
            .as_deref()
            .map(|text| date("ends_on", text))
            .transpose()?,
        zone,
    };
    let device = users::web_device(&state.pool, session.user).await?;
    let rule = recurring::create(&state.pool, session.user, device, &new).await?;
    Ok(Json(render(&rule, account.name, today)?))
}

/// Stops a repeating row. Its expected rows still to come go with it; what has happened
/// stays.
///
/// # Errors
/// [`ApiError::NotFound`] if there is no such repeating row of this user's.
pub async fn stop(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let id: RecurringId = dto::id("id", &id)?;
    let device = users::web_device(&state.pool, session.user).await?;
    let removed = recurring::stop(&state.pool, session.user, device, id).await?;
    Ok(Json(
        serde_json::json!({ "stopped": true, "removed_rows": removed }),
    ))
}

fn render(rule: &RecurringRule, account_name: String, today: Date) -> ApiResult<RecurringOut> {
    Ok(RecurringOut {
        id: rule.id.to_string(),
        account_id: rule.account_id.to_string(),
        account_name,
        direction: rule.direction.as_str(),
        amount: rule.amount.into(),
        label: rule.label.clone(),
        counterparty: rule.counterparty.clone(),
        category_id: rule.category_id.map(|id| id.to_string()),
        freq: rule.freq.as_str(),
        interval: rule.interval,
        starts_on: spell(rule.starts_on),
        ends_on: rule.ends_on.map(spell),
        next_on: recurring::next_on(rule, today)?.map(spell),
    })
}

/// Reads a `YYYY-MM-DD` date.
fn date(field: &str, text: &str) -> ApiResult<Date> {
    let format = time::macros::format_description!("[year]-[month]-[day]");
    Date::parse(text, &format).map_err(|_| ApiError::Invalid {
        field: field.to_owned(),
        reason: "expected a date as YYYY-MM-DD".to_owned(),
    })
}

fn spell(value: Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        value.year(),
        u8::from(value.month()),
        value.day()
    )
}
