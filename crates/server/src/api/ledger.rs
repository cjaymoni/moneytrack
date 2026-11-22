//! Accounts, transactions, and what they add up to.
//!
//! Every number here comes from `mt-core`. The handlers read rows, hand them to the same
//! walk a handset runs locally, and render the answer — they do not add anything up
//! themselves, and there is nothing in this file that decides what counts as spending.

use axum::Json;
use axum::extract::{Path, Query, State};
use mt_core::enums::{AccountKind, CategoryKind, Direction, TxSource, TxStatus};
use mt_core::ids::{AccountId, CategoryId, TxId};
use mt_core::ledger::{self, BalanceBasis};
use mt_core::money::Money;
use serde::Deserialize;
use time::OffsetDateTime;

use crate::AppState;
use crate::api::dto::EditTransactionIn;
use crate::api::dto::{
    self, AccountOut, CategoryOut, CurrencyOut, LedgerOut, MoneyOut, NewAccountIn, NewCategoryIn,
    NewTransactionIn, SetCategoryIn, TransactionOut, account_out, ledger_out,
};
use crate::error::{ApiError, ApiResult};
use crate::repo::{accounts, categories, transactions, users};
use crate::session::Session;

/// Every account, with what it holds on both bases.
///
/// One walk over every row rather than one per account, so the screen does not get slower in
/// proportion to the square of how long the person has used it.
///
/// # Errors
/// [`ApiError::Rule`] if the rows will not walk, or [`ApiError::Internal`].
pub async fn list_accounts(
    State(state): State<AppState>,
    session: Session,
) -> ApiResult<Json<Vec<AccountOut>>> {
    let accounts = accounts::list(&state.pool, session.user).await?;
    let rows = transactions::all(&state.pool, session.user).await?;

    let confirmed = balances(&accounts, &rows, BalanceBasis::Confirmed)?;
    let projected = balances(&accounts, &rows, BalanceBasis::Projected)?;

    accounts
        .iter()
        .map(|account| {
            let held = |on: &std::collections::BTreeMap<AccountId, Money>| {
                on.get(&account.id)
                    .copied()
                    .unwrap_or_else(|| Money::new(0, account.currency))
            };
            Ok(account_out(account, held(&confirmed), held(&projected)))
        })
        .collect::<ApiResult<Vec<_>>>()
        .map(Json)
}

/// Opens an account.
///
/// # Errors
/// [`ApiError::Invalid`] for a value that will not convert or a row the model refuses.
pub async fn create_account(
    State(state): State<AppState>,
    session: Session,
    Json(body): Json<NewAccountIn>,
) -> ApiResult<Json<AccountOut>> {
    let currency = dto::currency("currency", &body.currency)?;
    let new = accounts::NewAccount {
        name: body.name,
        kind: dto::variant::<AccountKind>("kind", &body.kind)?,
        currency,
        opening_balance: dto::amount(
            "opening_balance",
            body.opening_balance_minor,
            body.opening_balance.as_deref(),
            currency,
        )?
        .unwrap_or_else(|| Money::zero(currency)),
    };
    let device = users::web_device(&state.pool, session.user).await?;
    let account = accounts::create(&state.pool, session.user, device, &new).await?;

    // A new account holds its opening balance on both bases: there are no rows yet.
    Ok(Json(account_out(
        &account,
        account.opening_balance,
        account.opening_balance,
    )))
}

/// Which account a listing is about.
#[derive(Debug, Deserialize)]
pub struct Scope {
    /// The account, or absent for every account.
    pub account_id: Option<String>,
}

/// The rows, oldest first.
///
/// # Errors
/// [`ApiError::NotFound`] if the account named is not this user's.
pub async fn list_transactions(
    State(state): State<AppState>,
    session: Session,
    Query(scope): Query<Scope>,
) -> ApiResult<Json<Vec<TransactionOut>>> {
    let rows = match scope.account_id {
        Some(id) => {
            let account: AccountId = dto::id("account_id", &id)?;
            transactions::for_account(&state.pool, session.user, account).await?
        }
        None => transactions::all(&state.pool, session.user).await?,
    };
    rows.iter()
        .map(TransactionOut::try_from)
        .collect::<ApiResult<Vec<_>>>()
        .map(Json)
}

