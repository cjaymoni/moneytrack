//! Spotting the same purchase arriving twice.
//!
//! One trip to the shop can reach the ledger three times: an SMS alert when the card is
//! tapped, a line in next month's statement, and a photographed receipt. Catching that is a
//! feature, not a cleanup script, because by the time a user notices the double count they
//! have lost trust in every number on the screen.
//!
//! ## Nothing here merges anything
//!
//! Every function returns suspicions, ranked. The decision is the user's, in the inbox. The
//! reason is the trotro fare: two GHS 20 journeys on the same day in the same wallet are
//! indistinguishable from one journey recorded twice, by amount and date alone. Software
//! that merges on those two fields quietly deletes a real expense. So the strongest thing a
//! guess can be here is [`Suspect::is_certain`], which needs the institution's own reference
//! on both sides — and even that is offered rather than applied.

use std::collections::HashMap;

use crate::ids::{AccountId, TxId};
use crate::model::{BPS_SCALE, Candidate, Transaction};
use crate::money::{Currency, Money};
use time::{Duration, OffsetDateTime};

/// Score given for the gate alone: same amount, same currency, same direction, inside the
/// window. On its own this is a coincidence, not a duplicate.
pub const SCORE_AMOUNT_AND_WINDOW: u16 = 4_000;
/// Added when the candidate names the account the existing row is in.
pub const SCORE_SAME_ACCOUNT: u16 = 1_500;
/// Added when the two are close enough in time to be the same event seen twice, rather than
/// two events in the same couple of days.
pub const SCORE_CLOSE_IN_TIME: u16 = 1_000;
/// Added when the counterparties normalise to the same string.
pub const SCORE_SAME_COUNTERPARTY: u16 = 2_500;
/// Added when the counterparties share a significant word — "SHOPRITE" against "SHOPRITE
/// ACCRA MALL" — but are not identical.
pub const SCORE_OVERLAPPING_COUNTERPARTY: u16 = 1_200;
/// Added when the labels normalise to the same string.
pub const SCORE_SAME_LABEL: u16 = 500;

/// The score at or above which a pair is worth putting in front of the user.
///
/// Everything above this is *shown*. Nothing is merged.
pub const SCORE_WORTH_REVIEWING: u16 = 6_000;

/// The score an institution's own reference reaches when the candidate's account is not
/// known, so it cannot be confirmed to be the same account's reference.
///
/// Above anything the parsed-field weights can reach, because a matching reference is
/// stronger evidence than any amount of agreement between a guessed label and a guessed
/// date — but short of certain, because a per-account sequence number from another
/// institution looks exactly like this.
pub const SCORE_EXTERNAL_ID_UNPLACED: u16 = SCORE_CERTAIN - 1;

/// The score only an institution's own reference, matching on both sides, can reach.
///
/// The weights above deliberately cannot sum this high: no amount of agreement between a
/// parsed label and a parsed date is as good as the bank telling you it is the same movement.
pub const SCORE_CERTAIN: u16 = BPS_SCALE;

// Enforced rather than asserted in a comment: raising any weight above until a guess could
// reach certainty is a compile error, not a behaviour change nobody notices.
const _: () = assert!(
    SCORE_AMOUNT_AND_WINDOW
        + SCORE_SAME_ACCOUNT
        + SCORE_CLOSE_IN_TIME
        + SCORE_SAME_COUNTERPARTY
        + SCORE_SAME_LABEL
        < SCORE_EXTERNAL_ID_UNPLACED
);
// Keeps the sum above a true maximum: the two counterparty weights are exclusive, so the
// assert only bounds the total while the one it names is the larger of them.
const _: () = assert!(SCORE_OVERLAPPING_COUNTERPARTY <= SCORE_SAME_COUNTERPARTY);

