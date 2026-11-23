//! Budgets: setting a cap, changing it, removing it, and where each one stands today.
//!
//! Where a budget stands is `mt-core`'s `budget::evaluate`: which period, which rows count,
//! which categories roll up into which, and when it warns. These handlers read rows, ask it,
//! and render the answer twice — once for what has happened, once counting the rows still
//! expected this period — so a screen can say "you are fine now, and the rent will take you
//! over" without working either out.

use axum::Json;
use axum::extract::{Path, Query, State};
use mt_core::budget::{CategoryTree, Level, Status, evaluate};
use mt_core::enums::BudgetPeriod;
use mt_core::ids::{BudgetId, CategoryId};
use mt_core::ledger::BalanceBasis;
use mt_core::model::{Budget, Category};
use serde::{Deserialize, Serialize};
use time::{Date, Month, OffsetDateTime, UtcOffset};

use crate::AppState;
use crate::api::dto::{self, MoneyOut};
use crate::api::ledger::offset;
use crate::error::{ApiError, ApiResult};
use crate::repo::{budgets, categories, transactions, users};
use crate::session::Session;

/// The warning threshold a budget gets when none is asked for: 80% of the cap. The schema's
/// own default, restated so the two cannot drift apart silently — a test reads it back.
pub const DEFAULT_WARN_BPS: u16 = 8_000;

/// A budget and where it stands.
#[derive(Debug, Clone, Serialize)]
pub struct BudgetOut {
    /// Row id.
    pub id: String,
    /// The category capped, absent for all spending in the currency.
    pub category_id: Option<String>,
    /// The category's name, or "All spending".
    pub name: String,
    /// `weekly`, `monthly` or `yearly`.
    pub period: &'static str,
    /// The cap.
    pub cap: MoneyOut,
    /// Basis points of the cap at which it warns.
    pub warn_at_bps: u16,
    /// The first day of the first period, `YYYY-MM-DD`.
    pub starts_on: String,
    /// The last day it runs, if it ends.
    pub ends_on: Option<String>,
    /// Where it stands counting what has happened. Absent when it is not running today.
    pub now: Option<StandingOut>,
    /// Where it would stand once this period's expected rows land too.
    pub expected: Option<StandingOut>,
}

/// One evaluation of a budget, as `mt-core` worked it out.
#[derive(Debug, Clone, Serialize)]
pub struct StandingOut {
    /// The period counted, first day.
    pub period_starts_on: String,
    /// The period counted, last day inclusive.
    pub period_ends_on: String,
    /// Spent against the cap.
    pub spent: MoneyOut,
    /// Cap less spending; negative once over.
    pub remaining: MoneyOut,
    /// How far past the cap, as a positive amount, once over. Sent rather than left to a
    /// screen to negate `remaining`.
    pub over_by: Option<MoneyOut>,
    /// Spending in basis points of the cap. Past 10 000 once over; capped at 65 535.
    pub used_bps: u16,
    /// The same as a whole percentage, for a person: "123%".
    pub used_display: String,
    /// `under`, `warning` or `over`.
    pub level: &'static str,
}

/// On whose clock "today" is.
#[derive(Debug, Deserialize)]
pub struct Clock {
    /// The reader's offset from UTC in minutes, east positive. Absent means UTC.
    pub offset_minutes: Option<i32>,
}

/// Every budget, with where it stands today.
///
/// # Errors
/// [`ApiError::Invalid`] for an offset out of range, or [`ApiError::Rule`] if a budget will
/// not evaluate.
pub async fn list(
    State(state): State<AppState>,
    session: Session,
    Query(clock): Query<Clock>,
) -> ApiResult<Json<Vec<BudgetOut>>> {
    let zone = offset(clock.offset_minutes.unwrap_or(0))?;
    let today = today_in(zone);
    let held = budgets::list(&state.pool, session.user).await?;
    let filed = categories::list(&state.pool, session.user).await?;
    let rows = transactions::all(&state.pool, session.user).await?;
    let tree = CategoryTree::build(&filed);

    held.iter()
        .map(|budget| {
            let standing = |basis| {
                evaluate(budget, &tree, &rows, today, basis, zone).map_err(|reason| {
                    ApiError::Rule {
                        rule: "budget",
                        reason: reason.to_string(),
                    }
                })
            };
            let now = standing(BalanceBasis::Confirmed)?;
            let expected = standing(BalanceBasis::Projected)?;
            Ok(render(budget, &filed, now.as_ref(), expected.as_ref()))
        })
        .collect::<ApiResult<Vec<_>>>()
        .map(Json)
}

