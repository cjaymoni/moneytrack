//! The published surface. Everything Flutter may ask the core, and nothing else.
//!
//! Note what is absent: no save, no fetch, no sync. Storage is drift's, transport is Dart's,
//! and the clock belongs to whoever calls — every function here that needs to know the time
//! is told it. The one exception is [`expand_recurrence`], which mints ids, and that is
//! called out where it happens.
//!
//! Each function takes owned values and returns owned values. They are pure in the sense
//! that matters: the same inputs give the same answer on a handset, in a browser and on the
//! server, which is the whole reason the rules live in Rust rather than being written twice.

use mt_core::model::MAX_AMOUNT_MINOR;
use mt_core::money::{Currency, Money, MoneyError};
use mt_core::{budget, categorise, dedup, ledger, recurrence};
use time::Duration;
use uuid::Uuid;

use crate::dto::{
    AccountDto, BudgetDto, CandidateDto, CategoryDto, CategoryRuleDto, RecurringRuleDto,
    TransactionDto, batch, transactions_to_core,
};
use crate::enums::{BalanceBasis, BudgetLevel, DuplicateSignal};
use crate::error::BridgeError;
use crate::scalar::{self, MoneyDto};
use crate::zone::ZoneDto;

/// One row of a balance walk: what moved the balance, and where it stood afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalancePointDto {
    /// The row that moved it.
    pub transaction_id: String,
    /// When that row is booked, RFC 3339.
    pub booked_at: String,
    /// The balance once it had been applied.
    pub balance_after: MoneyDto,
}

/// An account's balance over time, in booked order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkDto {
    /// The account walked.
    pub account_id: String,
    /// The currency every figure here is in.
    pub currency: String,
    /// The balance before any row.
    pub opening: MoneyDto,
    /// Every counted row, sorted by `booked_at` then by id.
    pub entries: Vec<BalancePointDto>,
    /// The balance after the last row.
    pub closing: MoneyDto,
    /// The first row that takes the balance below zero. On a projected walk this is the
    /// outlook's headline: the date the account runs out, and what does it.
    pub first_shortfall: Option<BalancePointDto>,
}

/// An account and what it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountBalanceDto {
    /// The account.
    pub account_id: String,
    /// Its closing balance on the basis asked for.
    pub balance: MoneyDto,
}

/// Money in and money out over a set of rows, with transfers dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowsDto {
    /// The currency all three figures are in.
    pub currency: String,
    /// Total that arrived, as a positive amount.
    pub income: MoneyDto,
    /// Total that left, as a positive amount.
    pub spending: MoneyDto,
    /// `income - spending`.
    pub net: MoneyDto,
}

/// Where one budget stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetStatusDto {
    /// The budget evaluated.
    pub budget_id: String,
    /// First local day counted, `YYYY-MM-DD`.
    pub period_starts_on: String,
    /// Last local day counted, `YYYY-MM-DD`.
    pub period_ends_on: String,
    /// The cap for this period.
    pub cap: MoneyDto,
    /// Spending counted against it, as a positive amount.
    pub spent: MoneyDto,
    /// `cap - spent`. Negative once the cap has been passed.
    pub remaining: MoneyDto,
    /// Spending as basis points of the cap, held at 65535 rather than wrapping.
    pub used_bps: u16,
    /// Under, warning, or over.
    pub level: BudgetLevel,
}

/// What one generation run produced. Write both fields in one database transaction or
/// neither: rows without the watermark are generated again, a watermark without the rows
/// leaves a gap nothing will fill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationDto {
    /// The new pending rows, in date order.
    pub rows: Vec<TransactionDto>,
    /// The value to write back to the template's `generated_through`, `YYYY-MM-DD`.
    pub generated_through: String,
}

/// Which rung of the ladder produced a category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerdictBasis {
    /// The source named the category itself.
    Explicit,
    /// A stored pattern matched; `source_id` is the rule.
    Rule,
    /// Copied from one of the user's own earlier rows; `source_id` is that row.
    History,
    /// Nothing matched.
    Unmatched,
}

