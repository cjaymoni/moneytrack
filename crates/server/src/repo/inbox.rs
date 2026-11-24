//! The review inbox: statement lines waiting for a person to say what they are.
//!
//! Nothing here reaches the ledger by itself. A line is staged when its statement is
//! uploaded, and stays pending until the person accepts it (it becomes a row), merges it
//! into a row that is already there (it was that row), or rejects it. Each of those settles
//! it for good: the fingerprint stays, so the same line in a later upload is not offered
//! again whichever way it was settled.

use std::collections::{HashMap, HashSet};

use deadpool_postgres::{GenericClient, Pool};
use mt_core::categorise::{self, Lesson};
use mt_core::enums::{Direction, InboxSource, InboxStatus, MatchedField, TxSource, TxStatus};
use mt_core::ids::{AccountId, CategoryId, DeviceId, InboxItemId, TxId, UserId};
use mt_core::ingest::Staged;
use mt_core::model::{Candidate, SyncMeta, Transaction};
use mt_core::money::{Currency, Money};
use time::OffsetDateTime;
use tokio_postgres::Row;
use uuid::Uuid;

use crate::db;
use crate::error::ApiError;
use crate::repo::rows::Reader;
use crate::repo::{rules, transactions};

/// The table, for error messages.
const ENTITY: &str = "inbox item";

/// How many lines one insert carries. A statement is written in a handful of statements
/// rather than one per line, and each stays well under Postgres's parameter limits.
const STAGE_CHUNK: usize = 1_000;

/// The most decisions one request may carry.
pub const MAX_DECISIONS: usize = 500;

/// Prefix of the `source_ref` a row accepted from an upload carries: `import:<id>:<line>`.
const IMPORT_REF_PREFIX: &str = "import:";

/// The columns every read selects.
const COLUMNS: &str = "id, import_id, source, origin_ref, line_no, account_id, direction,
     amount_minor::bigint AS amount_minor, currency::text AS currency,
     booked_at::timestamptz AS booked_at, label, counterparty, category_id,
     category_confidence_bps::integer AS category_confidence_bps, external_id,
     reported_balance_minor, status";

/// A line in the inbox.
#[derive(Debug, Clone)]
pub struct InboxRow {
    /// Row id.
    pub id: InboxItemId,
    /// The upload it arrived in.
    pub import_id: Uuid,
    /// CSV or OFX.
    pub source: InboxSource,
    /// The uploaded file's name.
    pub origin_ref: Option<String>,
    /// Where it was in that file.
    pub line_no: u32,
    /// What the statement says, with the category the ladder proposed.
    pub candidate: Candidate,
    /// Where the review has got to.
    pub status: InboxStatus,
}

impl InboxRow {
    /// The account the line goes into. Always present: the schema requires it.
    ///
    /// # Errors
    /// [`ApiError::Internal`] if a row was read back without one, which the schema forbids.
    pub fn account_id(&self) -> Result<AccountId, ApiError> {
        self.candidate.account_id.ok_or_else(|| ApiError::Internal {
            operation: "read an inbox item",
            reason: format!("inbox item {} has no account", self.id),
        })
    }
}

/// Whether a ledger row was accepted from this upload.
///
/// A line is never offered as a duplicate of another line of the statement it came in: the
/// fingerprint has already told them apart, and two identical fares on one statement are
/// two fares.
#[must_use]
pub fn came_from(transaction: &Transaction, import_id: Uuid) -> bool {
    transaction
        .source_ref
        .as_deref()
        .and_then(|reference| reference.strip_prefix(IMPORT_REF_PREFIX))
        .and_then(|rest| rest.split_once(':'))
        .is_some_and(|(id, _)| id == import_id.to_string())
}

/// One upload's lines, ready to be staged.
#[derive(Debug)]
pub struct Arrival<'a> {
    /// The upload's id.
    pub import_id: Uuid,
    /// CSV or OFX.
    pub source: InboxSource,
    /// The file's name, as the browser gave it.
    pub file_name: Option<&'a str>,
    /// The account the person chose.
    pub account_id: AccountId,
    /// That account's currency, which every line is already in.
    pub currency: Currency,
    /// The lines, each with its proposed category applied.
    pub staged: &'a [Staged],
}

