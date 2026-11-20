//! Accounts, transactions, and what they add up to.
//!
//! Every number here comes from `mt-core`. The handlers read rows, hand them to the same
//! walk a handset runs locally, and render the answer — they do not add anything up
//! themselves, and there is nothing in this file that decides what counts as spending.

use axum::Json;
use axum::extract::{Query, State};
use mt_core::enums::{AccountKind, Direction, TxSource, TxStatus};
use mt_core::ids::AccountId;
use mt_core::ledger::{self, BalanceBasis};
use mt_core::money::Money;
use serde::Deserialize;
use time::OffsetDateTime;

use crate::AppState;
use crate::api::dto::{
    self, AccountOut, CurrencyOut, LedgerOut, MoneyOut, NewAccountIn, NewTransactionIn,
    TransactionOut, account_out, ledger_out,
};
use crate::error::{ApiError, ApiResult};
use crate::repo::{accounts, transactions, users};
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
        category_id: None,
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