/// The ladder's verdict, and anything it had to skip to reach it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategorisationDto {
    /// The category to propose, or absent for uncategorised.
    pub category_id: Option<String>,
    /// Which rung produced it.
    pub basis: VerdictBasis,
    /// The rule or transaction behind the verdict, when there was one.
    pub source_id: Option<String>,
    /// How much to trust it, in basis points.
    pub confidence_bps: u16,
    /// Stored rules that would not compile, one message each.
    ///
    /// Reported rather than swallowed. A rule that stopped compiling — a pack upgrade, a
    /// pattern edited on another device — changes what the ladder answers, so the UI can
    /// say so instead of quietly categorising differently than it did yesterday.
    pub skipped_rules: Vec<String>,
}

/// An existing row that might be the same event as a candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuspectDto {
    /// The existing ledger row.
    pub transaction_id: String,
    /// How strong the suspicion is, in basis points.
    pub score_bps: u16,
    /// Why.
    pub signals: Vec<DuplicateSignal>,
    /// True only for a match on the institution's own reference. Even this licenses a
    /// pre-ticked box, never a silent merge.
    pub is_certain: bool,
}

/// The suspected duplicates of one candidate, in the order the candidates were supplied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateReportDto {
    /// Suspects, strongest first.
    pub suspects: Vec<SuspectDto>,
}

/// The thresholds duplicate matching uses. Absent means the core's defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchRulesDto {
    /// Outside this many seconds, two rows are never the same event.
    pub window_seconds: i64,
    /// Inside this many seconds, they are close enough to score for it.
    pub close_window_seconds: i64,
    /// Suspicions scoring below this many basis points are not reported.
    pub report_at_or_above_bps: u16,
}

impl MatchRulesDto {
    /// Checks the thresholds and converts them.
    ///
    /// Validated like every other value entering this crate, and for the same reason. A
    /// negative window, or a close window wider than the window it sits inside, does not
    /// crash anything — it quietly reports fewer duplicates, which is the dropped row this
    /// crate is organised against wearing a different hat.
    ///
    /// # Errors
    /// [`BridgeError::OutOfRange`] naming the threshold at fault.
    pub fn to_core(&self) -> Result<dedup::MatchRules, BridgeError> {
        if self.window_seconds < 0 {
            return Err(BridgeError::OutOfRange {
                field: "match_rules.window_seconds".to_owned(),
                value: self.window_seconds.to_string(),
            });
        }
        if self.close_window_seconds < 0 || self.close_window_seconds > self.window_seconds {
            return Err(BridgeError::OutOfRange {
                field: "match_rules.close_window_seconds".to_owned(),
                value: self.close_window_seconds.to_string(),
            });
        }
        Ok(dedup::MatchRules {
            window: Duration::seconds(self.window_seconds),
            close_window: Duration::seconds(self.close_window_seconds),
            report_at_or_above: self.report_at_or_above_bps,
        })
    }
}

/// Walks one account's rows and returns the running balance after each.
///
/// History and forecast are the same call: a future-dated pending row is a row with a later
/// date, and [`BalanceBasis::Projected`] admits it. There is no second code path and no
/// projection engine to keep in agreement with this one.
///
/// # Errors
/// [`BridgeError`] for a row that will not convert, or [`BridgeError::Rule`] for a row that
/// belongs to another account, is in another currency, or overflows the running total.
pub fn walk_balance(
    account: AccountDto,
    transactions: Vec<TransactionDto>,
    basis: BalanceBasis,
) -> Result<WalkDto, BridgeError> {
    let account = account.to_core()?;
    let rows = transactions_to_core(&transactions)?;
    let walk = ledger::walk(&account, &rows, basis.into())
        .map_err(|reason| BridgeError::rule("walk", reason))?;

    let entries = walk
        .entries
        .iter()
        .map(point)
        .collect::<Result<Vec<_>, _>>()?;
    let first_shortfall = match walk.first_shortfall() {
        Some(entry) => Some(point(entry)?),
        None => None,
    };

    Ok(WalkDto {
        account_id: walk.account_id.to_string(),
        currency: walk.currency.code().to_owned(),
        opening: walk.opening.into(),
        entries,
        closing: walk.closing.into(),
        first_shortfall,
    })
}