/// Puts an upload's lines in the inbox, skipping any line already seen.
///
/// One database transaction: an upload is staged whole or not at all, so a failure half way
/// never leaves half a statement waiting with the other half unaccounted for.
///
/// # Errors
/// [`ApiError::Internal`] if a write fails.
pub async fn stage(pool: &Pool, user: UserId, arrival: &Arrival<'_>) -> Result<u64, ApiError> {
    let now = OffsetDateTime::now_utc();
    let mut client = db::client(pool).await?;
    let transaction = client
        .transaction()
        .await
        .map_err(|error| ApiError::database("begin staging", &error))?;

    // In fingerprint order, so two uploads of overlapping statements — one exported newest
    // first — take the unique index's locks in the same order and cannot deadlock.
    let mut ordered: Vec<&Staged> = arrival.staged.iter().collect();
    ordered.sort_by(|left, right| left.fingerprint.cmp(&right.fingerprint));
    let ordered: Vec<Staged> = ordered.into_iter().cloned().collect();

    let mut added = 0;
    for chunk in ordered.chunks(STAGE_CHUNK) {
        added += stage_chunk(&transaction, user, arrival, chunk, now).await?;
    }

    transaction
        .commit()
        .await
        .map_err(|error| ApiError::database("commit staging", &error))?;
    Ok(added)
}

/// One multi-row insert, as columns unnested side by side.
async fn stage_chunk(
    client: &impl GenericClient,
    user: UserId,
    arrival: &Arrival<'_>,
    chunk: &[Staged],
    now: OffsetDateTime,
) -> Result<u64, ApiError> {
    let ids: Vec<Uuid> = chunk.iter().map(|_| Uuid::now_v7()).collect();
    let line_nos: Vec<i32> = chunk
        .iter()
        .map(|staged| i32::try_from(staged.line_no).unwrap_or(i32::MAX))
        .collect();
    let fingerprints: Vec<&str> = chunk.iter().map(|s| s.fingerprint.as_str()).collect();
    let directions: Vec<&str> = chunk
        .iter()
        .map(|s| s.candidate.direction.as_str())
        .collect();
    let amounts: Vec<i64> = chunk.iter().map(|s| s.candidate.amount.minor()).collect();
    let booked: Vec<OffsetDateTime> = chunk.iter().map(|s| s.candidate.booked_at).collect();
    let labels: Vec<&str> = chunk.iter().map(|s| s.candidate.label.as_str()).collect();
    let counterparties: Vec<Option<&str>> = chunk
        .iter()
        .map(|s| s.candidate.counterparty.as_deref())
        .collect();
    let categories: Vec<Option<Uuid>> = chunk
        .iter()
        .map(|s| s.candidate.category_id.map(|id| *id.as_uuid()))
        .collect();
    let confidences: Vec<i32> = chunk
        .iter()
        .map(|s| i32::from(s.candidate.category_confidence_bps))
        .collect();
    let external_ids: Vec<Option<&str>> = chunk
        .iter()
        .map(|s| s.candidate.external_id.as_deref())
        .collect();
    let balances: Vec<Option<i64>> = chunk
        .iter()
        .map(|s| s.candidate.reported_balance.map(Money::minor))
        .collect();

    client
        .execute(
            "INSERT INTO inbox_items (
                 user_id, id, import_id, source, received_at, origin_ref, line_no, fingerprint,
                 account_id, direction, amount_minor, currency, booked_at, label, counterparty,
                 category_id, category_confidence_bps, external_id, reported_balance_minor,
                 created_at, updated_at
             )
             SELECT $1, line.id, $2, $3, $4::timestamptz::ledger_instant, $5, line.line_no,
                    line.fingerprint, $6, line.direction, line.amount::amount_minor,
                    $7::text::currency_code, line.booked_at::ledger_instant, line.label,
                    line.counterparty, line.category_id, line.confidence::basis_points,
                    line.external_id, line.balance,
                    $4::timestamptz::ledger_instant, $4::timestamptz::ledger_instant
               FROM UNNEST($8::uuid[], $9::integer[], $10::text[], $11::text[], $12::bigint[],
                           $13::timestamptz[], $14::text[], $15::text[], $16::uuid[],
                           $17::integer[], $18::text[], $19::bigint[])
                    AS line(id, line_no, fingerprint, direction, amount, booked_at, label,
                            counterparty, category_id, confidence, external_id, balance)
             ON CONFLICT (user_id, fingerprint) DO NOTHING",
            &[
                user.as_uuid(),
                &arrival.import_id,
                &arrival.source.as_str(),
                &now,
                &arrival.file_name,
                arrival.account_id.as_uuid(),
                &arrival.currency.code(),
                &ids,
                &line_nos,
                &fingerprints,
                &directions,
                &amounts,
                &booked,
                &labels,
                &counterparties,
                &categories,
                &confidences,
                &external_ids,
                &balances,
            ],
        )
        .await
        .map_err(|error| ApiError::database("stage statement lines", &error))
}