/// Records a transaction.
///
/// # Errors
/// [`ApiError::NotFound`] if the account is not this user's, or [`ApiError::Invalid`] for a
/// value that will not convert, a currency that does not match, or a row the model refuses.
pub async fn create_transaction(
    State(state): State<AppState>,
    session: Session,
    Json(body): Json<NewTransactionIn>,
) -> ApiResult<Json<TransactionOut>> {
    let account_id: AccountId = dto::id("account_id", &body.account_id)?;
    // The amount's currency is the account's, read from the account rather than taken from
    // the request: a caller cannot name a currency its account does not hold.
    let account = accounts::get(&state.pool, session.user, account_id).await?;

    let booked_at = match body.booked_at.as_deref() {
        Some(text) => dto::parse_instant("booked_at", text)?,
        None => OffsetDateTime::now_utc(),
    };
    let status = match body.status.as_deref() {
        Some(text) => dto::variant::<TxStatus>("status", text)?,
        None => TxStatus::Confirmed,
    };

    let new = transactions::NewTransaction {
        account_id,
        direction: dto::variant::<Direction>("direction", &body.direction)?,
        amount: dto::amount(
            "amount",
            body.amount_minor,
            body.amount.as_deref(),
            account.currency,
        )?
        .ok_or_else(|| ApiError::Invalid {
            field: "amount".to_owned(),
            reason: "it is missing".to_owned(),
        })?,
        booked_at,
        category_id: body
            .category_id
            .as_deref()
            .map(|id| dto::id::<CategoryId>("category_id", id))
            .transpose()?,
        label: body.label,
        counterparty: body.counterparty,
        note: body.note,
        status,
        // Typed by a person into a browser. The other sources arrive through import and
        // through the device, and they say so themselves.
        source: TxSource::Manual,
    };
    let device = users::web_device(&state.pool, session.user).await?;
    let written = transactions::create(&state.pool, session.user, device, &new).await?;
    Ok(Json(TransactionOut::try_from(&written)?))
}

/// Which account a ledger is about, and on which basis.
#[derive(Debug, Deserialize)]
pub struct LedgerQuery {
    /// The account.
    pub account_id: String,
    /// `confirmed` or `projected`. Absent means projected: the ledger runs through today
    /// into the rows still expected, which is the screen's whole shape.
    pub basis: Option<String>,
}

/// One account's rows with the running balance beside each.
///
/// The running balance is the one number on the ledger screen a browser would be most
/// tempted to compute for itself — it is only a sum — and the one that would most quietly
/// disagree with a handset's when two rows share an instant. So the walk is `mt-core`'s,
/// with its tiebreak, and the browser prints it.
///
/// # Errors
/// [`ApiError::NotFound`] if the account is not this user's, or [`ApiError::Rule`] if the
/// rows will not walk.
pub async fn account_ledger(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<LedgerQuery>,
) -> ApiResult<Json<LedgerOut>> {
    let account_id: AccountId = dto::id("account_id", &query.account_id)?;
    let basis = basis_from(Some(query.basis.as_deref().unwrap_or("projected")))?;
    let account = accounts::get(&state.pool, session.user, account_id).await?;
    let rows = transactions::for_account(&state.pool, session.user, account_id).await?;

    let walk = |basis| {
        ledger::walk(&account, &rows, basis).map_err(|reason| ApiError::Rule {
            rule: "walk",
            reason: reason.to_string(),
        })
    };
    let confirmed = walk(BalanceBasis::Confirmed)?;
    let projected = walk(BalanceBasis::Projected)?;
    let summary = account_out(&account, confirmed.closing, projected.closing);

    let (chosen, name) = match basis {
        BalanceBasis::Confirmed => (&confirmed, "confirmed"),
        BalanceBasis::Projected => (&projected, "projected"),
    };
    Ok(Json(ledger_out(summary, name, chosen)?))
}

/// How far ahead an outlook looks.
#[derive(Debug, Deserialize)]
pub struct OutlookQuery {
    /// Days from now, 1 to 366. Absent means 90.
    pub days: Option<i64>,
}

