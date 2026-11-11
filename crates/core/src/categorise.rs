//! The categorisation ladder: how an ingested row gets a category.
//!
//! Every candidate, from whichever source, climbs the same four rungs and stops at the first
//! hit:
//!
//! 1. a category the source named explicitly;
//! 2. a stored [`CategoryRule`], by priority;
//! 3. the user's own most recent categorised row with the same counterparty, then the same
//!    label;
//! 4. nothing — uncategorised, and the inbox asks.
//!
//! Rung 3 is what makes this feel like it is learning without any of the machinery of
//! learning: the second time a MoMo alert from the same merchant arrives, it lands in the
//! category you put the first one in. A correction made in the inbox writes a rule, so the
//! next one climbs no further than rung 2.
//!
//! Nothing here commits a row to the ledger. The ladder proposes; the inbox disposes.

use std::collections::HashMap;

use crate::enums::{MatchedField, RuleOrigin};
use crate::ids::{CategoryId, CategoryRuleId, TxId};
use crate::model::{BPS_SCALE, Candidate, CategoryRule, MAX_PATTERN_LEN, Transaction};
use regex::{Regex, RegexBuilder};
use time::OffsetDateTime;

/// Confidence of a category the source stated outright. Nothing was guessed.
pub const CONFIDENCE_EXPLICIT: u16 = BPS_SCALE;
/// Confidence of a category assigned by a user-written rule.
pub const CONFIDENCE_USER_RULE: u16 = 9_500;
/// Confidence of a category assigned by a rule shipped in a pack. Slightly lower than a
/// user's own rule, which is the tiebreak if the product ever shows one.
pub const CONFIDENCE_PACK_RULE: u16 = 9_000;
/// Confidence of a category copied from the user's last row with the same counterparty.
pub const CONFIDENCE_HISTORY_COUNTERPARTY: u16 = 8_000;
/// Confidence of a category copied from the user's last row with the same label. Weaker
/// than the counterparty match: labels are freer text and collide more.
pub const CONFIDENCE_HISTORY_LABEL: u16 = 7_000;

/// Largest compiled size, in bytes, allowed for one pattern.
///
/// Patterns arrive from rule packs and over sync, so an unbounded compile is an unbounded
/// allocation requested by something outside the app. The `regex` crate matches in linear
/// time, so there is no backtracking blow-up to defend against as well — only size.
pub const MAX_PATTERN_COMPILED_BYTES: usize = 64 * 1024;

/// Largest lazy-DFA cache, in bytes, allowed for one pattern.
///
/// Separate from [`MAX_PATTERN_COMPILED_BYTES`], and easy to miss: the compiled program is
/// bounded by that constant, but the cache the matcher fills in as it runs is bounded by
/// this one, defaults to 2 MiB *per pattern*, and is allocated lazily. A few hundred regex
/// rules from a pack would otherwise reach hundreds of megabytes on a handset. Past the cap
/// the matcher falls back to a slower engine; it does not stop working.
pub const MAX_PATTERN_CACHE_BYTES: usize = 256 * 1024;

/// A stored pattern that cannot be compiled.
///
/// Raised when the rule set is built, which is to say when a rule is saved or a pack is
/// installed — never swallowed at match time. A rule that silently never matches is worse
/// than one that refuses to save, because the first symptom is a month of miscategorised
/// spending.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuleError {
    /// The pattern is not a valid regular expression, or compiles to something too large.
    #[error("rule {rule} has an invalid pattern: {reason}")]
    BadPattern {
        /// The rule that could not be compiled.
        rule: CategoryRuleId,
        /// What the regular-expression compiler said.
        reason: String,
    },

    /// The pattern was empty, whitespace, or past the column width.
    #[error("rule {rule} has a pattern of {length} characters; the limit is {max}")]
    PatternLength {
        /// The rule at fault.
        rule: CategoryRuleId,
        /// What was supplied, in characters, after trimming.
        length: usize,
        /// The limit.
        max: usize,
    },
}

/// How a category was arrived at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    /// The source named the category itself.
    Explicit,
    /// A stored pattern matched.
    Rule(CategoryRuleId),
    /// Copied from one of the user's own earlier rows.
    History(TxId),
    /// Nothing matched.
    Unmatched,
}

/// The ladder's verdict on one candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verdict {
    /// The category to propose, or `None` for uncategorised.
    pub category_id: Option<CategoryId>,
    /// Which rung produced it.
    pub basis: Basis,
    /// How much to trust it, in basis points of certainty.
    pub confidence_bps: u16,
}