/// What is waiting, oldest line first, and how much is waiting in all.
#[derive(Debug)]
pub struct Waiting {
    /// Up to the limit asked for.
    pub items: Vec<InboxRow>,
    /// Every pending line, including those past the limit.
    pub total: i64,
}

/// The lines still waiting for this user, up to `limit`.
///
/// # Errors
/// [`ApiError::Internal`] if a query fails or a row will not read back.
pub async fn waiting(pool: &Pool, user: UserId, limit: i64) -> Result<Waiting, ApiError> {
    let client = db::client(pool).await?;
    let rows = client
        .query(
            &format!(
                "SELECT {COLUMNS} FROM inbox_items
                  WHERE user_id = $1 AND status = 'pending'
                  ORDER BY booked_at, line_no, id
                  LIMIT $2"
            ),
            &[user.as_uuid(), &limit],
        )
        .await
        .map_err(|error| ApiError::database("list the inbox", &error))?;
    let total: i64 = client
        .query_one(
            "SELECT count(*) FROM inbox_items WHERE user_id = $1 AND status = 'pending'",
            &[user.as_uuid()],
        )
        .await
        .and_then(|row| row.try_get(0))
        .map_err(|error| ApiError::database("count the inbox", &error))?;

    Ok(Waiting {
        items: rows.iter().map(read).collect::<Result<_, _>>()?,
        total,
    })
}

/// What a person decided about one line they are accepting.
#[derive(Debug, Clone, Copy)]
pub struct Decision {
    /// The line.
    pub item: InboxItemId,
    /// Where to file it; `None` leaves it uncategorised.
    pub category_id: Option<CategoryId>,
}

/// Accepts lines into the ledger, each as a row, all in one database transaction.
///
/// A choice of category that differs from the proposal leaves a rule behind, in the same
/// transaction, so the next statement from that merchant is filed the same way.
///
/// # Errors
/// [`ApiError::NotFound`] for a line or category that is not this user's,
/// [`ApiError::Conflict`] for a line already settled or one whose bank reference is already
/// in the account, [`ApiError::Invalid`] for repeated lines, or [`ApiError::Internal`].
pub async fn accept(
    pool: &Pool,
    user: UserId,
    device: DeviceId,
    decisions: &[Decision],
) -> Result<Vec<Transaction>, ApiError> {
    let wanted = distinct_items(decisions)?;

    let mut client = db::client(pool).await?;
    let transaction = client
        .transaction()
        .await
        .map_err(|error| ApiError::database("begin accepting", &error))?;
    let mut pending = lock_pending(&transaction, user, &wanted).await?;

    // Checked inside the transaction, so a category or account deleted a moment ago in
    // another tab is not written to.
    let chosen: Vec<Uuid> = decisions
        .iter()
        .filter_map(|d| d.category_id.map(|id| *id.as_uuid()))
        .collect();
    if !all_live(&transaction, user, LiveTable::Categories, &chosen).await? {
        return Err(ApiError::NotFound { entity: "category" });
    }
    let accounts: Vec<Uuid> = pending
        .values()
        .map(|row| row.account_id().map(|id| *id.as_uuid()))
        .collect::<Result<_, _>>()?;
    if !all_live(&transaction, user, LiveTable::Accounts, &accounts).await? {
        return Err(ApiError::Conflict {
            reason: "the account those lines were for has been closed".to_owned(),
        });
    }

    let now = OffsetDateTime::now_utc();
    let mut lines = Vec::with_capacity(decisions.len());
    for decision in decisions {
        let row = pending
            .remove(&decision.item)
            .ok_or(ApiError::NotFound { entity: ENTITY })?;
        let ledger = ledger_row(&row, decision.category_id, device, now)?;
        lines.push(Accepting {
            row,
            ledger,
            category_id: decision.category_id,
        });
    }

    let known = references_in_ledger(&transaction, user, &lines).await?;
    keep_references_apart(&mut lines, &known)?;

    let ledger_rows: Vec<Transaction> = lines.iter().map(|line| line.ledger.clone()).collect();
    let written = transactions::insert_many(&transaction, user, &ledger_rows).await?;
    settle_accepted(&transaction, user, &lines, now).await?;
    let lessons = lines
        .iter()
        .filter_map(|line| categorise::lesson(&line.row.candidate, line.category_id));
    for lesson in distinct_lessons(lessons) {
        rules::learn(&transaction, user, device, &lesson).await?;
    }

    transaction
        .commit()
        .await
        .map_err(|error| ApiError::database("commit accepting", &error))?;
    Ok(written)
}