/// Where each account is headed, and the first row that runs it short.
///
/// One per account, never pooled: money does not move between accounts by itself, so a
/// surplus in the bank does not cover a shortfall in the wallet until someone moves it, and
/// an outlook that pooled them would hide exactly the day that needs a transfer. Archived
/// accounts are left out.
///
/// # Errors
/// [`ApiError::Invalid`] for a window out of range, or [`ApiError::Rule`] if the rows will
/// not walk.
pub async fn outlook(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<OutlookQuery>,
) -> ApiResult<Json<Vec<dto::OutlookOut>>> {
    let days = query.days.unwrap_or(90);
    if !(1..=366).contains(&days) {
        return Err(ApiError::Invalid {
            field: "days".to_owned(),
            reason: "expected a number of days from 1 to 366".to_owned(),
        });
    }
    let from = OffsetDateTime::now_utc();
    let until = from + time::Duration::days(days);

    let held = accounts::list(&state.pool, session.user).await?;
    let rows = transactions::all(&state.pool, session.user).await?;

    held.iter()
        .filter(|account| !account.is_archived())
        .map(|account| {
            let own: Vec<mt_core::model::Transaction> = rows
                .iter()
                .filter(|row| row.account_id == account.id)
                .cloned()
                .collect();
            let walk = |basis| {
                ledger::walk(account, &own, basis).map_err(|reason| ApiError::Rule {
                    rule: "walk",
                    reason: reason.to_string(),
                })
            };
            let confirmed = walk(BalanceBasis::Confirmed)?;
            let projected = walk(BalanceBasis::Projected)?;
            let summary = account_out(account, confirmed.closing, projected.closing);
            let ahead = mt_core::outlook::outlook(&projected, from, until);
            dto::outlook_out(summary, &ahead)
        })
        .collect::<ApiResult<Vec<_>>>()
        .map(Json)
}

/// Which month a breakdown is about, and on whose clock.
#[derive(Debug, Deserialize)]
pub struct BreakdownQuery {
    /// `YYYY-MM`.
    pub month: String,
    /// The reader's offset from UTC in minutes, east positive — `0` in Accra. Decides
    /// which month a row near midnight belongs to.
    pub offset_minutes: Option<i32>,
    /// `confirmed` or `projected`. Absent means confirmed: what happened.
    pub basis: Option<String>,
}

/// Where the money went in a month: each currency the person holds, by category.
///
/// One breakdown per currency, never one converted total. The grouping, the dropping of
/// transfers and the shares are `mt-core`'s; this handler reads rows and names categories.
///
/// The offset is one number for the whole month, which is exact for a zone that never
/// shifts — Ghana's — and off by an hour at the edges of a month in which one does. The
/// person's own zone stored on their profile is the fix, when there is a profile to put
/// it on.
///
/// # Errors
/// [`ApiError::Invalid`] for a month or offset that will not read, or [`ApiError::Rule`].
pub async fn breakdown(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<BreakdownQuery>,
) -> ApiResult<Json<Vec<dto::BreakdownOut>>> {
    let period = month_period(&query.month)?;
    let zone = offset(query.offset_minutes.unwrap_or(0))?;
    let basis = basis_from(Some(query.basis.as_deref().unwrap_or("confirmed")))?;

    let held = accounts::list(&state.pool, session.user).await?;
    let rows = transactions::all(&state.pool, session.user).await?;
    let filed = categories::list(&state.pool, session.user).await?;

    let mut currencies: Vec<mt_core::money::Currency> = Vec::new();
    for account in &held {
        if !currencies.contains(&account.currency) {
            currencies.push(account.currency);
        }
    }

    currencies
        .into_iter()
        .map(|currency| {
            mt_core::breakdown::breakdown(currency, &rows, &filed, period, zone, basis)
                .map(|result| dto::breakdown_out(&result, &filed))
                .map_err(|reason| ApiError::Rule {
                    rule: "breakdown",
                    reason: reason.to_string(),
                })
        })
        .collect::<ApiResult<Vec<_>>>()
        .map(Json)
}

