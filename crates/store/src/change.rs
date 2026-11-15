//! The change bus: what replaces drift's live queries.
//!
//! Under Flutter the inbox and the running balance were going to lean on drift's reactive
//! queries, which re-run themselves when a table they read changes. Two native UIs have no
//! such thing, and writing one twice is how the two apps start disagreeing.
//!
//! So invalidation is explicit and lives here. Every write publishes a revision and the
//! tables it touched; a subscriber re-runs the queries that care. Re-querying local SQLite
//! is cheap, and the alternative — each screen guessing when its data went stale — is the
//! bug that shows a stale balance after a transfer.
//!
//! Three properties the subscribers can rely on. A change is published **after** the write
//! has committed, so a query run in response sees it. Revisions are strictly increasing, so
//! a subscriber that misses one can tell. And observers are called with no database lock
//! held, so an observer may call straight back into the store without deadlocking itself.

use core::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::error::StoreError;

/// A table a write touched.
///
/// Coarser than a row id on purpose: a screen reads a query, not a row, and "transactions
/// changed, re-run your query" is both true and cheap. Finer-grained invalidation would be
/// a performance decision to make against a profile, not in advance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Table {
    /// `accounts`.
    Accounts,
    /// `transactions`.
    Transactions,
    /// `categories`.
    Categories,
    /// `category_rules`.
    CategoryRules,
    /// `recurring`.
    Recurring,
    /// `budgets`.
    Budgets,
    /// `inbox_items`.
    InboxItems,
}

/// One committed write, as a subscriber sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// Strictly increasing per store. A gap means a change was published while the
    /// subscriber was not listening, not that one was lost.
    pub revision: u64,
    /// The tables the write touched, in a stable order and without repeats.
    pub tables: Vec<Table>,
}

/// Someone listening for writes.
///
/// Implemented by the apps — by an `AsyncStream` on the Swift side and a `Flow` on the
/// Kotlin one. The call is made on whichever thread committed the write, so an
/// implementation does the smallest possible thing and hands off.
pub trait ChangeObserver: Send + Sync {
    /// A write has committed.
    fn changed(&self, change: &Change);
}

/// The handle `unsubscribe` takes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SubscriptionId(u64);

impl SubscriptionId {
    /// The handle as a plain number, which is how it crosses the FFI.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Reads a handle back from the number that crossed.
    ///
    /// Unchecked on purpose: a number that was never issued, or one issued by another
    /// store, simply matches no observer, and [`Store::unsubscribe`] treats that as the
    /// no-op it is. Validating it would mean keeping a list of handles that have been
    /// handed out, for no gain.
    ///
    /// [`Store::unsubscribe`]: crate::Store::unsubscribe
    #[must_use]
    pub const fn from_u64(id: u64) -> Self {
        Self(id)
    }
}

/// The revision counter and the observer list.
#[derive(Default)]
pub(crate) struct ChangeBus {
    revision: AtomicU64,
    next_subscription: AtomicU64,
    observers: Mutex<Vec<(SubscriptionId, Arc<dyn ChangeObserver>)>>,
}

impl core::fmt::Debug for ChangeBus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ChangeBus")
            .field("revision", &self.revision.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl ChangeBus {
    /// The revision of the last published change, or 0 before any.
    pub(crate) fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    /// Adds an observer and returns the handle that removes it again.
    ///
    /// # Errors
    /// [`StoreError::LockPoisoned`] if a thread failed while holding the observer list.
    pub(crate) fn subscribe(
        &self,
        observer: Arc<dyn ChangeObserver>,
    ) -> Result<SubscriptionId, StoreError> {
        let id = SubscriptionId(self.next_subscription.fetch_add(1, Ordering::Relaxed));
        let mut observers = self
            .observers
            .lock()
            .map_err(|_| StoreError::LockPoisoned)?;
        observers.push((id, observer));
        Ok(id)
    }

    /// Removes an observer. Removing one twice, or one that was never added, is not an
    /// error: a screen tearing down twice is ordinary, and failing it would make callers
    /// track state the bus already knows.
    ///
    /// # Errors
    /// [`StoreError::LockPoisoned`] as [`ChangeBus::subscribe`].
    pub(crate) fn unsubscribe(&self, id: SubscriptionId) -> Result<(), StoreError> {
        let mut observers = self
            .observers
            .lock()
            .map_err(|_| StoreError::LockPoisoned)?;
        observers.retain(|(held, _)| *held != id);
        Ok(())
    }

    /// Publishes a committed write.
    ///
    /// The observer list is copied out and the lock released before anything is called, so
    /// an observer that writes to the store in response does not deadlock against the next
    /// subscriber.
    ///
    /// # Errors
    /// [`StoreError::LockPoisoned`] as [`ChangeBus::subscribe`].
    pub(crate) fn publish(&self, tables: &[Table]) -> Result<u64, StoreError> {
        let mut tables = tables.to_vec();
        tables.sort_unstable();
        tables.dedup();

        let revision = self
            .revision
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        let change = Change { revision, tables };

        let listeners = {
            let observers = self
                .observers
                .lock()
                .map_err(|_| StoreError::LockPoisoned)?;
            observers
                .iter()
                .map(|(_, observer)| Arc::clone(observer))
                .collect::<Vec<_>>()
        };
        for listener in listeners {
            listener.changed(&change);
        }
        Ok(revision)
    }
}