/// A line being accepted: the line, the row it becomes, and where the person filed it.
struct Accepting {
    row: InboxRow,
    ledger: Transaction,
    category_id: Option<CategoryId>,
}

/// What an account already holds under a bank reference: the way it moved and how much.
type Referenced = HashMap<(AccountId, String), (Direction, Money)>;

/// The live ledger rows sharing a bank reference with any of the lines, in one query.
async fn references_in_ledger(
    client: &impl GenericClient,
    user: UserId,
    lines: &[Accepting],
) -> Result<Referenced, ApiError> {
    let (accounts, references): (Vec<Uuid>, Vec<&str>) = lines
        .iter()
        .filter_map(|line| {
            line.ledger
                .external_id
                .as_deref()
                .map(|reference| (*line.ledger.account_id.as_uuid(), reference))
        })
        .unzip();
    if references.is_empty() {
        return Ok(Referenced::new());
    }
    let rows = client
        .query(
            "SELECT t.id, t.account_id, t.external_id, t.direction,
                    t.amount_minor::bigint AS amount_minor, t.currency::text AS currency
               FROM transactions t
               JOIN UNNEST($2::uuid[], $3::text[]) AS wanted(account_id, external_id)
                 ON t.account_id = wanted.account_id AND t.external_id = wanted.external_id
              WHERE t.user_id = $1 AND t.deleted_at IS NULL",
            &[user.as_uuid(), &accounts, &references],
        )
        .await
        .map_err(|error| ApiError::database("look up bank references", &error))?;
    rows.iter()
        .map(|row| {
            let reader = Reader::new("transaction", row)?;
            let currency = reader.currency("currency")?;
            Ok((
                (reader.uuid("account_id")?, reader.text("external_id")?),
                (
                    reader.variant("direction")?,
                    reader.money("amount_minor", currency)?,
                ),
            ))
        })
        .collect()
}

/// Settles which lines keep their bank reference, before anything is written.
///
/// An account holds a reference once. A line whose reference the account already holds for
/// the same movement — same way, same amount — is that row arriving again, and is refused
/// with a pointer to "Same payment". A line whose reference names a *different* movement is
/// the case banks create by reusing one reference for a transfer and the charge on it: the
/// line is kept as a row of its own, without the reference, rather than refused or folded
/// into the other. Its fingerprint still keeps the same line out of a later upload. Lines
/// within the request are held to the same rule against each other, in the order given.
///
/// # Errors
/// [`ApiError::Conflict`] naming the line that is already in the ledger, or in the request.
fn keep_references_apart(lines: &mut [Accepting], known: &Referenced) -> Result<(), ApiError> {
    let mut claimed: HashMap<(AccountId, String), (Direction, Money, u32)> = HashMap::new();
    for line in lines.iter_mut() {
        let Some(reference) = line.ledger.external_id.clone() else {
            continue;
        };
        let key = (line.ledger.account_id, reference);
        let movement = (line.ledger.direction, line.ledger.amount);
        if let Some(held) = known.get(&key) {
            if *held == movement {
                return Err(ApiError::Conflict {
                    reason: format!(
                        "line {}: a payment with the same bank reference and amount is already \
                         in this account; mark it as the same payment instead",
                        line.row.line_no
                    ),
                });
            }
            line.ledger.external_id = None;
            continue;
        }
        if let Some((direction, amount, first)) = claimed.get(&key) {
            if (*direction, *amount) == movement {
                return Err(ApiError::Conflict {
                    reason: format!(
                        "lines {first} and {} have the same bank reference and amount, so \
                         they are one payment; add one and mark the other as the same payment",
                        line.row.line_no
                    ),
                });
            }
            line.ledger.external_id = None;
            continue;
        }
        claimed.insert(key, (movement.0, movement.1, line.row.line_no));
    }
    Ok(())
}

