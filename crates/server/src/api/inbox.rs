//! The review inbox: what is waiting, and accepting, merging or rejecting it.
//!
//! The duplicate check runs on every read rather than once at upload. A line can become a
//! duplicate after it arrives — the person types the same purchase by hand the next day, or
//! a repeating row writes its expected instance — and a suspicion worked out at upload
//! would miss exactly those.

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, State};
use mt_core::dedup::{DuplicateIndex, MatchRules, Signal};
use mt_core::enums::{TxSource, TxStatus};
use mt_core::ids::{AccountId, CategoryId, DeviceId, InboxItemId, TxId};
use mt_core::model::{SyncMeta, Transaction};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::api::dto::{self, MoneyOut, TransactionOut};
use crate::error::{ApiError, ApiResult};
use crate::repo::inbox::{self, InboxRow};
use crate::repo::{accounts, transactions, users};
use crate::session::Session;

/// How many waiting lines one read returns. The count of all of them comes back too.
const PAGE: i64 = 200;

/// A ledger row a waiting line might be the same event as.
#[derive(Debug, Serialize)]
pub struct SuspectOut {
    /// The row.
    pub transaction: TransactionOut,
    /// How strong the suspicion is, in basis points.
    pub score_bps: u16,
    /// True only when both carry the bank's own reference. Offered pre-ticked, still not
    /// merged without the person.
    pub certain: bool,
    /// Why, for the inbox to say.
    pub signals: Vec<&'static str>,
}

/// A line waiting for review.
#[derive(Debug, Serialize)]
pub struct InboxItemOut {
    /// Row id.
    pub id: String,
    /// `csv` or `ofx`.
    pub source: &'static str,
    /// The file it came in.
    pub file_name: Option<String>,
    /// Where it was in that file.
    pub line_no: u32,
    /// The account it goes into.
    pub account_id: String,
    /// That account's name.
    pub account_name: String,
    /// `in` or `out`.
    pub direction: &'static str,
    /// How much, always positive.
    pub amount: MoneyOut,
    /// When, RFC 3339.
    pub booked_at: String,
    /// The statement's description.
    pub label: String,
    /// Merchant, sender or recipient.
    pub counterparty: Option<String>,
    /// The category the ladder proposed.
    pub category_id: Option<String>,
    /// How sure it was, basis points.
    pub category_confidence_bps: u16,
    /// The balance the statement reported after this line.
    pub reported_balance: Option<MoneyOut>,
    /// Rows already in the ledger it might be, strongest first.
    pub suspects: Vec<SuspectOut>,
    /// Lines from other uploads, also waiting, that look like the same movement — the same
    /// month uploaded twice, say, as CSV and then as OFX. Accepting both would book it
    /// twice, so a line with any is never part of a bulk accept.
    pub twins: Vec<TwinOut>,
}

/// Another waiting line that looks like the same movement.
#[derive(Debug, Serialize)]
pub struct TwinOut {
    /// Its id.
    pub id: String,
    /// The file it came in.
    pub file_name: Option<String>,
    /// Where it was in that file.
    pub line_no: u32,
    /// Its description.
    pub label: String,
}

/// How many lines wait for one account.
#[derive(Debug, Serialize)]
pub struct AccountCountOut {
    /// The account.
    pub account_id: String,
    /// Lines waiting for it.
    pub waiting: i64,
}

/// How many lines wait, cheaply, for the navigation and the account screens.
#[derive(Debug, Serialize)]
pub struct CountOut {
    /// Every waiting line.
    pub total: i64,
    /// Per account, for accounts with any.
    pub by_account: Vec<AccountCountOut>,
}

/// The inbox.
#[derive(Debug, Serialize)]
pub struct InboxOut {
    /// Every line waiting, including those not in `items`.
    pub total: i64,
    /// The oldest waiting lines.
    pub items: Vec<InboxItemOut>,
}

/// One line to accept.
#[derive(Debug, Deserialize)]
pub struct AcceptItemIn {
    /// The line.
    pub id: String,
    /// Where to file it; absent or null for uncategorised.
    pub category_id: Option<String>,
}

/// Lines to accept together.
#[derive(Debug, Deserialize)]
pub struct AcceptIn {
    /// Up to [`inbox::MAX_DECISIONS`] of them.
    pub items: Vec<AcceptItemIn>,
}

/// What a line is being merged into.
#[derive(Debug, Deserialize)]
pub struct MergeIn {
    /// The ledger row it is the same event as.
    pub transaction_id: String,
}

