//! Uploading a statement: previewing a CSV to map its columns, then staging its lines.
//!
//! Reading the file is `mt-core`'s `ingest`; proposing a category is its ladder. What
//! happens here is fetching what those need — the account, the stored rules, recent
//! history — and putting the result in the inbox. Nothing reaches the ledger from here.

use axum::Json;
use axum::extract::State;
use base64ct::{Base64, Encoding};
use mt_core::categorise::{self, HistoryIndex, RuleSet, Subject};
use mt_core::enums::InboxSource;
use mt_core::ids::AccountId;
use mt_core::ingest::csv::{self, Delimiter, Mapping};
use mt_core::ingest::{
    self, IngestError, LineProblem, MAX_STATEMENT_BYTES, Staging, Statement, ofx,
};
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::AppState;
use crate::api::dto;
use crate::api::ledger::offset;
use crate::error::{ApiError, ApiResult};
use crate::repo::{accounts, inbox, mappings, rules, transactions};
use crate::session::Session;

/// The largest request an upload may make: the largest statement, base64-encoded, with
/// room for the rest of the body.
pub const MAX_UPLOAD_BODY: usize = MAX_STATEMENT_BYTES / 3 * 4 + 64 * 1024;

/// How far back the ladder's third rung looks for "the last time you filed this merchant".
/// Two years covers anything a person still buys regularly without reading their whole
/// ledger on every upload.
const HISTORY_DAYS: i64 = 730;

/// How many line problems an upload reports in detail. The count is always exact.
const MAX_PROBLEMS_SHOWN: usize = 100;

/// The longest file name kept.
const MAX_FILE_NAME_CHARS: usize = 200;

/// What a browser sends to see the start of a CSV file.
#[derive(Debug, Deserialize)]
pub struct PreviewIn {
    /// The account the statement is for, whose stored mapping comes back with the preview.
    pub account_id: String,
    /// The file's bytes, base64. Only the start is needed; the browser may send just that.
    pub content_base64: String,
    /// The delimiter to split on; guessed when absent.
    pub delimiter: Option<Delimiter>,
}

/// The start of a CSV file, split into cells.
#[derive(Debug, Serialize)]
pub struct PreviewOut {
    /// The delimiter the rows were split on.
    pub delimiter: Delimiter,
    /// The first rows, each a list of cells.
    pub rows: Vec<Vec<String>>,
    /// The mapping last used for this account, to start the form from.
    pub saved_mapping: Option<Mapping>,
}

/// What a browser sends to import a statement.
#[derive(Debug, Deserialize)]
pub struct ImportIn {
    /// The account the statement is for.
    pub account_id: String,
    /// The file's name, for showing beside its lines in the inbox.
    pub file_name: Option<String>,
    /// `csv` or `ofx`.
    pub format: String,
    /// The whole file, base64.
    pub content_base64: String,
    /// Which column is which. Required for CSV, ignored for OFX.
    pub mapping: Option<Mapping>,
    /// The reader's offset, which statement dates are read in.
    pub offset_minutes: Option<i32>,
}

/// A line that was left out.
#[derive(Debug, Serialize)]
pub struct ProblemOut {
    /// Where it was in the file.
    pub line_no: u32,
    /// Why, for a person.
    pub reason: String,
}

/// What an upload did.
#[derive(Debug, Serialize)]
pub struct ImportOut {
    /// The upload's id.
    pub import_id: String,
    /// Lines read from the file.
    pub read: usize,
    /// Lines new to the inbox.
    pub added: u64,
    /// Lines read but already seen in an earlier upload, whatever was decided about them.
    pub already_seen: u64,
    /// How many lines could not be read.
    pub problem_count: usize,
    /// The first of them, in file order.
    pub problems: Vec<ProblemOut>,
}

/// Splits the start of a CSV file into cells, for choosing which column is which.
///
/// # Errors
/// [`ApiError::NotFound`] for an account not this user's, or [`ApiError::Invalid`] for
/// content that is not base64 or is empty.
pub async fn preview(
    State(state): State<AppState>,
    session: Session,
    Json(body): Json<PreviewIn>,
) -> ApiResult<Json<PreviewOut>> {
    let account_id: AccountId = dto::id("account_id", &body.account_id)?;
    accounts::get(&state.pool, session.user, account_id).await?;
    let bytes = decode_content(&body.content_base64)?;
    let preview = csv::preview(&bytes, body.delimiter).map_err(refuse_file)?;
    let saved_mapping = mappings::get(&state.pool, session.user, account_id).await?;
    Ok(Json(PreviewOut {
        delimiter: preview.delimiter,
        rows: preview.rows,
        saved_mapping,
    }))
}