/// Marks every accepted line settled, pointing at the row it became, in one statement.
async fn settle_accepted(
    client: &impl GenericClient,
    user: UserId,
    lines: &[Accepting],
    now: OffsetDateTime,
) -> Result<(), ApiError> {
    let items: Vec<Uuid> = lines.iter().map(|line| *line.row.id.as_uuid()).collect();
    let rows: Vec<Uuid> = lines.iter().map(|line| *line.ledger.id.as_uuid()).collect();
    let settled = client
        .execute(
            "UPDATE inbox_items
                SET status = 'accepted', transaction_id = settled.transaction_id,
                    duplicate_of = NULL,
                    updated_at = greatest($4::timestamptz, created_at::timestamptz)
                                 ::ledger_instant
               FROM UNNEST($2::uuid[], $3::uuid[]) AS settled(id, transaction_id)
              WHERE inbox_items.user_id = $1 AND inbox_items.id = settled.id",
            &[user.as_uuid(), &items, &rows, &now],
        )
        .await
        .map_err(|error| ApiError::database("settle accepted lines", &error))?;
    if usize::try_from(settled).ok() != Some(lines.len()) {
        return Err(ApiError::Internal {
            operation: "settle accepted lines",
            reason: format!("settled {settled} of {} locked lines", lines.len()),
        });
    }
    Ok(())
}

/// One lesson per merchant, the last choice winning, in the order first seen.
///
/// A request that files the same merchant twice, two different ways, means the later answer;
/// writing both would leave the earlier as a rule nobody chose last.
fn distinct_lessons(lessons: impl IntoIterator<Item = Lesson>) -> Vec<Lesson> {
    let mut order: Vec<Lesson> = Vec::new();
    let mut at: HashMap<(MatchedField, String), usize> = HashMap::new();
    for lesson in lessons {
        let key = (lesson.matched_field, lesson.pattern.trim().to_lowercase());
        match at.get(&key) {
            Some(&index) => {
                if let Some(slot) = order.get_mut(index) {
                    *slot = lesson;
                }
            }
            None => {
                at.insert(key, order.len());
                order.push(lesson);
            }
        }
    }
    order
}