/// The lines waiting for review, each with the rows it might duplicate.
///
/// # Errors
/// [`ApiError::Internal`] if a query fails.
pub async fn list(State(state): State<AppState>, session: Session) -> ApiResult<Json<InboxOut>> {
    let waiting = inbox::waiting(&state.pool, session.user, PAGE).await?;
    let names: HashMap<AccountId, String> = accounts::list(&state.pool, session.user)
        .await?
        .into_iter()
        .map(|account| (account.id, account.name))
        .collect();

    let rules = MatchRules::default();
    let nearby = match window(&waiting.items, &rules) {
        Some((from, to)) => transactions::between(&state.pool, session.user, from, to).await?,
        None => Vec::new(),
    };
    let ledger = DuplicateIndex::build(&nearby);
    let by_id: HashMap<TxId, &Transaction> = nearby.iter().map(|row| (row.id, row)).collect();
    let imports: Vec<Uuid> = waiting.items.iter().map(|row| row.import_id).collect();
    let claimed = inbox::claimed(&state.pool, session.user, &imports).await?;

    // The waiting lines themselves, as rows the matcher can compare against.
    let stand_ins: Vec<Transaction> = waiting.items.iter().map(stand_in).collect();
    let waiting_index = DuplicateIndex::build(&stand_ins);
    let rows_by_id: HashMap<TxId, &InboxRow> = waiting
        .items
        .iter()
        .map(|row| (TxId::from(*row.id.as_uuid()), row))
        .collect();

    let items = waiting
        .items
        .iter()
        .map(|row| {
            let suspects = ledger
                .suspects(&row.candidate, &rules)
                .into_iter()
                .filter_map(|suspect| {
                    let found = by_id.get(&suspect.transaction)?;
                    let is_this_statements = inbox::came_from(found, row.import_id)
                        || claimed.contains(&(row.import_id, found.id));
                    (!is_this_statements).then_some((suspect, *found))
                })
                .map(|(suspect, found)| {
                    Ok(SuspectOut {
                        transaction: TransactionOut::try_from(found)?,
                        score_bps: suspect.score_bps,
                        certain: suspect.is_certain(),
                        signals: suspect.signals.iter().map(|s| signal(*s)).collect(),
                    })
                })
                .collect::<ApiResult<Vec<_>>>()?;
            let twins = waiting_index
                .suspects(&row.candidate, &rules)
                .into_iter()
                .filter_map(|suspect| rows_by_id.get(&suspect.transaction).copied())
                .filter(|other| other.import_id != row.import_id)
                .map(|other| TwinOut {
                    id: other.id.to_string(),
                    file_name: other.origin_ref.clone(),
                    line_no: other.line_no,
                    label: other.candidate.label.clone(),
                })
                .collect();
            render(row, &names, suspects, twins)
        })
        .collect::<ApiResult<Vec<_>>>()?;

    Ok(Json(InboxOut {
        total: waiting.total,
        items,
    }))
}

/// How many lines wait, without reading or matching any of them.
///
/// # Errors
/// [`ApiError::Internal`] if the query fails.
pub async fn count(State(state): State<AppState>, session: Session) -> ApiResult<Json<CountOut>> {
    let counts = inbox::counts(&state.pool, session.user).await?;
    Ok(Json(CountOut {
        total: counts.total,
        by_account: counts
            .by_account
            .into_iter()
            .map(|(account, waiting)| AccountCountOut {
                account_id: account.to_string(),
                waiting,
            })
            .collect(),
    }))
}

/// A waiting line dressed as a ledger row, so the duplicate matcher can compare two waiting
/// lines the way it compares a line with the ledger. Never written anywhere; its id is the
/// line's own.
fn stand_in(row: &InboxRow) -> Transaction {
    let line = &row.candidate;
    Transaction {
        id: TxId::from(*row.id.as_uuid()),
        account_id: line
            .account_id
            .unwrap_or_else(|| AccountId::from(Uuid::nil())),
        direction: line.direction,
        amount: line.amount,
        booked_at: line.booked_at,
        category_id: None,
        label: line.label.clone(),
        counterparty: line.counterparty.clone(),
        note: None,
        status: TxStatus::Pending,
        source: TxSource::Csv,
        source_ref: None,
        // Left off on purpose. Every line is in the index it is looked up in, and a
        // reference match is decisive and returned alone — so a line carrying one would find
        // only itself and hide the copy from the other upload behind it.
        external_id: None,
        recurring_id: None,
        transfer_id: None,
        sync: SyncMeta {
            created_at: line.booked_at,
            updated_at: line.booked_at,
            deleted_at: None,
            device_id: DeviceId::from(Uuid::nil()),
        },
    }
}

