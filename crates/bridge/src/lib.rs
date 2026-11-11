//! `mt_bridge` — the only surface Flutter may call.
//!
//! One rule governs everything here: **values cross, handles do not.** The core holds no
//! database connection, no socket and no clock, so the bridge hands it owned values and
//! hands owned values back. Storage stays in drift, transport stays in Dart, and the
//! platform keeps its own time-zone database (see [`zone`]).
//!
//! ## What is here
//!
//! - [`api`] — the published functions. The balance walk, budget evaluation, recurrence
//!   expansion, the categorisation ladder, duplicate scoring, and the two refusals that
//!   belong on a save path.
//! - [`dto`] — one struct per core row, in the wire spellings the sync protocol already
//!   commits to, plus the parameter types the api takes.
//! - [`enums`] — the core's enums mirrored, so Dart gets sealed enums and a new variant is
//!   a compile error on both sides rather than a silent `default:` branch.
//! - [`scalar`] — ids, money, instants and dates, and how each is spelled.
//! - [`zone`] — the user's time zone as a table of offsets, supplied by the platform.
//! - [`error`] — what the boundary refuses, with the field that caused it.
//!
//! ## What is not here
//!
//! No rule. Every decision about what a number is lives in `mt-core` and is reached from
//! here by one call, so the server — which links `mt-core` directly and never sees this
//! crate — cannot disagree with a handset about someone's balance.
//!
//! ## The codegen contract
//!
//! What is settled is which part of this crate codegen reads.
//!
//! **`api.rs` is the input, and the DTO and enum types it names come with it.** Those
//! signatures hold the shape codegen accepts — no lifetimes, no generics, no borrows, and
//! no `mt-core` type anywhere in them.
//!
//! The rest is Rust-side plumbing and must be kept out of the scanned set, by `rust_input`
//! or by `#[frb(ignore)]`. The conversions are public because the integration tests call
//! them, not because Dart should: `scalar::id` is generic, and every `to_core` returns an
//! `mt-core` value. Letting codegen near them would produce opaque pointers — handles
//! crossing the bridge, which is the one thing this crate exists to prevent.

// `forbid` per module, not at the crate root, and not left to the workspace's `deny`.
// Under `deny` alone a hand-written `#[allow(unsafe_code)] unsafe { .. }` compiles clean,
// which is exactly the temptation an FFI-adjacent crate puts in front of someone. These
// six modules are the ones people write; `frb_generated` is the one the generator writes,
// and it is the only place unsafe is permitted at all.
#[forbid(unsafe_code)]
pub mod api;
#[forbid(unsafe_code)]
pub mod dto;
#[forbid(unsafe_code)]
pub mod enums;
#[forbid(unsafe_code)]
pub mod error;
#[forbid(unsafe_code)]
pub mod scalar;
#[forbid(unsafe_code)]
pub mod zone;

/// The `flutter_rust_bridge` glue, written by the generator and edited by nobody.
///
/// This is the one module in the project that uses `unsafe`, because an FFI boundary is
/// unsafe by construction: it hands Dart raw pointers to values Rust owns. Everything the
/// generator reaches is in [`api`].
///
/// The allow is attached here rather than at the crate root, which is what lets the six
/// modules above forbid unsafe outright and the test crates forbid it at their own roots.
/// What is left on plain `deny` is this file — a crate-root `forbid` would conflict with
/// the allow below — and nothing else.
///
/// The clippy denials are listed one by one rather than as a group, because all six of the
/// workspace's live in the `restriction` set that `clippy::all` does not cover. Each is
/// still denied everywhere a person writes code.
#[allow(
    unsafe_code,
    missing_docs,
    clippy::all,
    clippy::pedantic,
    clippy::as_conversions,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]
#[rustfmt::skip]
mod frb_generated;

pub use error::BridgeError;