/// How far apart two records of the same event may be.
pub const DEFAULT_WINDOW: Duration = Duration::days(2);
/// How close two records have to be to look like one event rather than two.
///
/// Eighteen hours rather than "the same calendar day", because a statement line dated
/// midnight and an SMS timed at two in the afternoon are the same purchase, and because a
/// calendar day needs a time zone this crate has no business resolving.
pub const DEFAULT_CLOSE_WINDOW: Duration = Duration::hours(18);

/// The thresholds duplicate matching uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchRules {
    /// Outside this, two rows are never the same event.
    pub window: Duration,
    /// Inside this, they are close enough to score [`SCORE_CLOSE_IN_TIME`].
    pub close_window: Duration,
    /// Suspicions scoring below this are not reported at all. A suspicion scoring exactly
    /// this much is reported.
    ///
    /// A match on the institution's own reference ignores this: it is not a guess that can
    /// be tuned out, and a caller that raised the bar was raising it against the scored
    /// signals.
    pub report_at_or_above: u16,
}

impl Default for MatchRules {
    fn default() -> Self {
        Self {
            window: DEFAULT_WINDOW,
            close_window: DEFAULT_CLOSE_WINDOW,
            report_at_or_above: SCORE_WORTH_REVIEWING,
        }
    }
}

/// One reason a pair looks like a duplicate, kept so the inbox can say why it is asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Signal {
    /// Both carry the same reference from the bank or wallet. Decisive.
    SameExternalId,
    /// Same amount, same currency, same direction, inside the window.
    SameAmount,
    /// The candidate names the account the existing row is in.
    SameAccount,
    /// Within [`MatchRules::close_window`] of each other.
    CloseInTime,
    /// The counterparties normalise to the same string.
    SameCounterparty,
    /// The counterparties share a significant word.
    OverlappingCounterparty,
    /// The labels normalise to the same string.
    SameLabel,
}

/// An existing row that might be the same event as the candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suspect {
    /// The existing ledger row.
    pub transaction: TxId,
    /// How strong the suspicion is, in basis points.
    pub score_bps: u16,
    /// Why, in the order the signals are listed on [`Signal`].
    pub signals: Vec<Signal>,
}

impl Suspect {
    /// True only for a match on the institution's own reference.
    ///
    /// Even this does not license a silent merge. It licenses a pre-ticked box.
    #[must_use]
    pub fn is_certain(&self) -> bool {
        self.score_bps >= SCORE_CERTAIN
    }
}

/// Existing rows, bucketed by the things a duplicate must agree on exactly.
///
/// Built once per import. Without it, checking a thousand statement rows against a
/// three-thousand-row ledger is three million comparisons; with it, each candidate touches
/// only the handful of rows for the same amount in the same currency.
#[derive(Debug, Default)]
pub struct DuplicateIndex<'a> {
    by_amount: HashMap<(Currency, i64), Vec<&'a Transaction>>,
    /// Keyed by the reference alone, with the account checked at lookup time.
    ///
    /// The account matters — both schemas enforce `UNIQUE (account_id, external_id)`, so two
    /// institutions numbering their own statement lines from one are not describing the same
    /// purchase — but it decides how strong the match is, not whether to look. A parser that
    /// could not work out the account still has the strongest evidence the system gets, and
    /// throwing it away because a weaker signal is missing is how a real duplicate goes
    /// unnoticed.
    ///
    /// A `Vec` rather than one row, because the ledger may already hold two rows carrying
    /// the same reference, and that is exactly the state the user needs shown to them.
    by_external_id: HashMap<String, Vec<&'a Transaction>>,
}

impl<'a> DuplicateIndex<'a> {
    /// Indexes the rows a candidate will be checked against.
    ///
    /// Soft-deleted rows are left out: a row the user deleted must not keep flagging new
    /// ones as duplicates of it.
    #[must_use]
    pub fn build<I>(transactions: I) -> Self
    where
        I: IntoIterator<Item = &'a Transaction>,
    {
        let mut index = Self::default();
        for transaction in transactions {
            if transaction.sync.is_deleted() {
                continue;
            }
            index
                .by_amount
                .entry(key_of(transaction.amount))
                .or_default()
                .push(transaction);
            if let Some(external_id) = transaction.external_id.as_deref() {
                let external_id = external_id.trim();
                if !external_id.is_empty() {
                    index
                        .by_external_id
                        .entry(external_id.to_owned())
                        .or_default()
                        .push(transaction);
                }
            }
        }
        index
    }