impl Verdict {
    /// The verdict for a candidate nothing matched.
    #[must_use]
    pub const fn unmatched() -> Self {
        Self {
            category_id: None,
            basis: Basis::Unmatched,
            confidence_bps: 0,
        }
    }

    /// True when a category was found.
    #[must_use]
    pub const fn is_matched(&self) -> bool {
        self.category_id.is_some()
    }
}

/// The fields a rule can be tried against.
///
/// `raw_text` is borrowed, never owned and never stored: an SMS body exists in memory for
/// the length of one parse and goes no further. The schema has nowhere to put it, and this
/// type keeps it that way by holding a borrow rather than a `String`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Subject<'a> {
    /// The human-readable description.
    pub label: &'a str,
    /// The merchant, sender or recipient.
    pub counterparty: Option<&'a str>,
    /// The message body, for the length of one parse only.
    pub raw_text: Option<&'a str>,
    /// A category the source stated outright — rung 1.
    pub explicit: Option<CategoryId>,
}

impl<'a> Subject<'a> {
    /// Borrows the matchable fields of a parsed candidate.
    ///
    /// `raw_text` is supplied separately by the parser if at all, because a [`Candidate`]
    /// that has been stored no longer has one.
    #[must_use]
    pub fn from_candidate(candidate: &'a Candidate) -> Self {
        Self {
            label: &candidate.label,
            counterparty: candidate.counterparty.as_deref(),
            raw_text: None,
            explicit: candidate.category_id,
        }
    }

    /// The same, with a message body attached for the length of the parse.
    #[must_use]
    pub fn with_raw_text(mut self, raw_text: &'a str) -> Self {
        self.raw_text = Some(raw_text);
        self
    }

    /// The field a given rule looks at, if the candidate has it.
    fn field(&self, which: MatchedField) -> Option<&str> {
        match which {
            MatchedField::Label => Some(self.label),
            MatchedField::Counterparty => self.counterparty,
            MatchedField::RawText => self.raw_text,
        }
    }
}

/// A compiled, ordered set of stored patterns — rung 2.
///
/// Compiling once and matching many times is the point: categorising an imported statement
/// runs every rule against every row, and recompiling a regular expression inside that loop
/// is the difference between an import that finishes and one that appears to hang.
#[derive(Debug)]
pub struct RuleSet {
    rules: Vec<CompiledRule>,
}

#[derive(Debug)]
struct CompiledRule {
    id: CategoryRuleId,
    category_id: CategoryId,
    matched_field: MatchedField,
    confidence_bps: u16,
    matcher: Matcher,
}

#[derive(Debug)]
enum Matcher {
    /// Case-insensitive substring, lowercased once at compile time.
    Contains(String),
    /// A compiled regular expression, already case-insensitive.
    Pattern(Box<Regex>),
}

impl RuleSet {
    /// Compiles the enabled rules, in the order they will be tried.
    ///
    /// Disabled and soft-deleted rules are dropped rather than compiled: a disabled rule is
    /// kept for reference and may well hold a pattern that no longer compiles.
    ///
    /// Order is `priority` ascending, then `id`. The id tiebreak matters — two rules at the
    /// same priority must pick the same winner on every device, or the same statement
    /// imports differently on a phone and on a laptop.
    ///
    /// # Errors
    /// [`RuleError`] naming the first rule that will not compile. Use this on the path where
    /// a human is waiting — saving a rule, installing a pack — so the bad pattern is refused
    /// at the point someone can fix it. To load rules already in the store, use
    /// [`RuleSet::compile_lenient`].
    pub fn compile(rules: &[CategoryRule]) -> Result<Self, RuleError> {
        let mut compiled = Vec::new();
        for rule in Self::in_match_order(rules) {
            compiled.push(CompiledRule::new(rule)?);
        }
        Ok(Self { rules: compiled })
    }