/// Settles a line as the same event as a row already in the ledger.
///
/// The row must be one the line could be: in the same account, moving money the same way,
/// for the same amount. A shared bank reference is not enough on its own — banks reuse one
/// reference for a transfer and the charge levied on it, and merging the charge into the
/// transfer would lose it — which is the rule `mt_core::dedup` applies too. And it must not have
/// been claimed already by another line of the same statement — two identical fares on one
/// statement are two fares, and merging both into one typed fare would lose one of them.
///
/// When that row is still expected — a repeating row's pending instance, say — the
/// statement is the news that it happened, so the row is confirmed in the same transaction.
///
/// # Errors
/// [`ApiError::NotFound`] for a line or live row that is not this user's,
/// [`ApiError::Conflict`] for a line already settled, a row it cannot be, or a row another
/// line of the statement was already merged into, or [`ApiError::Internal`].
pub async fn merge(
    pool: &Pool,
    user: UserId,
    device: DeviceId,
    item: InboxItemId,
    into: TxId,
) -> Result<(), ApiError> {
    let mut client = db::client(pool).await?;
    let transaction = client
        .transaction()
        .await
        .map_err(|error| ApiError::database("begin merging", &error))?;
    let mut pending = lock_pending(&transaction, user, &[item]).await?;
    let row = pending
        .remove(&item)
        .ok_or(ApiError::NotFound { entity: ENTITY })?;
    let target = transactions::lock(&transaction, user, into).await?;

    if let Some(reason) = why_not_the_same(&row, &target) {
        return Err(ApiError::Conflict {
            reason: reason.to_owned(),
        });
    }
    let already_claimed = transaction
        .query_opt(
            "SELECT 1 FROM inbox_items
              WHERE user_id = $1 AND import_id = $2 AND duplicate_of = $3
                AND status = 'merged'
              LIMIT 1",
            &[user.as_uuid(), &row.import_id, into.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("check a merge target", &error))?;
    if already_claimed.is_some() {
        return Err(ApiError::Conflict {
            reason: "another line of this statement was already merged into that row; \
                     two lines of one statement are two movements"
                .to_owned(),
        });
    }

    let now = OffsetDateTime::now_utc();
    if target.status == TxStatus::Pending {
        transaction
            .execute(
                "UPDATE transactions
                    SET status = 'confirmed',
                        updated_at = greatest($3::timestamptz, created_at::timestamptz)
                                     ::ledger_instant,
                        device_id = $4
                  WHERE user_id = $1 AND id = $2",
                &[user.as_uuid(), into.as_uuid(), &now, device.as_uuid()],
            )
            .await
            .map_err(|error| ApiError::database("confirm a merged row", &error))?;
    }
    settle(&transaction, user, item, Settled::Merged(into), now).await?;

    transaction
        .commit()
        .await
        .map_err(|error| ApiError::database("commit merging", &error))
}

/// Why a ledger row cannot be the movement a line describes, if it cannot.
fn why_not_the_same(row: &InboxRow, target: &Transaction) -> Option<&'static str> {
    let line = &row.candidate;
    if line.account_id != Some(target.account_id) {
        return Some("that row is in a different account");
    }
    if line.direction != target.direction {
        return Some("that row moves money the other way");
    }
    if line.amount != target.amount {
        return Some("that row is for a different amount");
    }
    None
}

/// The ledger rows each upload's lines have already been merged into, for leaving them out
/// of that upload's other lines' suspects.
///
/// # Errors
/// [`ApiError::Internal`] if the query fails.
pub async fn claimed(
    pool: &Pool,
    user: UserId,
    imports: &[Uuid],
) -> Result<HashSet<(Uuid, TxId)>, ApiError> {
    let client = db::client(pool).await?;
    let rows = client
        .query(
            "SELECT import_id, duplicate_of FROM inbox_items
              WHERE user_id = $1 AND import_id = ANY($2) AND status = 'merged'",
            &[user.as_uuid(), &imports],
        )
        .await
        .map_err(|error| ApiError::database("list merged lines", &error))?;
    rows.iter()
        .map(|row| {
            let import: Uuid = row.try_get(0)?;
            let into: Uuid = row.try_get(1)?;
            Ok((import, TxId::from(into)))
        })
        .collect::<Result<_, tokio_postgres::Error>>()
        .map_err(|error| ApiError::database("read merged lines", &error))
}

/// How many lines wait, in all and per account.
#[derive(Debug)]
pub struct Counts {
    /// Every waiting line.
    pub total: i64,
    /// Waiting lines per account, for the accounts that have any.
    pub by_account: Vec<(AccountId, i64)>,
}