/// What a browser sends to set a budget.
#[derive(Debug, Deserialize)]
pub struct NewBudgetIn {
    /// The category to cap; absent caps all spending in the currency.
    pub category_id: Option<String>,
    /// `weekly`, `monthly` or `yearly`.
    pub period: String,
    /// The cap, as the decimal text a person typed.
    pub cap: String,
    /// ISO 4217 code.
    pub currency: String,
    /// Basis points of the cap at which to warn. Absent means 80%.
    pub warn_at_bps: Option<u16>,
    /// The reader's offset, which decides which period "now" is.
    pub offset_minutes: Option<i32>,
}

/// Sets a budget, starting with the period today falls in.
///
/// A monthly budget starts on the first of this month and a yearly one on the first of
/// January, so spending earlier in the current period counts: someone setting a food budget
/// on the 20th wants to know about the 19 days already gone, not to start from nothing. A
/// weekly budget starts today, which is where its weeks are counted from.
///
/// # Errors
/// [`ApiError::Invalid`], [`ApiError::NotFound`] for a category not this user's, or
/// [`ApiError::Conflict`] if one already covers that scope.
pub async fn create(
    State(state): State<AppState>,
    session: Session,
    Json(body): Json<NewBudgetIn>,
) -> ApiResult<Json<BudgetOut>> {
    let zone = offset(body.offset_minutes.unwrap_or(0))?;
    let currency = dto::currency("currency", &body.currency)?;
    let period = dto::variant::<BudgetPeriod>("period", &body.period)?;
    let cap =
        dto::amount("cap", None, Some(&body.cap), currency)?.ok_or_else(|| ApiError::Invalid {
            field: "cap".to_owned(),
            reason: "it is missing".to_owned(),
        })?;
    let new = budgets::NewBudget {
        category_id: body
            .category_id
            .as_deref()
            .map(|id| dto::id::<CategoryId>("category_id", id))
            .transpose()?,
        period,
        cap,
        starts_on: first_period_start(period, today_in(zone))?,
        warn_at_bps: warn(body.warn_at_bps)?,
    };
    let device = users::web_device(&state.pool, session.user).await?;
    let budget = budgets::create(&state.pool, session.user, device, &new).await?;
    respond(&state, session, &budget, zone).await
}

/// What a browser sends to change a budget.
#[derive(Debug, Deserialize)]
pub struct ChangeBudgetIn {
    /// The new cap, as decimal text.
    pub cap: String,
    /// Basis points of the cap at which to warn. Absent keeps the current threshold.
    pub warn_at_bps: Option<u16>,
    /// The reader's offset, which decides which period is current.
    pub offset_minutes: Option<i32>,
}

/// Changes a budget's cap from the current period on. Past periods keep theirs.
///
/// # Errors
/// [`ApiError::NotFound`] if the budget is not this user's, or [`ApiError::Invalid`].
pub async fn change(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<String>,
    Json(body): Json<ChangeBudgetIn>,
) -> ApiResult<Json<BudgetOut>> {
    let zone = offset(body.offset_minutes.unwrap_or(0))?;
    let id: BudgetId = dto::id("id", &id)?;
    let existing = budgets::get(&state.pool, session.user, id).await?;
    let cap =
        dto::amount("cap", None, Some(&body.cap), existing.cap.currency())?.ok_or_else(|| {
            ApiError::Invalid {
                field: "cap".to_owned(),
                reason: "it is missing".to_owned(),
            }
        })?;
    let warn_at = match body.warn_at_bps {
        Some(bps) => warn(Some(bps))?,
        None => existing.warn_at_bps,
    };
    let device = users::web_device(&state.pool, session.user).await?;
    let budget = budgets::change(
        &state.pool,
        session.user,
        device,
        id,
        cap,
        warn_at,
        today_in(zone),
    )
    .await?;
    respond(&state, session, &budget, zone).await
}