/// Renders one walk entry.
fn point(entry: &ledger::Entry<'_>) -> Result<BalancePointDto, BridgeError> {
    Ok(BalancePointDto {
        transaction_id: entry.transaction.id.to_string(),
        booked_at: scalar::format_instant("booked_at", entry.transaction.booked_at)?,
        balance_after: entry.balance_after.into(),
    })
}

/// Closing balance per account, for every account at once.
///
/// One pass over the rows rather than one pass per account, so the accounts screen does not
/// get slower in proportion to the square of how long someone has used the app.
///
/// # Errors
/// As [`walk_balance`], including a row whose account was not supplied at all.
pub fn account_balances(
    accounts: Vec<AccountDto>,
    transactions: Vec<TransactionDto>,
    basis: BalanceBasis,
) -> Result<Vec<AccountBalanceDto>, BridgeError> {
    let accounts = batch(&accounts, AccountDto::to_core)?;
    let rows = transactions_to_core(&transactions)?;
    let balances = ledger::balances_by_account(&accounts, &rows, basis.into())
        .map_err(|reason| BridgeError::rule("balances", reason))?;

    Ok(balances
        .into_iter()
        .map(|(id, balance)| AccountBalanceDto {
            account_id: id.to_string(),
            balance: balance.into(),
        })
        .collect())
}

/// Total held per currency, one figure each.
///
/// Never one converted number: a total spanning currencies is an estimate that moves when
/// the rate does, and saying so is the UI's job.
///
/// # Errors
/// As [`account_balances`].
pub fn currency_totals(
    accounts: Vec<AccountDto>,
    transactions: Vec<TransactionDto>,
    basis: BalanceBasis,
) -> Result<Vec<MoneyDto>, BridgeError> {
    let accounts = batch(&accounts, AccountDto::to_core)?;
    let rows = transactions_to_core(&transactions)?;
    let totals = ledger::totals_by_currency(&accounts, &rows, basis.into())
        .map_err(|reason| BridgeError::rule("totals", reason))?;
    Ok(totals.into_values().map(MoneyDto::from).collect())
}

/// Totals income and spending over whatever rows are handed in, transfers dropped.
///
/// The window is the caller's: hand over the rows for the month being shown. That keeps one
/// definition of "in this period" per screen rather than one hidden in here.
///
/// # Errors
/// [`BridgeError`] for a row that will not convert, or [`BridgeError::Rule`] on overflow.
pub fn period_flows(
    currency: String,
    transactions: Vec<TransactionDto>,
    basis: BalanceBasis,
) -> Result<FlowsDto, BridgeError> {
    let currency: Currency = scalar::currency("currency", &currency)?;
    let rows = transactions_to_core(&transactions)?;
    let flows = ledger::flows(currency, &rows, basis.into())
        .map_err(|reason| BridgeError::rule("flows", reason))?;
    Ok(FlowsDto {
        currency: flows.currency.code().to_owned(),
        income: flows.income.into(),
        spending: flows.spending.into(),
        net: flows.net.into(),
    })
}

/// Where every budget stands on a local date.
///
/// Dormant budgets are left out rather than reported as empty — a budget that has not
/// started yet has no period to count.
///
/// # Errors
/// [`BridgeError`] for a row that will not convert, or [`BridgeError::Rule`] if a budget
/// cannot be evaluated at all. One bad budget stops the set: a cap of zero or a reversed
/// date range means the store is handing out rows that cannot be trusted.
pub fn evaluate_budgets(
    budgets: Vec<BudgetDto>,
    categories: Vec<CategoryDto>,
    transactions: Vec<TransactionDto>,
    on: String,
    basis: BalanceBasis,
    zone: ZoneDto,
) -> Result<Vec<BudgetStatusDto>, BridgeError> {
    let budgets = batch(&budgets, BudgetDto::to_core)?;
    let categories = batch(&categories, CategoryDto::to_core)?;
    let rows = transactions_to_core(&transactions)?;
    let on = scalar::date("on", &on)?;
    let table = zone.to_table()?;

    let statuses = budget::evaluate_all(&budgets, &categories, &rows, on, basis.into(), &table)
        .map_err(|reason| BridgeError::rule("budget", reason))?;

    statuses
        .into_iter()
        .map(|status| {
            Ok(BudgetStatusDto {
                budget_id: status.budget.to_string(),
                period_starts_on: scalar::format_date("period.starts_on", status.period.starts_on)?,
                period_ends_on: scalar::format_date("period.ends_on", status.period.ends_on)?,
                cap: status.cap.into(),
                spent: status.spent.into(),
                remaining: status.remaining.into(),
                used_bps: status.used_bps,
                level: status.level.into(),
            })
        })
        .collect()
}