    /// Compiles what it can, and reports the rest.
    ///
    /// For loading the stored set, where refusing everything is the wrong answer. Patterns
    /// reach the store over sync from another device and from rule packs, and a build of
    /// this crate does not accept every pattern that stock `regex` does — the Unicode
    /// character-class tables are left out to keep the WASM bundle down, so `\p{L}` and
    /// friends compile elsewhere and fail here. One such row must cost the user that one
    /// rule, not every rule they have.
    ///
    /// The failures come back rather than being swallowed, so the caller can surface them —
    /// a rule that silently never fires is the thing this module exists to avoid.
    #[must_use]
    pub fn compile_lenient(rules: &[CategoryRule]) -> (Self, Vec<RuleError>) {
        let mut compiled = Vec::new();
        let mut rejected = Vec::new();
        for rule in Self::in_match_order(rules) {
            match CompiledRule::new(rule) {
                Ok(ready) => compiled.push(ready),
                Err(error) => rejected.push(error),
            }
        }
        (Self { rules: compiled }, rejected)
    }

    /// The live rules, in the order they will be tried.
    fn in_match_order(rules: &[CategoryRule]) -> Vec<&CategoryRule> {
        let mut live: Vec<&CategoryRule> = rules
            .iter()
            .filter(|rule| rule.enabled && !rule.sync.is_deleted())
            .collect();
        live.sort_unstable_by(|left, right| {
            left.priority
                .cmp(&right.priority)
                .then_with(|| left.id.cmp(&right.id))
        });
        live
    }

    /// An empty set, for the cases where no rules are loaded yet.
    #[must_use]
    pub const fn empty() -> Self {
        Self { rules: Vec::new() }
    }

    /// How many rules will be tried.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// True when no rule will ever match.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The first rule that matches, in priority order.
    #[must_use]
    pub fn first_match(&self, subject: &Subject<'_>) -> Option<Verdict> {
        // Each field is folded to lowercase at most once, however many substring rules look
        // at it. Folding inside the loop instead would allocate once per rule per row, which
        // on a statement import is one allocation per rule per line.
        let mut folded = Folded::default();
        self.rules.iter().find_map(|rule| {
            let field = subject.field(rule.matched_field)?;
            let matched = match &rule.matcher {
                Matcher::Contains(needle) => folded
                    .lowercase(rule.matched_field, field)
                    .contains(needle.as_str()),
                Matcher::Pattern(regex) => regex.is_match(field),
            };
            matched.then_some(Verdict {
                category_id: Some(rule.category_id),
                basis: Basis::Rule(rule.id),
                confidence_bps: rule.confidence_bps,
            })
        })
    }
}

/// The subject's fields, lowercased on first use and kept for the rest of the pass.
#[derive(Debug, Default)]
struct Folded {
    label: Option<String>,
    counterparty: Option<String>,
    raw_text: Option<String>,
}

impl Folded {
    fn lowercase(&mut self, which: MatchedField, value: &str) -> &str {
        let slot = match which {
            MatchedField::Label => &mut self.label,
            MatchedField::Counterparty => &mut self.counterparty,
            MatchedField::RawText => &mut self.raw_text,
        };
        slot.get_or_insert_with(|| value.to_lowercase())
    }
}

impl CompiledRule {
    fn new(rule: &CategoryRule) -> Result<Self, RuleError> {
        let pattern = rule.pattern.trim();
        let length = pattern.chars().count();
        if length == 0 || length > MAX_PATTERN_LEN {
            return Err(RuleError::PatternLength {
                rule: rule.id,
                length,
                max: MAX_PATTERN_LEN,
            });
        }

        let matcher = if rule.is_regex {
            let compiled = RegexBuilder::new(pattern)
                .case_insensitive(true)
                .size_limit(MAX_PATTERN_COMPILED_BYTES)
                .dfa_size_limit(MAX_PATTERN_CACHE_BYTES)
                .build()
                .map_err(|error| RuleError::BadPattern {
                    rule: rule.id,
                    reason: error.to_string(),
                })?;
            Matcher::Pattern(Box::new(compiled))
        } else {
            Matcher::Contains(pattern.to_lowercase())
        };

        Ok(Self {
            id: rule.id,
            category_id: rule.category_id,
            matched_field: rule.matched_field,
            confidence_bps: match rule.origin {
                RuleOrigin::User => CONFIDENCE_USER_RULE,
                RuleOrigin::Pack => CONFIDENCE_PACK_RULE,
            },
            matcher,
        })
    }
}

/// The user's own categorised rows, indexed for lookup — rung 3.
///
/// Built once per batch. The alternative, scanning the whole transaction history for every
/// candidate, is the N+1 of local code: fine for the one row typed by hand, quadratic for
/// the thousand-row statement.
#[derive(Debug, Default)]
pub struct HistoryIndex {
    by_counterparty: HashMap<String, Seen>,
    by_label: HashMap<String, Seen>,
}