/// Removes a budget.
///
/// # Errors
/// [`ApiError::NotFound`] if there is no such budget of this user's.
pub async fn delete(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let id: BudgetId = dto::id("id", &id)?;
    let device = users::web_device(&state.pool, session.user).await?;
    budgets::delete(&state.pool, session.user, device, id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// Evaluates one budget and renders it, for the answer to a write.
async fn respond(
    state: &AppState,
    session: Session,
    budget: &Budget,
    zone: UtcOffset,
) -> ApiResult<Json<BudgetOut>> {
    let filed = categories::list(&state.pool, session.user).await?;
    let rows = transactions::all(&state.pool, session.user).await?;
    let tree = CategoryTree::build(&filed);
    let today = today_in(zone);
    let standing = |basis| {
        evaluate(budget, &tree, &rows, today, basis, zone).map_err(|reason| ApiError::Rule {
            rule: "budget",
            reason: reason.to_string(),
        })
    };
    let now = standing(BalanceBasis::Confirmed)?;
    let expected = standing(BalanceBasis::Projected)?;
    Ok(Json(render(
        budget,
        &filed,
        now.as_ref(),
        expected.as_ref(),
    )))
}

/// The reader's date today.
fn today_in(zone: UtcOffset) -> Date {
    OffsetDateTime::now_utc().to_offset(zone).date()
}

/// Where a new budget's first period begins, given today.
fn first_period_start(period: BudgetPeriod, today: Date) -> ApiResult<Date> {
    let start = match period {
        BudgetPeriod::Weekly => Ok(today),
        BudgetPeriod::Monthly => Date::from_calendar_date(today.year(), today.month(), 1),
        BudgetPeriod::Yearly => Date::from_calendar_date(today.year(), Month::January, 1),
    };
    start.map_err(|error| ApiError::Internal {
        operation: "start a budget",
        reason: error.to_string(),
    })
}

/// A warning threshold: within the cap, or zero for no warning.
fn warn(asked: Option<u16>) -> ApiResult<u16> {
    match asked {
        None => Ok(DEFAULT_WARN_BPS),
        Some(bps) if bps <= 10_000 => Ok(bps),
        Some(_) => Err(ApiError::Invalid {
            field: "warn_at_bps".to_owned(),
            reason: "expected basis points of the cap, 0 to 10 000".to_owned(),
        }),
    }
}

fn render(
    budget: &Budget,
    filed: &[Category],
    now: Option<&Status>,
    expected: Option<&Status>,
) -> BudgetOut {
    let name = match budget.category_id {
        None => "All spending".to_owned(),
        Some(id) => filed.iter().find(|category| category.id == id).map_or_else(
            || "A removed category".to_owned(),
            |category| category.name.clone(),
        ),
    };
    BudgetOut {
        id: budget.id.to_string(),
        category_id: budget.category_id.map(|id| id.to_string()),
        name,
        period: budget.period.as_str(),
        cap: budget.cap.into(),
        warn_at_bps: budget.warn_at_bps,
        starts_on: date(budget.starts_on),
        ends_on: budget.ends_on.map(date),
        now: now.map(standing),
        expected: expected.map(standing),
    }
}

fn standing(status: &Status) -> StandingOut {
    StandingOut {
        period_starts_on: date(status.period.starts_on),
        period_ends_on: date(status.period.ends_on),
        spent: status.spent.into(),
        remaining: status.remaining.into(),
        over_by: if status.remaining.is_negative() {
            status.remaining.try_neg().ok().map(MoneyOut::from)
        } else {
            None
        },
        used_bps: status.used_bps,
        used_display: if status.used_bps == u16::MAX {
            "far past".to_owned()
        } else {
            format!("{}%", status.used_bps / 100)
        },
        level: match status.level {
            Level::Under => "under",
            Level::Warning => "warning",
            Level::Over => "over",
        },
    }
}

fn date(value: Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        value.year(),
        u8::from(value.month()),
        value.day()
    )
}