/// Expands a recurring template into the concrete pending rows the balance walk then sees.
///
/// This is the one function here that is not pure: it mints a UUIDv7 per row, because a row
/// needs an id before it can be written and asking Dart for a list of ids whose length is
/// only known after the expansion would be worse. Nothing else about it depends on the
/// machine — `now` is passed in, and the zone comes from the caller's table.
///
/// # Errors
/// [`BridgeError`] for a value that will not convert, or [`BridgeError::Rule`] if the
/// template does not validate or the window holds more occurrences than the core will
/// generate in one call.
pub fn expand_recurrence(
    rule: RecurringRuleDto,
    through: String,
    now: String,
    zone: ZoneDto,
) -> Result<GenerationDto, BridgeError> {
    let rule = rule.to_core()?;
    let through = scalar::date("through", &through)?;
    let now = scalar::instant("now", &now)?;
    let table = zone.to_table()?;

    let generation = recurrence::generate(&rule, through, now, &table, || Uuid::now_v7().into())
        .map_err(|reason| BridgeError::rule("recurrence", reason))?;

    Ok(GenerationDto {
        rows: generation
            .rows
            .iter()
            .map(TransactionDto::from_core)
            .collect::<Result<Vec<_>, _>>()?,
        generated_through: scalar::format_date("generated_through", generation.generated_through)?,
    })
}

/// Runs the four-rung ladder over one candidate.
///
/// `raw_text` is the message body, and it lives for the length of this call and is then
/// dropped: nothing stores it, nothing returns it, and no rule here may send it anywhere.
/// That is a policy commitment as much as a design one — see the SMS section of PLAN.md.
///
/// # Errors
/// [`BridgeError`] for a value that will not convert.
pub fn categorise_candidate(
    candidate: CandidateDto,
    rules: Vec<CategoryRuleDto>,
    history: Vec<TransactionDto>,
    raw_text: Option<String>,
) -> Result<CategorisationDto, BridgeError> {
    let candidate = candidate.to_core()?;
    let rules = batch(&rules, CategoryRuleDto::to_core)?;
    let history = transactions_to_core(&history)?;

    let (compiled, skipped) = categorise::RuleSet::compile_lenient(&rules);
    let index = categorise::HistoryIndex::build(&history);
    let subject = categorise::Subject::from_candidate(&candidate);
    let subject = match raw_text.as_deref() {
        Some(text) => subject.with_raw_text(text),
        None => subject,
    };

    let verdict = categorise::categorise(&subject, &compiled, &index);
    let (basis, source_id) = match verdict.basis {
        categorise::Basis::Explicit => (VerdictBasis::Explicit, None),
        categorise::Basis::Rule(id) => (VerdictBasis::Rule, Some(id.to_string())),
        categorise::Basis::History(id) => (VerdictBasis::History, Some(id.to_string())),
        categorise::Basis::Unmatched => (VerdictBasis::Unmatched, None),
    };

    Ok(CategorisationDto {
        category_id: verdict.category_id.map(|id| id.to_string()),
        basis,
        source_id,
        confidence_bps: verdict.confidence_bps,
        skipped_rules: skipped.iter().map(ToString::to_string).collect(),
    })
}

