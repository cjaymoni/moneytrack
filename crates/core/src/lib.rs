//! `mt-core` — every rule that decides what a number is.
//!
//! Pure: no I/O, no database handle, no socket, no clock. Time is passed in, rows are passed
//! in, owned values come back. The same crate is compiled into the Flutter apps through
//! `flutter_rust_bridge`, into the web client as WASM, and linked into the Axum server, so a
//! client and a server cannot disagree about someone's balance.
//!
//! ## What is here
//!
//! The data model — [`money`], [`enums`], [`ids`] and [`model`] — describes the shape of the
//! rows and what makes one valid. On top of it sit the five rules that turn rows into
//! answers:
//!
//! - [`ledger`] walks transactions into balances. History and forecast are one walk, because
//!   a future-dated pending row is just a row with a later date.
//! - [`categorise`] runs the four-rung ladder that gives an ingested row a category.
//! - [`recurrence`] expands a template into the concrete pending rows the walk then sees.
//! - [`dedup`] ranks the chance that two records are the same purchase. It never merges.
//! - [`budget`] says where spending stands against a cap, per period and per currency.
//!
//! ## What this crate will not do
//!
//! It will not read a clock, generate an id, or resolve a time zone. Each of those is passed
//! in: `now` as an argument, ids through a closure, and a zone as a function from a local
//! date to its offset. That is what makes every rule here testable by writing down an input
//! and an expected output, and it is why the same code can run on a handset and on a server
//! without either of them having to agree about anything but the numbers.

// The workspace denies unsafe; this crate forbids it, which cannot be switched off by an
// inner `allow`. Nothing here touches a pointer, a socket or an FFI boundary, and nothing
// ever should: it is arithmetic over values.
#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod budget;
pub mod categorise;
pub mod dedup;
pub mod enums;
pub mod ids;
pub mod ledger;
pub mod model;
pub mod money;
pub mod recurrence;
pub mod zone;

pub use enums::{
    AccountKind, BudgetPeriod, CategoryKind, Direction, InboxSource, InboxStatus, MatchedField,
    RecurrenceFreq, RecurrenceScope, RuleOrigin, TxSource, TxStatus, UnknownVariant,
};
pub use ids::{
    AccountId, AttachmentId, BudgetId, CategoryId, CategoryRuleId, DeviceId, InboxItemId,
    RecurringId, RulePackId, TransferId, TxId, UserId,
};
pub use model::{
    Account, Attachment, Budget, Candidate, Category, CategoryRule, FxRate, InboxItem,
    RecurringRule, SyncMeta, Transaction, ValidationError, validate_transfer_pair,
};
pub use money::{Currency, Money, MoneyError, ParseRejection};

pub use budget::{BudgetError, CategoryTree, Level, Period, Status as BudgetStatus};
pub use categorise::{Basis, HistoryIndex, RuleError, RuleSet, Subject, Verdict};
pub use dedup::{DuplicateIndex, MatchRules, Signal, Suspect};
pub use ledger::{BalanceBasis, Entry, Flows, LedgerError, Walk};
pub use recurrence::{Generation, RecurrenceError};
pub use zone::Zone;