/// The first and last local dates of a `YYYY-MM` month.
fn month_period(text: &str) -> ApiResult<mt_core::budget::Period> {
    let refuse = || ApiError::Invalid {
        field: "month".to_owned(),
        reason: "expected YYYY-MM".to_owned(),
    };
    let (year, month) = text.split_once('-').ok_or_else(refuse)?;
    if year.len() != 4 || month.len() != 2 {
        return Err(refuse());
    }
    let year: i32 = year.parse().map_err(|_| refuse())?;
    let month: u8 = month.parse().map_err(|_| refuse())?;
    let month = time::Month::try_from(month).map_err(|_| refuse())?;
    let starts_on = time::Date::from_calendar_date(year, month, 1).map_err(|_| refuse())?;
    let last = month.length(year);
    let ends_on = time::Date::from_calendar_date(year, month, last).map_err(|_| refuse())?;
    Ok(mt_core::budget::Period { starts_on, ends_on })
}

/// A fixed offset from minutes east of UTC.
fn offset(minutes: i32) -> ApiResult<time::UtcOffset> {
    let refuse = || ApiError::Invalid {
        field: "offset_minutes".to_owned(),
        reason: "expected minutes east of UTC, between -720 and 840".to_owned(),
    };
    if !(-720..=840).contains(&minutes) {
        return Err(refuse());
    }
    time::UtcOffset::from_whole_seconds(minutes.saturating_mul(60)).map_err(|_| refuse())
}

/// Every currency an account may be opened in.
pub async fn currencies() -> Json<Vec<CurrencyOut>> {
    Json(
        mt_core::money::Currency::ALL
            .iter()
            .copied()
            .map(CurrencyOut::from)
            .collect(),
    )
}

/// Every category, in display order.
///
/// # Errors
/// [`ApiError::Internal`].
pub async fn list_categories(
    State(state): State<AppState>,
    session: Session,
) -> ApiResult<Json<Vec<CategoryOut>>> {
    let rows = categories::list(&state.pool, session.user).await?;
    Ok(Json(rows.iter().map(CategoryOut::from).collect()))
}

/// Adds a category.
///
/// # Errors
/// [`ApiError::NotFound`] if the parent is not this user's, or [`ApiError::Invalid`].
pub async fn create_category(
    State(state): State<AppState>,
    session: Session,
    Json(body): Json<NewCategoryIn>,
) -> ApiResult<Json<CategoryOut>> {
    let new = categories::NewCategory {
        name: body.name,
        kind: dto::variant::<CategoryKind>("kind", &body.kind)?,
        parent_id: body
            .parent_id
            .as_deref()
            .map(|id| dto::id::<CategoryId>("parent_id", id))
            .transpose()?,
    };
    let device = users::web_device(&state.pool, session.user).await?;
    let category = categories::create(&state.pool, session.user, device, &new).await?;
    Ok(Json(CategoryOut::from(&category)))
}

/// Files a row under a category, or takes it out of one.
///
/// # Errors
/// [`ApiError::NotFound`] if the row or the category is not this user's, or
/// [`ApiError::Invalid`] for a transfer leg.
pub async fn set_category(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<String>,
    Json(body): Json<SetCategoryIn>,
) -> ApiResult<Json<TransactionOut>> {
    let id: TxId = dto::id("id", &id)?;
    let category = body
        .category_id
        .as_deref()
        .map(|id| dto::id::<CategoryId>("category_id", id))
        .transpose()?;
    let device = users::web_device(&state.pool, session.user).await?;
    let written =
        transactions::set_category(&state.pool, session.user, device, id, category).await?;
    Ok(Json(TransactionOut::try_from(&written)?))
}

