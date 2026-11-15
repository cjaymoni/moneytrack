//! A limit on how fast anyone may try to sign in.
//!
//! Small and in memory, deliberately. It counts attempts per address over a fixed window
//! and refuses past a threshold, which is enough to turn an online password guess from
//! thousands of attempts a minute into a handful. It is not a defence against a distributed
//! attempt from many addresses, and it does not survive a restart or hold across more than
//! one instance of the server — when there are two instances, this moves to shared storage,
//! and that is the moment to notice rather than a reason to skip it now.
//!
//! Argon2id is the real defence. This is the thing that stops the server spending every core
//! it has computing Argon2id hashes for someone else's dictionary.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Attempts allowed from one address per window.
const ATTEMPTS: u32 = 10;

/// How long the window is.
const WINDOW: Duration = Duration::from_secs(60);

/// How many addresses to track before the table is swept. A sweep is O(n) and happens when
/// the table has grown, not on a timer, so an idle server does no work at all.
const SWEEP_AT: usize = 10_000;

/// Counts attempts per address.
#[derive(Debug, Default)]
pub struct Attempts {
    seen: Mutex<HashMap<IpAddr, (Instant, u32)>>,
}

impl Attempts {
    /// Records an attempt and says whether it is allowed.
    ///
    /// A poisoned lock allows the attempt: this is a rate limiter, and failing closed would
    /// turn a panic in one request into a server nobody can sign in to.
    pub fn allow(&self, caller: IpAddr) -> bool {
        let now = Instant::now();
        let Ok(mut seen) = self.seen.lock() else {
            tracing::error!("the rate limiter's lock is poisoned; allowing the attempt");
            return true;
        };

        if seen.len() >= SWEEP_AT {
            seen.retain(|_, (started, _)| now.duration_since(*started) < WINDOW);
        }

        let entry = seen.entry(caller).or_insert((now, 0));
        if now.duration_since(entry.0) >= WINDOW {
            *entry = (now, 0);
        }
        entry.1 = entry.1.saturating_add(1);
        entry.1 <= ATTEMPTS
    }
}