#[derive(Debug, Clone, Copy)]
struct Seen {
    transaction: TxId,
    category: CategoryId,
    booked_at: OffsetDateTime,
}

impl HistoryIndex {
    /// Indexes the most recent categorised row for each counterparty and each label.
    ///
    /// Transfer legs are skipped — they carry no category by construction — as are
    /// soft-deleted rows and anything uncategorised. "Most recent" is by `booked_at`, with
    /// the row id breaking ties so two devices build the same index.
    #[must_use]
    pub fn build<'a, I>(transactions: I) -> Self
    where
        I: IntoIterator<Item = &'a Transaction>,
    {
        let mut index = Self::default();
        for transaction in transactions {
            let Some(category) = transaction.category_id else {
                continue;
            };
            if transaction.sync.is_deleted() || transaction.is_transfer() {
                continue;
            }
            let seen = Seen {
                transaction: transaction.id,
                category,
                booked_at: transaction.booked_at,
            };
            if let Some(counterparty) = transaction.counterparty.as_deref() {
                remember(&mut index.by_counterparty, counterparty, seen);
            }
            remember(&mut index.by_label, &transaction.label, seen);
        }
        index
    }

    /// The category last used for this counterparty, then for this label.
    ///
    /// Counterparty first because it is the stronger signal: "MTN" as a label covers airtime,
    /// data and a transfer to a friend, while the counterparty is the merchant itself.
    #[must_use]
    pub fn lookup(&self, subject: &Subject<'_>) -> Option<Verdict> {
        let by_counterparty = subject
            .counterparty
            .and_then(|counterparty| self.by_counterparty.get(&normalise(counterparty)))
            .map(|seen| (seen, CONFIDENCE_HISTORY_COUNTERPARTY));
        let by_label = || {
            self.by_label
                .get(&normalise(subject.label))
                .map(|seen| (seen, CONFIDENCE_HISTORY_LABEL))
        };

        by_counterparty
            .or_else(by_label)
            .map(|(seen, confidence_bps)| Verdict {
                category_id: Some(seen.category),
                basis: Basis::History(seen.transaction),
                confidence_bps,
            })
    }

    /// How many distinct counterparties and labels are indexed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_counterparty.len() + self.by_label.len()
    }

    /// True when there is nothing to learn from yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_counterparty.is_empty() && self.by_label.is_empty()
    }
}

/// Keeps the later of the row already indexed and the one being offered.
fn remember(index: &mut HashMap<String, Seen>, key: &str, seen: Seen) {
    let key = normalise(key);
    if key.is_empty() {
        return;
    }
    match index.get(&key) {
        Some(existing) if is_later(existing, &seen) => {}
        _ => {
            index.insert(key, seen);
        }
    }
}

/// Later by instant, with the row id breaking a tie so the index is device-independent.
fn is_later(existing: &Seen, offered: &Seen) -> bool {
    (existing.booked_at, existing.transaction) >= (offered.booked_at, offered.transaction)
}

/// Folds case and surrounding whitespace so "SHOPRITE " and "Shoprite" are one merchant.
///
/// Deliberately not more clever than that. Stripping punctuation or collapsing inner spaces
/// would merge "MTN" and "M.T.N" — and also "VODA 123" and "VODA 456", which are two
/// different things.
fn normalise(value: &str) -> String {
    value.trim().to_lowercase()
}

/// Runs the whole ladder and returns the first rung that answers.
///
/// Infallible on purpose. The patterns were compiled when the [`RuleSet`] was built, which
/// is where a bad one is reported; by the time a candidate is being categorised there is
/// nothing left that can fail, so no ingestion path has to decide what to do with an error
/// in the middle of a thousand-row import.
#[must_use]
pub fn categorise(subject: &Subject<'_>, rules: &RuleSet, history: &HistoryIndex) -> Verdict {
    if let Some(category_id) = subject.explicit {
        return Verdict {
            category_id: Some(category_id),
            basis: Basis::Explicit,
            confidence_bps: CONFIDENCE_EXPLICIT,
        };
    }
    rules
        .first_match(subject)
        .or_else(|| history.lookup(subject))
        .unwrap_or_else(Verdict::unmatched)
}

/// Applies the ladder's verdict to a candidate in place, ready for the inbox.
pub fn apply(candidate: &mut Candidate, verdict: Verdict) {
    candidate.category_id = verdict.category_id;
    candidate.category_confidence_bps = verdict.confidence_bps;
}