    /// How many rows are indexed by amount.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_amount.values().map(Vec::len).sum()
    }

    /// True when there is nothing to compare against.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_amount.is_empty()
    }

    /// The suspected duplicates of one candidate, strongest first.
    ///
    /// Ties break on the row id so two devices offer the same pair in the same order.
    #[must_use]
    pub fn suspects(&self, candidate: &Candidate, rules: &MatchRules) -> Vec<Suspect> {
        let certain = self.by_external_id_match(candidate);
        if !certain.is_empty() {
            return certain;
        }

        let candidate_words = candidate
            .counterparty
            .as_deref()
            .map(significant_words)
            .unwrap_or_default();

        let mut suspects: Vec<Suspect> = self
            .by_amount
            .get(&key_of(candidate.amount))
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|existing| score(candidate, existing, rules, &candidate_words))
            .filter(|suspect| suspect.score_bps >= rules.report_at_or_above)
            .collect();

        suspects.sort_unstable_by(|left, right| {
            right
                .score_bps
                .cmp(&left.score_bps)
                .then_with(|| left.transaction.cmp(&right.transaction))
        });
        suspects
    }

    /// The strongest case: the institution gave both records the same reference.
    ///
    /// Certain when the candidate names the account the row is in, because that is the
    /// uniqueness the store enforces. Strong but not certain when the parser could not work
    /// the account out — the reference is still the best evidence available, so the pair goes
    /// in front of the user; it just does not arrive pre-ticked.
    ///
    /// A reference matching a row in a *different* account, when the candidate's account is
    /// known to be another one, is not a match at all: per-account sequence numbers collide
    /// constantly and that is not evidence of anything.
    fn by_external_id_match(&self, candidate: &Candidate) -> Vec<Suspect> {
        let Some(external_id) = candidate.external_id.as_deref() else {
            return Vec::new();
        };
        let external_id = external_id.trim();
        if external_id.is_empty() {
            return Vec::new();
        }
        let rows = self
            .by_external_id
            .get(external_id)
            .map(Vec::as_slice)
            .unwrap_or_default();

        let mut found: Vec<Suspect> = rows
            .iter()
            .filter_map(|existing| {
                let score_bps = match candidate.account_id {
                    Some(account_id) if account_id == existing.account_id => SCORE_CERTAIN,
                    Some(_) => return None,
                    None => SCORE_EXTERNAL_ID_UNPLACED,
                };
                Some(Suspect {
                    transaction: existing.id,
                    score_bps,
                    signals: vec![Signal::SameExternalId],
                })
            })
            .collect();
        found.sort_unstable_by_key(|suspect| suspect.transaction);
        found
    }
}

/// The bucket an amount falls in: duplicates must agree on amount and currency exactly.
fn key_of(amount: Money) -> (Currency, i64) {
    (amount.currency(), amount.minor())
}

