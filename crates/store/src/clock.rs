//! Where the store gets the time.
//!
//! `mt-core` refuses to read a clock — every rule there is told what `now` is — and that is
//! what makes the rules testable by writing down an input and an expected output. The store
//! cannot hold that line all the way: `created_at` and `updated_at` are stamped on every row
//! it writes, and threading `now` down from a tap on a screen to an `INSERT` would put the
//! clock in the signature of every call in the app for no gain.
//!
//! So the clock stops here, behind a trait, and the tests hand over a fixed one.

use core::fmt::Debug;
use time::OffsetDateTime;

/// What the store asks for the time.
pub trait Clock: Debug + Send + Sync {
    /// The current instant, UTC.
    fn now(&self) -> OffsetDateTime;
}

/// The device's own clock.
///
/// Skewed clocks are a sync problem rather than a storage one: a row carries the instant
/// the device that wrote it believed in, and the merge resolves it. See `crates/sync`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}
