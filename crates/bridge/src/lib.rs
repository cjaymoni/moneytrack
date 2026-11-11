//! `mt-bridge` — the only surface Flutter may call.
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
//! The `flutter_rust_bridge` annotations are not here yet: the generated glue lands when the
//! Flutter app does. What is settled is which part of this crate codegen reads.
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

pub mod api;
pub mod dto;
pub mod enums;
pub mod error;
pub mod scalar;
pub mod zone;

pub use error::BridgeError;