/// Accepts lines into the ledger.
///
/// # Errors
/// [`ApiError::Invalid`] for an empty or oversized request or a malformed id,
/// [`ApiError::NotFound`] or [`ApiError::Conflict`] as [`inbox::accept`] says.
pub async fn accept(
    State(state): State<AppState>,
    session: Session,
    Json(body): Json<AcceptIn>,
) -> ApiResult<Json<serde_json::Value>> {
    if body.items.is_empty() || body.items.len() > inbox::MAX_DECISIONS {
        return Err(ApiError::Invalid {
            field: "items".to_owned(),
            reason: format!("expected 1 to {} lines", inbox::MAX_DECISIONS),
        });
    }
    let decisions = body
        .items
        .iter()
        .map(|item| {
            Ok(inbox::Decision {
                item: dto::id::<InboxItemId>("id", &item.id)?,
                category_id: item
                    .category_id
                    .as_deref()
                    .map(|id| dto::id::<CategoryId>("category_id", id))
                    .transpose()?,
            })
        })
        .collect::<ApiResult<Vec<_>>>()?;

    let device = users::web_device(&state.pool, session.user).await?;
    let written = inbox::accept(&state.pool, session.user, device, &decisions).await?;
    Ok(Json(serde_json::json!({ "accepted": written.len() })))
}

/// Settles a line as a row already in the ledger.
///
/// # Errors
/// [`ApiError::NotFound`] or [`ApiError::Conflict`] as [`inbox::merge`] says.
pub async fn merge(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<String>,
    Json(body): Json<MergeIn>,
) -> ApiResult<Json<serde_json::Value>> {
    let item: InboxItemId = dto::id("id", &id)?;
    let into: TxId = dto::id("transaction_id", &body.transaction_id)?;
    let device = users::web_device(&state.pool, session.user).await?;
    inbox::merge(&state.pool, session.user, device, item, into).await?;
    Ok(Json(serde_json::json!({ "merged": true })))
}

/// Settles a line as not worth recording.
///
/// # Errors
/// [`ApiError::NotFound`] or [`ApiError::Conflict`] as [`inbox::reject`] says.
pub async fn reject(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let item: InboxItemId = dto::id("id", &id)?;
    inbox::reject(&state.pool, session.user, item).await?;
    Ok(Json(serde_json::json!({ "rejected": true })))
}

/// The span of ledger dates a set of lines could duplicate, if there are any lines.
fn window(
    items: &[InboxRow],
    rules: &MatchRules,
) -> Option<(time::OffsetDateTime, time::OffsetDateTime)> {
    let earliest = items.iter().map(|row| row.candidate.booked_at).min()?;
    let latest = items.iter().map(|row| row.candidate.booked_at).max()?;
    Some((earliest - rules.window, latest + rules.window))
}

/// A signal's name on the wire.
const fn signal(signal: Signal) -> &'static str {
    match signal {
        Signal::SameExternalId => "same_external_id",
        Signal::SameAmount => "same_amount",
        Signal::SameAccount => "same_account",
        Signal::CloseInTime => "close_in_time",
        Signal::SameCounterparty => "same_counterparty",
        Signal::OverlappingCounterparty => "overlapping_counterparty",
        Signal::SameLabel => "same_label",
    }
}

fn render(
    row: &InboxRow,
    names: &HashMap<AccountId, String>,
    suspects: Vec<SuspectOut>,
    twins: Vec<TwinOut>,
) -> ApiResult<InboxItemOut> {
    let account_id = row.account_id()?;
    let candidate = &row.candidate;
    Ok(InboxItemOut {
        id: row.id.to_string(),
        source: row.source.as_str(),
        file_name: row.origin_ref.clone(),
        line_no: row.line_no,
        account_id: account_id.to_string(),
        account_name: names.get(&account_id).cloned().unwrap_or_default(),
        direction: candidate.direction.as_str(),
        amount: candidate.amount.into(),
        booked_at: dto::instant("booked_at", candidate.booked_at)?,
        label: candidate.label.clone(),
        counterparty: candidate.counterparty.clone(),
        category_id: candidate.category_id.map(|id| id.to_string()),
        category_confidence_bps: candidate.category_confidence_bps,
        reported_balance: candidate.reported_balance.map(MoneyOut::from),
        suspects,
        twins,
    })
}