/// Scores one candidate against one existing row, or `None` if the gate fails.
fn score(
    candidate: &Candidate,
    existing: &Transaction,
    rules: &MatchRules,
    candidate_words: &[String],
) -> Option<Suspect> {
    if candidate.direction != existing.direction || candidate.amount != existing.amount {
        return None;
    }
    let gap = time_between(candidate.booked_at, existing.booked_at);
    if gap > rules.window {
        return None;
    }

    let mut signals = vec![Signal::SameAmount];
    let mut total = SCORE_AMOUNT_AND_WINDOW;

    if same_account(candidate.account_id, existing.account_id) {
        signals.push(Signal::SameAccount);
        total = total.saturating_add(SCORE_SAME_ACCOUNT);
    }
    if gap <= rules.close_window {
        signals.push(Signal::CloseInTime);
        total = total.saturating_add(SCORE_CLOSE_IN_TIME);
    }
    match counterparty_agreement(
        candidate.counterparty.as_deref(),
        existing.counterparty.as_deref(),
        candidate_words,
    ) {
        Agreement::Same => {
            signals.push(Signal::SameCounterparty);
            total = total.saturating_add(SCORE_SAME_COUNTERPARTY);
        }
        Agreement::Overlapping => {
            signals.push(Signal::OverlappingCounterparty);
            total = total.saturating_add(SCORE_OVERLAPPING_COUNTERPARTY);
        }
        Agreement::None => {}
    }
    if normalise(&candidate.label) == normalise(&existing.label) {
        signals.push(Signal::SameLabel);
        total = total.saturating_add(SCORE_SAME_LABEL);
    }

    signals.sort_unstable();
    Some(Suspect {
        transaction: existing.id,
        // Clamped one below certainty: only an external-id match is ever certain, and the
        // weights are chosen so this clamp never actually bites. It is here so that tuning
        // a weight upward later cannot silently promote a guess to a certainty.
        score_bps: total.min(SCORE_CERTAIN - 1),
        signals,
    })
}

/// The gap between two instants, always positive.
fn time_between(left: OffsetDateTime, right: OffsetDateTime) -> Duration {
    if left >= right {
        left - right
    } else {
        right - left
    }
}

/// Whether the candidate names the same account. An unparsed account is not disagreement.
fn same_account(candidate: Option<AccountId>, existing: AccountId) -> bool {
    candidate == Some(existing)
}

/// How much two counterparty strings agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Agreement {
    /// Identical once normalised.
    Same,
    /// Share at least one significant word.
    Overlapping,
    /// Nothing in common, or one of them is missing.
    None,
}

/// Compares two counterparties, allowing for the same merchant being written differently by
/// a wallet and by a bank statement.
fn counterparty_agreement(
    candidate: Option<&str>,
    existing: Option<&str>,
    candidate_words: &[String],
) -> Agreement {
    let (Some(candidate), Some(existing)) = (candidate, existing) else {
        return Agreement::None;
    };
    if normalise(candidate) == normalise(existing) {
        return Agreement::Same;
    }
    if candidate_words.is_empty() {
        return Agreement::None;
    }
    let shares_a_word = significant_words(existing)
        .iter()
        .any(|word| candidate_words.contains(word));
    if shares_a_word {
        Agreement::Overlapping
    } else {
        Agreement::None
    }
}

/// Shortest word worth treating as identifying. "MTN" counts; "TO" and "AT" do not.
const MIN_SIGNIFICANT_WORD: usize = 3;

/// The identifying words of a counterparty: uppercase, letters and digits only.
///
/// Splitting on anything that is not alphanumeric is what lets "SHOPRITE/ACCRA" and
/// "Shoprite Accra Mall" meet. Short words are dropped because matching on "TO" would pair
/// every transfer with every other.
fn significant_words(value: &str) -> Vec<String> {
    value
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| word.chars().count() >= MIN_SIGNIFICANT_WORD)
        .map(str::to_uppercase)
        .collect()
}

/// Folds case and surrounding whitespace.
fn normalise(value: &str) -> String {
    value.trim().to_lowercase()
}

/// The suspected duplicates of one candidate against a set of rows.
///
/// A convenience for the single-row case — a typed entry, one SMS. Importing a file should
/// build a [`DuplicateIndex`] once and reuse it, rather than calling this per row.
#[must_use]
pub fn suspects<'a, I>(candidate: &Candidate, transactions: I, rules: &MatchRules) -> Vec<Suspect>
where
    I: IntoIterator<Item = &'a Transaction>,
{
    DuplicateIndex::build(transactions).suspects(candidate, rules)
}
