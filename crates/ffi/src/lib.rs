//! `mt_ffi` — the only surface the two apps may call.
//!
//! One rule governed this crate when Flutter was on the other side of it: values cross,
//! handles do not. The database is in Rust now (`mt-store`), so it reads:
//!
//! > **One handle crosses, and it is the store. Everything else is values.**
//!
//! An app opens a [`store::Store`], holds it, and passes owned values across every call it
//! makes on it. No connection, cursor, statement or row is ever visible to Swift or Kotlin,
//! and the core below still has no idea a database exists.
//!
//! ## What is here
//!
//! - [`api`] — the rules, published directly: the balance walk over rows the caller holds,
//!   budget evaluation, recurrence expansion, the categorisation ladder, duplicate scoring,
//!   and the two refusals that belong on a save path.
//! - [`store`] — the handle: opening, the repositories, the screen queries, and the change
//!   bus the apps re-query on.
//! - [`dto`] — one struct per core row, in the wire spellings the sync protocol already
//!   commits to, plus the parameter types the api takes.
//! - [`enums`] — the core's enums mirrored, so Swift gets an `enum` and Kotlin a sealed
//!   class, and a new variant is a compile error in all three languages rather than a
//!   silent `else` branch.
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
//! UniFFI reads this crate's own attributes rather than scanning a module, so the contract
//! is local: a type crosses because it derives `uniffi::Record`, `uniffi::Enum`,
//! `uniffi::Object` or `uniffi::Error`, and a function crosses because it carries
//! `#[uniffi::export]`. Nothing else is published, which is why the conversions can stay
//! public for the tests to reach without appearing in Swift or Kotlin.
//!
//! The generator and the runtime are the same version by construction: the bindings are
//! written by `cargo run --bin uniffi-bindgen`, which is this crate's own binary.
//!
//! ## Unsafe
//!
//! This crate root forbids it, like every other crate root in the workspace, and that is
//! new: under `flutter_rust_bridge` the generated glue was a file in this crate that had to
//! be allowed, so the root could only deny. UniFFI's scaffolding is written by a macro
//! instead of into a file, and the compiler does not fire lints inside another crate's
//! macro expansion — so what the forbid governs is exactly what it should govern, which is
//! the code in this crate that a person wrote. The FFI's own `unsafe` is still there, in
//! the expansion, doing what an FFI boundary does; it is simply no longer something anyone
//! here can reach around.

#![forbid(unsafe_code)]

pub mod api;
pub mod dto;
pub mod enums;
pub mod error;
pub mod scalar;
pub mod store;
pub mod zone;

pub use error::BridgeError;
pub use store::{Store, StoreError};

// The FFI scaffolding, written by UniFFI's macro and edited by nobody.
//
// This macro is the one place in the project that uses `unsafe`, because an FFI boundary is
// unsafe by construction: it hands raw pointers to values Rust owns across to another
// language's runtime. Everything it reaches is behind a `uniffi` attribute in the modules
// above.
//
// It has to sit at the crate root — it defines the tag type every generated impl is written
// against — so the allow sits on the macro call rather than on the root itself. That is what
// lets the seven modules above forbid unsafe outright, and the test crates forbid it at
// their own roots, while this file stays on the workspace's `deny`.
uniffi::setup_scaffolding!();