/// Refuses a rule whose pattern will not compile.
///
/// For the path where a human is waiting — saving a rule, installing a pack — so a bad
/// pattern is caught where someone can fix it rather than silently never matching.
///
/// # Errors
/// [`BridgeError::Rule`] naming the first rule that will not compile.
pub fn compile_rules(rules: Vec<CategoryRuleDto>) -> Result<(), BridgeError> {
    let rules = batch(&rules, CategoryRuleDto::to_core)?;
    categorise::RuleSet::compile(&rules)
        .map(|_| ())
        .map_err(|reason| BridgeError::rule("category_rule", reason))
}

/// Scores each candidate against the existing rows, and never merges anything.
///
/// Reports come back in candidate order. The index over the existing rows is built once for
/// the whole batch, which is what makes importing a thousand statement lines linear rather
/// than quadratic.
///
/// # Errors
/// [`BridgeError`] for a row that will not convert, or [`BridgeError::OutOfRange`] for
/// thresholds that do not make sense.
pub fn score_duplicates(
    candidates: Vec<CandidateDto>,
    existing: Vec<TransactionDto>,
    rules: Option<MatchRulesDto>,
) -> Result<Vec<DuplicateReportDto>, BridgeError> {
    let candidates = batch(&candidates, CandidateDto::to_core)?;
    let existing = transactions_to_core(&existing)?;
    let rules = match rules {
        Some(rules) => rules.to_core()?,
        None => dedup::MatchRules::default(),
    };
    let index = dedup::DuplicateIndex::build(&existing);

    Ok(candidates
        .iter()
        .map(|candidate| DuplicateReportDto {
            suspects: index
                .suspects(candidate, &rules)
                .into_iter()
                .map(|suspect| SuspectDto {
                    transaction_id: suspect.transaction.to_string(),
                    score_bps: suspect.score_bps,
                    is_certain: suspect.is_certain(),
                    signals: suspect.signals.into_iter().map(Into::into).collect(),
                })
                .collect(),
        })
        .collect())
}

/// Refuses a transfer whose two legs do not agree.
///
/// Bank to MoMo is neither income nor spending, and the walk nets the pair. A pair that does
/// not balance makes every report wrong, so it is checked before it is written.
///
/// # Errors
/// [`BridgeError::InvalidRow`] saying what does not agree.
pub fn validate_transfer(legs: Vec<TransactionDto>) -> Result<(), BridgeError> {
    let legs = transactions_to_core(&legs)?;
    mt_core::model::validate_transfer_pair(&legs)
        .map_err(|reason| BridgeError::invalid_row("transfer", reason))
}

/// The amount a decimal string denotes, in minor units.
///
/// Entry screens take typed text, and the one place a float could still creep into this
/// project is the conversion from what someone typed. It happens here, in Rust, strictly:
/// no thousands separators, no exponent, and never more decimal places than the currency
/// has.
///
/// # Errors
/// [`BridgeError::InvalidCurrency`] for an unusable code, or [`BridgeError::OutOfRange`]
/// saying why the text is not an amount. The rejection never quotes what was typed: an
/// amount is someone's salary or rent, and an error string ends up in a crash report.
pub fn parse_amount(input: String, currency: String) -> Result<MoneyDto, BridgeError> {
    let currency = scalar::currency("currency", &currency)?;
    let rejected = |reason: &dyn core::fmt::Display| BridgeError::OutOfRange {
        field: "amount".to_owned(),
        value: reason.to_string(),
    };

    let amount = Money::parse(&input, currency).map_err(|error| match error {
        // `MoneyError::Parse`'s own message quotes the input. The rejection inside it does
        // not, and it is the part that says what to fix.
        MoneyError::Parse { reason, .. } => rejected(&reason),
        other => rejected(&other),
    })?;

    // A row may not carry more than this, so refusing it at the keyboard is cheaper than
    // refusing it at save time with the entry screen already dismissed.
    if amount.minor().unsigned_abs() > MAX_AMOUNT_MINOR.unsigned_abs() {
        return Err(BridgeError::OutOfRange {
            field: "amount".to_owned(),
            value: "larger than any row may carry".to_owned(),
        });
    }
    Ok(amount.into())
}