/// Reads a statement and puts its new lines in the inbox, each with a proposed category.
///
/// A CSV mapping that staged at least one line is kept as the account's default.
///
/// # Errors
/// [`ApiError::NotFound`] for an account not this user's, [`ApiError::Invalid`] for a file
/// that cannot be read at all or a CSV upload with no mapping, or [`ApiError::Internal`].
pub async fn import(
    State(state): State<AppState>,
    session: Session,
    Json(body): Json<ImportIn>,
) -> ApiResult<Json<ImportOut>> {
    let zone = offset(body.offset_minutes.unwrap_or(0))?;
    let account_id: AccountId = dto::id("account_id", &body.account_id)?;
    let account = accounts::get(&state.pool, session.user, account_id).await?;
    let source = dto::variant::<InboxSource>("format", &body.format)?;
    let bytes = decode_content(&body.content_base64)?;

    let (statement, mapping) = match source {
        InboxSource::Csv => {
            let mapping = body.mapping.as_ref().ok_or_else(|| ApiError::Invalid {
                field: "mapping".to_owned(),
                reason: "a CSV statement needs to be told which column is which".to_owned(),
            })?;
            let statement = csv::read(&bytes, mapping, account.currency).map_err(refuse_file)?;
            (statement, Some(mapping))
        }
        InboxSource::Ofx => (
            ofx::read(&bytes, account.currency).map_err(refuse_file)?,
            None,
        ),
        InboxSource::Sms | InboxSource::Receipt => {
            return Err(ApiError::Invalid {
                field: "format".to_owned(),
                reason: "expected `csv` or `ofx`".to_owned(),
            });
        }
    };

    let mut staging = ingest::stage(&statement, account_id, &zone);
    propose_categories(&state, &session, &mut staging.staged).await?;

    let import_id = Uuid::now_v7();
    let file_name = body.file_name.as_deref().and_then(file_name);
    let added = inbox::stage(
        &state.pool,
        session.user,
        &inbox::Arrival {
            import_id,
            source,
            file_name: file_name.as_deref(),
            account_id,
            currency: account.currency,
            staged: &staging.staged,
        },
    )
    .await?;

    // Kept only once the lines it read are safely staged, so a failed upload does not
    // quietly change what the form offers next time.
    if let Some(mapping) = mapping.filter(|_| !staging.staged.is_empty()) {
        mappings::put(&state.pool, session.user, account_id, mapping).await?;
    }

    Ok(Json(outcome(import_id, &statement, &staging, added)))
}

/// Runs the categorisation ladder over every staged line.
///
/// A stored rule that no longer compiles is skipped and logged rather than failing the
/// upload: it was saved by an earlier build or a sync, and the person's statement is not
/// the place to discover it.
async fn propose_categories(
    state: &AppState,
    session: &Session,
    staged: &mut [ingest::Staged],
) -> ApiResult<()> {
    let stored = rules::list(&state.pool, session.user).await?;
    let (rule_set, broken) = RuleSet::compile_lenient(&stored);
    if !broken.is_empty() {
        tracing::warn!(
            user = %session.user,
            broken = broken.len(),
            "skipping category rules that do not compile"
        );
    }
    let now = OffsetDateTime::now_utc();
    let recent = transactions::between(
        &state.pool,
        session.user,
        now - Duration::days(HISTORY_DAYS),
        now + Duration::days(HISTORY_DAYS),
    )
    .await?;
    let history = HistoryIndex::build(&recent);

    for line in staged {
        let verdict = categorise::categorise(
            &Subject::from_candidate(&line.candidate),
            &rule_set,
            &history,
        );
        categorise::apply(&mut line.candidate, verdict);
    }
    Ok(())
}

/// The response for an upload: lines the reader refused and lines staging refused, in file
/// order.
fn outcome(import_id: Uuid, statement: &Statement, staging: &Staging, added: u64) -> ImportOut {
    let mut problems: Vec<&LineProblem> =
        statement.problems.iter().chain(&staging.problems).collect();
    problems.sort_by_key(|problem| problem.line_no);
    let staged = u64::try_from(staging.staged.len()).unwrap_or(u64::MAX);
    ImportOut {
        import_id: import_id.to_string(),
        read: staging.staged.len(),
        added,
        already_seen: staged.saturating_sub(added),
        problem_count: problems.len(),
        problems: problems
            .into_iter()
            .take(MAX_PROBLEMS_SHOWN)
            .map(|problem| ProblemOut {
                line_no: problem.line_no,
                reason: problem.problem.to_string(),
            })
            .collect(),
    }
}

/// Reads the base64 body of an upload.
fn decode_content(content: &str) -> ApiResult<Vec<u8>> {
    Base64::decode_vec(content.trim()).map_err(|_| ApiError::Invalid {
        field: "content_base64".to_owned(),
        reason: "it is not base64".to_owned(),
    })
}

/// A whole-file refusal, as the API says it.
fn refuse_file(error: IngestError) -> ApiError {
    ApiError::Invalid {
        field: "file".to_owned(),
        reason: error.to_string(),
    }
}

/// The last path segment of a file name, tidied and cut to length; `None` if nothing is
/// left. Browsers send the bare name, but a name is text from a client, and a path in it
/// says something about the person's disk that the inbox has no use for.
fn file_name(raw: &str) -> Option<String> {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let tidy: String = base
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_FILE_NAME_CHARS)
        .collect();
    let tidy = tidy.trim();
    (!tidy.is_empty()).then(|| tidy.to_owned())
}

#[cfg(test)]
mod tests {
    use super::file_name;

    #[test]
    fn a_file_name_keeps_only_its_last_segment() {
        assert_eq!(file_name("statement.csv").as_deref(), Some("statement.csv"));
        assert_eq!(
            file_name("C:\\Users\\me\\nov.csv").as_deref(),
            Some("nov.csv")
        );
        assert_eq!(file_name("/home/me/nov.ofx").as_deref(), Some("nov.ofx"));
        assert_eq!(file_name("  \t"), None);
        assert_eq!(file_name("dir/"), None);
    }
}