/// How many lines wait, without reading them.
///
/// # Errors
/// [`ApiError::Internal`] if the query fails.
pub async fn counts(pool: &Pool, user: UserId) -> Result<Counts, ApiError> {
    let client = db::client(pool).await?;
    let rows = client
        .query(
            "SELECT account_id, count(*) FROM inbox_items
              WHERE user_id = $1 AND status = 'pending'
              GROUP BY account_id ORDER BY account_id",
            &[user.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("count the inbox", &error))?;
    let by_account = rows
        .iter()
        .map(|row| {
            let account: Uuid = row.try_get(0)?;
            let count: i64 = row.try_get(1)?;
            Ok((AccountId::from(account), count))
        })
        .collect::<Result<Vec<_>, tokio_postgres::Error>>()
        .map_err(|error| ApiError::database("read inbox counts", &error))?;
    Ok(Counts {
        total: by_account.iter().map(|(_, count)| count).sum(),
        by_account,
    })
}

/// Settles a line as not a movement worth recording.
///
/// # Errors
/// [`ApiError::NotFound`] for a line that is not this user's, [`ApiError::Conflict`] for one
/// already settled, or [`ApiError::Internal`].
pub async fn reject(pool: &Pool, user: UserId, item: InboxItemId) -> Result<(), ApiError> {
    let mut client = db::client(pool).await?;
    let transaction = client
        .transaction()
        .await
        .map_err(|error| ApiError::database("begin rejecting", &error))?;
    let mut pending = lock_pending(&transaction, user, &[item]).await?;
    if pending.remove(&item).is_none() {
        return Err(ApiError::NotFound { entity: ENTITY });
    }
    settle(
        &transaction,
        user,
        item,
        Settled::Rejected,
        OffsetDateTime::now_utc(),
    )
    .await?;
    transaction
        .commit()
        .await
        .map_err(|error| ApiError::database("commit rejecting", &error))
}

/// The ids decided on, refusing one decided twice in the same request.
fn distinct_items(decisions: &[Decision]) -> Result<Vec<InboxItemId>, ApiError> {
    let mut seen = HashSet::with_capacity(decisions.len());
    for decision in decisions {
        if !seen.insert(decision.item) {
            return Err(ApiError::Invalid {
                field: "items".to_owned(),
                reason: "the same line is listed twice".to_owned(),
            });
        }
    }
    Ok(decisions.iter().map(|decision| decision.item).collect())
}

/// Locks the named lines for the rest of the transaction and returns them.
///
/// A line that is this user's but already settled is a conflict, not a missing line: the
/// person has two tabs open, or pressed twice, and should be told it was done. A line that
/// is not theirs is simply left out, and the caller reports it as not found.
async fn lock_pending(
    client: &impl GenericClient,
    user: UserId,
    items: &[InboxItemId],
) -> Result<HashMap<InboxItemId, InboxRow>, ApiError> {
    let ids: Vec<Uuid> = items.iter().map(|id| *id.as_uuid()).collect();
    let rows = client
        .query(
            &format!(
                "SELECT {COLUMNS} FROM inbox_items
                  WHERE user_id = $1 AND id = ANY($2)
                  ORDER BY id
                  FOR UPDATE"
            ),
            &[user.as_uuid(), &ids],
        )
        .await
        .map_err(|error| ApiError::database("lock inbox items", &error))?;

    let mut pending = HashMap::with_capacity(rows.len());
    for row in &rows {
        let row = read(row)?;
        if row.status != InboxStatus::Pending {
            return Err(ApiError::Conflict {
                reason: format!(
                    "line {} of that statement has already been {}",
                    row.line_no, row.status
                ),
            });
        }
        pending.insert(row.id, row);
    }
    Ok(pending)
}

/// How a line was settled one at a time. Accepting settles in bulk; see [`settle_accepted`].
#[derive(Debug, Clone, Copy)]
enum Settled {
    Merged(TxId),
    Rejected,
}

/// Records how a line was settled. The line was locked as pending by the caller.
async fn settle(
    client: &impl GenericClient,
    user: UserId,
    item: InboxItemId,
    settled: Settled,
    now: OffsetDateTime,
) -> Result<(), ApiError> {
    let (status, duplicate_of) = match settled {
        Settled::Merged(id) => (InboxStatus::Merged, Some(*id.as_uuid())),
        Settled::Rejected => (InboxStatus::Rejected, None),
    };
    client
        .execute(
            "UPDATE inbox_items
                SET status = $3, transaction_id = NULL, duplicate_of = $4,
                    updated_at = greatest($5::timestamptz, created_at::timestamptz)
                                 ::ledger_instant
              WHERE user_id = $1 AND id = $2",
            &[
                user.as_uuid(),
                item.as_uuid(),
                &status.as_str(),
                &duplicate_of,
                &now,
            ],
        )
        .await
        .map_err(|error| ApiError::database("settle an inbox item", &error))?;
    Ok(())
}

/// A table whose rows may be soft-deleted under a request.
#[derive(Debug, Clone, Copy)]
enum LiveTable {
    Categories,
    Accounts,
}

/// Whether every id names a live row of this user's in the table.
async fn all_live(
    client: &impl GenericClient,
    user: UserId,
    table: LiveTable,
    ids: &[Uuid],
) -> Result<bool, ApiError> {
    let wanted: HashSet<&Uuid> = ids.iter().collect();
    if wanted.is_empty() {
        return Ok(true);
    }
    // The table name comes from this enum, never from input.
    let sql = match table {
        LiveTable::Categories => {
            "SELECT count(*) FROM categories
              WHERE user_id = $1 AND id = ANY($2) AND deleted_at IS NULL"
        }
        LiveTable::Accounts => {
            "SELECT count(*) FROM accounts
              WHERE user_id = $1 AND id = ANY($2) AND deleted_at IS NULL"
        }
    };
    let distinct: Vec<Uuid> = wanted.into_iter().copied().collect();
    let found: i64 = client
        .query_one(sql, &[user.as_uuid(), &distinct])
        .await
        .and_then(|row| row.try_get(0))
        .map_err(|error| ApiError::database("check rows are live", &error))?;
    Ok(usize::try_from(found).ok() == Some(distinct.len()))
}

/// The ledger row an accepted line becomes.
fn ledger_row(
    row: &InboxRow,
    category_id: Option<CategoryId>,
    device: DeviceId,
    now: OffsetDateTime,
) -> Result<Transaction, ApiError> {
    let source = match row.source {
        InboxSource::Csv => TxSource::Csv,
        InboxSource::Ofx => TxSource::Ofx,
        other => {
            return Err(ApiError::Internal {
                operation: "accept an inbox item",
                reason: format!("inbox item {} has source `{other}`", row.id),
            });
        }
    };
    let candidate = &row.candidate;
    let transaction = Transaction {
        id: TxId::from(Uuid::now_v7()),
        account_id: row.account_id()?,
        direction: candidate.direction,
        amount: candidate.amount,
        booked_at: candidate.booked_at,
        category_id,
        label: candidate.label.clone(),
        counterparty: candidate.counterparty.clone(),
        note: None,
        // A statement reports what has happened; one dated ahead of now is the rare bank
        // that posts tomorrow's value date, and stays expected until it arrives.
        status: if candidate.booked_at <= now {
            TxStatus::Confirmed
        } else {
            TxStatus::Pending
        },
        source,
        source_ref: Some(format!(
            "{IMPORT_REF_PREFIX}{}:{}",
            row.import_id, row.line_no
        )),
        external_id: candidate.external_id.clone(),
        recurring_id: None,
        transfer_id: None,
        sync: SyncMeta {
            created_at: now,
            updated_at: now,
            deleted_at: None,
            device_id: device,
        },
    };
    transaction
        .validate()
        .map_err(|reason| ApiError::Internal {
            operation: "accept an inbox item",
            reason: format!("inbox item {}: {reason}", row.id),
        })?;
    Ok(transaction)
}

/// Reads one row.
fn read(row: &Row) -> Result<InboxRow, ApiError> {
    let reader = Reader::new(ENTITY, row)?;
    let currency = reader.currency("currency")?;
    let balance: Option<i64> =
        row.try_get("reported_balance_minor")
            .map_err(|error| ApiError::Internal {
                operation: "read an inbox item",
                reason: error.to_string(),
            })?;
    Ok(InboxRow {
        id: reader.id(),
        import_id: reader.uuid("import_id")?,
        source: reader.variant("source")?,
        origin_ref: reader.optional_text("origin_ref")?,
        line_no: reader.narrowed("line_no")?,
        candidate: Candidate {
            account_id: Some(reader.uuid("account_id")?),
            direction: reader.variant("direction")?,
            amount: reader.money("amount_minor", currency)?,
            booked_at: reader.instant("booked_at")?,
            label: reader.text("label")?,
            counterparty: reader.optional_text("counterparty")?,
            category_id: reader.optional_uuid("category_id")?,
            category_confidence_bps: reader.narrowed("category_confidence_bps")?,
            external_id: reader.optional_text("external_id")?,
            reported_balance: balance.map(|minor| Money::new(minor, currency)),
        },
        status: reader.variant("status")?,
    })
}
