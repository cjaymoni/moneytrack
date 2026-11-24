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
use mt_core::dedup::MatchRules;
use mt_core::enums::{Direction, InboxSource, InboxStatus, MatchedField, TxSource, TxStatus};
use mt_core::ids::{AccountId, CategoryId, DeviceId, InboxItemId, TxId, UserId};
use mt_core::ingest::Staged;
use mt_core::model::{Candidate, SyncMeta, Transaction};
use mt_core::money::{Currency, Money};
use time::{Duration, OffsetDateTime};
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
    if !all_categories_live(&transaction, user, &chosen).await? {
        return Err(ApiError::NotFound { entity: "category" });
    }
    let accounts: Vec<Uuid> = pending
        .values()
        .map(|row| row.account_id().map(|id| *id.as_uuid()))
        .collect::<Result<_, _>>()?;
    if !lock_live_accounts(&transaction, user, &accounts).await? {
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

    // The accounts are locked above, so no other accept into them can decide between this
    // read and the insert. A row typed or merged meanwhile by another route is still caught
    // by the `transactions_external` index when it carries the reference.
    let known = references_in_ledger(&transaction, user, &lines).await?;
    let movements: Vec<Movement> = lines.iter().map(Movement::of).collect();
    let kept = references_kept(&movements, &known, MatchRules::default().window)?;
    for (line, keeps) in lines.iter_mut().zip(kept) {
        if !keeps {
            line.ledger.external_id = None;
        }
    }

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

/// A line's movement, as far as its bank reference is concerned.
#[derive(Debug, Clone)]
struct Movement {
    account: AccountId,
    reference: Option<String>,
    direction: Direction,
    amount: Money,
    booked_at: OffsetDateTime,
    /// "line 12 of march.ofx (SHOPRITE OSU)", for a message the person can act on.
    described: String,
}

impl Movement {
    fn of(line: &Accepting) -> Self {
        let row = &line.row;
        let file = row
            .origin_ref
            .as_deref()
            .map(|name| format!(" of {name}"))
            .unwrap_or_default();
        Self {
            account: line.ledger.account_id,
            reference: line.ledger.external_id.clone(),
            direction: line.ledger.direction,
            amount: line.ledger.amount,
            booked_at: line.ledger.booked_at,
            described: format!("line {}{file} ({})", row.line_no, row.candidate.label),
        }
    }

    /// The same payment as `other`: same way, same amount, close enough in time to be one
    /// event. A standing order that carries its mandate as the reference every month is
    /// twelve payments, not one.
    fn same_as(
        &self,
        direction: Direction,
        amount: Money,
        booked_at: OffsetDateTime,
        window: Duration,
    ) -> bool {
        self.direction == direction
            && self.amount == amount
            && (self.booked_at - booked_at).abs() <= window
    }
}

/// A live ledger row already known by a bank reference.
#[derive(Debug, Clone, Copy)]
struct Known {
    direction: Direction,
    amount: Money,
    booked_at: OffsetDateTime,
    /// Whether the row carries the reference itself. A row accepted from a line whose
    /// reference had to be dropped is still known by it, through the line it came from.
    holds_reference: bool,
}

/// What each account already holds under each bank reference.
type Referenced = HashMap<(AccountId, String), Vec<Known>>;

/// The live ledger rows known by a bank reference any of the lines carry, in one query: the
/// rows holding it, and the rows that earlier lines carrying it were accepted as or merged
/// into — a row typed by hand and matched to a statement line is known by that line's
/// reference from then on.
async fn references_in_ledger(
    client: &impl GenericClient,
    user: UserId,
    lines: &[Accepting],
) -> Result<Referenced, ApiError> {
    let pairs: HashSet<(Uuid, &str)> = lines
        .iter()
        .filter_map(|line| {
            line.ledger
                .external_id
                .as_deref()
                .map(|reference| (*line.ledger.account_id.as_uuid(), reference))
        })
        .collect();
    if pairs.is_empty() {
        return Ok(Referenced::new());
    }
    let (accounts, references): (Vec<Uuid>, Vec<&str>) = pairs.into_iter().unzip();
    // Two branches rather than one join with an OR, so each can use its own index: the
    // rows holding a reference through `transactions_external`, and the rows a line carrying
    // it was accepted as or merged into, through the inbox.
    let rows = client
        .query(
            "SELECT t.id, wanted.account_id, wanted.external_id, t.direction,
                    t.amount_minor::bigint AS amount_minor, t.currency::text AS currency,
                    t.booked_at::timestamptz AS booked_at, TRUE AS holds_reference
               FROM UNNEST($2::uuid[], $3::text[]) AS wanted(account_id, external_id)
               JOIN transactions t
                 ON t.user_id = $1 AND t.account_id = wanted.account_id
                AND t.external_id = wanted.external_id AND t.deleted_at IS NULL
             UNION ALL
             SELECT t.id, wanted.account_id, wanted.external_id, t.direction,
                    t.amount_minor::bigint, t.currency::text,
                    t.booked_at::timestamptz,
                    t.external_id IS NOT DISTINCT FROM wanted.external_id
               FROM UNNEST($2::uuid[], $3::text[]) AS wanted(account_id, external_id)
               JOIN inbox_items i
                 ON i.user_id = $1 AND i.account_id = wanted.account_id
                AND i.external_id = wanted.external_id
                AND i.status IN ('accepted', 'merged')
               JOIN transactions t
                 ON t.user_id = $1 AND t.deleted_at IS NULL
                AND t.id = CASE i.status WHEN 'accepted' THEN i.transaction_id
                                         ELSE i.duplicate_of END",
            &[user.as_uuid(), &accounts, &references],
        )
        .await
        .map_err(|error| ApiError::database("look up bank references", &error))?;
    let mut known = Referenced::new();
    for row in &rows {
        let reader = Reader::new("transaction", row)?;
        let currency = reader.currency("currency")?;
        let key = (reader.uuid("account_id")?, reader.text("external_id")?);
        known.entry(key).or_default().push(Known {
            direction: reader.variant("direction")?,
            amount: reader.money("amount_minor", currency)?,
            booked_at: reader.instant("booked_at")?,
            holds_reference: reader.flag("holds_reference")?,
        });
    }
    Ok(known)
}

/// Which lines keep their bank reference, decided before anything is written.
///
/// An account holds a reference once. A line that is the same payment as one the ledger
/// already knows by its reference — same way, same amount, within `window` — is that row
/// arriving again, and is refused with a pointer to "Same payment". So is a second copy of a
/// line earlier in the request. Any other line sharing a reference is a movement of its own:
/// banks reuse one reference for a transfer and the charge on it, and standing orders reuse
/// one every month. Such a line is kept as a row, and keeps the reference only if nothing
/// holds it yet; otherwise the row goes in without it.
///
/// Dropping the reference is deliberate: the unique index allows one row per reference. The
/// line's fingerprint still keeps it out of a later upload, and the line itself keeps the
/// reference, which is how a later copy of the same charge is still recognised here.
///
/// # Errors
/// [`ApiError::Conflict`] naming the line that is already in the ledger, or in the request.
fn references_kept(
    lines: &[Movement],
    known: &Referenced,
    window: Duration,
) -> Result<Vec<bool>, ApiError> {
    let mut earlier: HashMap<(AccountId, &str), Vec<&Movement>> = HashMap::new();
    let mut kept = Vec::with_capacity(lines.len());
    for line in lines {
        let Some(reference) = line.reference.as_deref() else {
            kept.push(false);
            continue;
        };
        let seen = earlier.entry((line.account, reference)).or_default();
        if let Some(first) = seen
            .iter()
            .find(|other| line.same_as(other.direction, other.amount, other.booked_at, window))
        {
            return Err(ApiError::Conflict {
                reason: format!(
                    "{} and {} have the same bank reference and amount, so they are one \
                     payment; add one and mark the other as the same payment",
                    first.described, line.described
                ),
            });
        }
        let held = known
            .get(&(line.account, reference.to_owned()))
            .map(Vec::as_slice)
            .unwrap_or_default();
        if held
            .iter()
            .any(|row| line.same_as(row.direction, row.amount, row.booked_at, window))
        {
            return Err(ApiError::Conflict {
                reason: format!(
                    "{}: a payment with the same bank reference and amount is already in \
                     this account; mark it as the same payment instead",
                    line.described
                ),
            });
        }
        kept.push(seen.is_empty() && !held.iter().any(|row| row.holds_reference));
        seen.push(line);
    }
    Ok(kept)
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

/// Distinct ids, for binding to `ANY($n)` and comparing a count against.
fn distinct(ids: &[Uuid]) -> Vec<Uuid> {
    let set: HashSet<&Uuid> = ids.iter().collect();
    set.into_iter().copied().collect()
}

/// Whether every id names a live category of this user's.
async fn all_categories_live(
    client: &impl GenericClient,
    user: UserId,
    ids: &[Uuid],
) -> Result<bool, ApiError> {
    let wanted = distinct(ids);
    if wanted.is_empty() {
        return Ok(true);
    }
    let found: i64 = client
        .query_one(
            "SELECT count(*) FROM categories
              WHERE user_id = $1 AND id = ANY($2) AND deleted_at IS NULL",
            &[user.as_uuid(), &wanted],
        )
        .await
        .and_then(|row| row.try_get(0))
        .map_err(|error| ApiError::database("check categories are live", &error))?;
    Ok(usize::try_from(found).ok() == Some(wanted.len()))
}

/// Locks the accounts lines are going into until the transaction ends, and says whether
/// every one is live and this user's.
///
/// This is what makes accepting safe against a second request for the same account: the
/// reference check reads the ledger and then decides, and two requests deciding at once
/// could each drop a reference and each write the same charge — a case the unique index
/// cannot see, because neither row carries the reference. So accepts into one account take
/// turns. `FOR NO KEY UPDATE` leaves the foreign-key share lock an ordinary insert takes
/// alone, so typing a row into the account is not held up; accounts are locked in id order,
/// after the inbox lines, so two accepts cannot deadlock.
async fn lock_live_accounts(
    client: &impl GenericClient,
    user: UserId,
    ids: &[Uuid],
) -> Result<bool, ApiError> {
    let wanted = distinct(ids);
    if wanted.is_empty() {
        return Ok(true);
    }
    let rows = client
        .query(
            "SELECT id FROM accounts
              WHERE user_id = $1 AND id = ANY($2) AND deleted_at IS NULL
              ORDER BY id
              FOR NO KEY UPDATE",
            &[user.as_uuid(), &wanted],
        )
        .await
        .map_err(|error| ApiError::database("lock accounts", &error))?;
    Ok(rows.len() == wanted.len())
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use time::macros::datetime;

    const NOV_3: OffsetDateTime = datetime!(2026-11-03 09:00 UTC);
    const WINDOW: Duration = Duration::days(2);

    fn account() -> AccountId {
        AccountId::from(Uuid::from_u128(1))
    }

    fn ghs(minor: i64) -> Money {
        Money::new(minor, Currency::GHS)
    }

    fn line(no: u32, reference: Option<&str>, minor: i64, at: OffsetDateTime) -> Movement {
        Movement {
            account: account(),
            reference: reference.map(str::to_owned),
            direction: Direction::Out,
            amount: ghs(minor),
            booked_at: at,
            described: format!("line {no}"),
        }
    }

    fn held(minor: i64, at: OffsetDateTime, holds_reference: bool) -> Known {
        Known {
            direction: Direction::Out,
            amount: ghs(minor),
            booked_at: at,
            holds_reference,
        }
    }

    fn ledger(reference: &str, rows: Vec<Known>) -> Referenced {
        HashMap::from([((account(), reference.to_owned()), rows)])
    }

    fn refused(result: Result<Vec<bool>, ApiError>) -> String {
        match result {
            Err(ApiError::Conflict { reason }) => reason,
            other => panic!("expected a conflict, got {other:?}"),
        }
    }

    #[test]
    fn a_new_reference_is_kept_and_a_line_without_one_has_none_to_keep() {
        let lines = [line(1, Some("R"), 500, NOV_3), line(2, None, 100, NOV_3)];
        assert_eq!(
            references_kept(&lines, &Referenced::new(), WINDOW).unwrap(),
            [true, false]
        );
    }

    #[test]
    fn a_charge_beside_its_transfer_is_kept_without_the_reference() {
        let lines = [line(1, Some("R"), 100, NOV_3)];
        let known = ledger("R", vec![held(50_000, NOV_3, true)]);
        assert_eq!(references_kept(&lines, &known, WINDOW).unwrap(), [false]);
    }

    #[test]
    fn the_same_payment_already_in_the_ledger_is_refused() {
        let lines = [line(7, Some("R"), 50_000, NOV_3)];
        let known = ledger("R", vec![held(50_000, NOV_3, true)]);
        assert!(refused(references_kept(&lines, &known, WINDOW)).contains("line 7"));
    }

    #[test]
    fn a_second_copy_of_a_charge_is_refused_in_one_request() {
        // The transfer is in the ledger; two copies of its charge arrive together.
        let lines = [
            line(1, Some("R"), 100, NOV_3),
            line(2, Some("R"), 100, NOV_3),
        ];
        let known = ledger("R", vec![held(50_000, NOV_3, true)]);
        let reason = refused(references_kept(&lines, &known, WINDOW));
        assert!(reason.contains("line 1 and line 2"), "{reason}");

        let empty = [
            line(1, Some("R"), 50_000, NOV_3),
            line(2, Some("R"), 100, NOV_3),
            line(3, Some("R"), 100, NOV_3),
        ];
        let reason = refused(references_kept(&empty, &Referenced::new(), WINDOW));
        assert!(reason.contains("line 2 and line 3"), "{reason}");
    }

    #[test]
    fn a_charge_accepted_earlier_without_its_reference_is_still_known_by_it() {
        let lines = [line(4, Some("R"), 100, NOV_3)];
        let known = ledger(
            "R",
            vec![held(50_000, NOV_3, true), held(100, NOV_3, false)],
        );
        assert!(refused(references_kept(&lines, &known, WINDOW)).contains("line 4"));
    }

    #[test]
    fn the_first_of_two_movements_keeps_the_reference() {
        let lines = [
            line(1, Some("R"), 100, NOV_3),
            line(2, Some("R"), 50_000, NOV_3),
        ];
        assert_eq!(
            references_kept(&lines, &Referenced::new(), WINDOW).unwrap(),
            [true, false]
        );
    }

    #[test]
    fn a_standing_order_reusing_its_reference_each_month_is_a_new_payment() {
        let december = datetime!(2026-12-03 09:00 UTC);
        let known = ledger("MANDATE", vec![held(20_000, NOV_3, true)]);
        assert_eq!(
            references_kept(
                &[line(1, Some("MANDATE"), 20_000, december)],
                &known,
                WINDOW
            )
            .unwrap(),
            [false]
        );
        let both = [
            line(1, Some("MANDATE"), 20_000, NOV_3),
            line(2, Some("MANDATE"), 20_000, december),
        ];
        assert_eq!(
            references_kept(&both, &Referenced::new(), WINDOW).unwrap(),
            [true, false]
        );
    }

    #[test]
    fn the_window_is_inclusive_and_a_second_past_it_is_a_new_payment() {
        let known = ledger("R", vec![held(100, NOV_3, true)]);
        let at_edge = [line(1, Some("R"), 100, NOV_3 + WINDOW)];
        assert!(refused(references_kept(&at_edge, &known, WINDOW)).contains("line 1"));
        let past = [line(
            1,
            Some("R"),
            100,
            NOV_3 + WINDOW + Duration::seconds(1),
        )];
        assert_eq!(references_kept(&past, &known, WINDOW).unwrap(), [false]);
    }

    #[test]
    fn a_refund_under_the_same_reference_is_its_own_movement() {
        let known = ledger("R", vec![held(100, NOV_3, true)]);
        let refund = [Movement {
            direction: Direction::In,
            ..line(1, Some("R"), 100, NOV_3)
        }];
        assert_eq!(references_kept(&refund, &known, WINDOW).unwrap(), [false]);
    }

    #[test]
    fn the_same_reference_in_another_account_is_unrelated() {
        let known = ledger("R", vec![held(100, NOV_3, true)]);
        let elsewhere = [Movement {
            account: AccountId::from(Uuid::from_u128(2)),
            ..line(1, Some("R"), 100, NOV_3)
        }];
        assert_eq!(references_kept(&elsewhere, &known, WINDOW).unwrap(), [true]);
    }

    #[test]
    fn a_merchant_filed_twice_is_learned_once_the_last_way() {
        let first = CategoryId::from(Uuid::from_u128(10));
        let last = CategoryId::from(Uuid::from_u128(11));
        let lesson = |pattern: &str, category_id| Lesson {
            matched_field: MatchedField::Counterparty,
            pattern: pattern.to_owned(),
            category_id,
        };
        let learned = distinct_lessons([lesson("Shoprite", first), lesson(" SHOPRITE ", last)]);
        assert_eq!(learned.len(), 1);
        assert_eq!(learned[0].category_id, last);
    }
}