/// Changes a row.
///
/// # Errors
/// [`ApiError::NotFound`] if the row or the category is not this user's, or
/// [`ApiError::Invalid`] for a value that will not convert, a transfer leg, or a row the
/// model refuses.
pub async fn edit_transaction(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<String>,
    Json(body): Json<EditTransactionIn>,
) -> ApiResult<Json<TransactionOut>> {
    let id: TxId = dto::id("id", &id)?;
    // The amount is read in the row's own currency, which is its account's.
    let existing = transactions::get(&state.pool, session.user, id).await?;
    let currency = existing.amount.currency();

    let edit = transactions::Edit {
        direction: dto::variant::<Direction>("direction", &body.direction)?,
        amount: dto::amount(
            "amount",
            body.amount_minor,
            body.amount.as_deref(),
            currency,
        )?
        .ok_or_else(|| ApiError::Invalid {
            field: "amount".to_owned(),
            reason: "it is missing".to_owned(),
        })?,
        booked_at: dto::parse_instant("booked_at", &body.booked_at)?,
        category_id: body
            .category_id
            .as_deref()
            .map(|id| dto::id::<CategoryId>("category_id", id))
            .transpose()?,
        label: body.label,
        counterparty: body.counterparty,
        note: body.note,
        status: dto::variant::<TxStatus>("status", &body.status)?,
    };
    let device = users::web_device(&state.pool, session.user).await?;
    let written = transactions::update(&state.pool, session.user, device, id, &edit).await?;
    Ok(Json(TransactionOut::try_from(&written)?))
}

/// Deletes a row, leaving the tombstone sync needs.
///
/// # Errors
/// [`ApiError::NotFound`] if there is no such live row of this user's, or
/// [`ApiError::Invalid`] for a transfer leg.
pub async fn delete_transaction(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let id: TxId = dto::id("id", &id)?;
    let device = users::web_device(&state.pool, session.user).await?;
    transactions::delete(&state.pool, session.user, device, id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// Brings a deleted row back.
///
/// # Errors
/// [`ApiError::NotFound`] if there is no deleted row of this user's with that id.
pub async fn restore_transaction(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<String>,
) -> ApiResult<Json<TransactionOut>> {
    let id: TxId = dto::id("id", &id)?;
    let device = users::web_device(&state.pool, session.user).await?;
    let restored = transactions::restore(&state.pool, session.user, device, id).await?;
    Ok(Json(TransactionOut::try_from(&restored)?))
}

/// Which rows a balance counts.
#[derive(Debug, Deserialize)]
pub struct Basis {
    /// `confirmed` or `projected`. Absent means confirmed.
    pub basis: Option<String>,
}

/// The total held in each currency.
///
/// Never one converted number: a total that spans currencies is an estimate that moves when
/// the rate does, and saying so is the screen's job.
///
/// # Errors
/// [`ApiError::Rule`] if the rows will not walk.
pub async fn currency_totals(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<Basis>,
) -> ApiResult<Json<Vec<MoneyOut>>> {
    let basis = basis_from(query.basis.as_deref())?;
    let accounts = accounts::list(&state.pool, session.user).await?;
    let rows = transactions::all(&state.pool, session.user).await?;

    let totals =
        ledger::totals_by_currency(&accounts, &rows, basis).map_err(|reason| ApiError::Rule {
            rule: "totals",
            reason: reason.to_string(),
        })?;
    Ok(Json(totals.into_values().map(MoneyOut::from).collect()))
}

/// Reads the basis a query asked for.
///
/// Spelled out here rather than through `FromStr`, because `BalanceBasis` is a core type
/// with no wire spelling of its own: it has never been stored in a column or sent over sync.
fn basis_from(supplied: Option<&str>) -> ApiResult<BalanceBasis> {
    match supplied {
        None | Some("confirmed") => Ok(BalanceBasis::Confirmed),
        Some("projected") => Ok(BalanceBasis::Projected),
        Some(_) => Err(ApiError::Invalid {
            field: "basis".to_owned(),
            reason: "expected one of: confirmed, projected".to_owned(),
        }),
    }
}

/// Walks every account on one basis.
fn balances(
    accounts: &[mt_core::model::Account],
    rows: &[mt_core::model::Transaction],
    basis: BalanceBasis,
) -> ApiResult<std::collections::BTreeMap<AccountId, Money>> {
    ledger::balances_by_account(accounts, rows, basis).map_err(|reason| ApiError::Rule {
        rule: "balances",
        reason: reason.to_string(),
    })
}
