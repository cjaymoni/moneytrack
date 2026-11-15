//! Opening the device database, and what the apps hold afterwards.
//!
//! This is the one handle that crosses the FFI. Everything else is values: the apps never
//! see a connection, a statement, a cursor or a row, and `mt-core` below still has no idea
//! a database exists.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use mt_core::ids::DeviceId;
use rusqlite::Connection;
use uuid::Uuid;

use crate::change::{ChangeBus, ChangeObserver, SubscriptionId, Table};
use crate::clock::{Clock, SystemClock};
use crate::error::StoreError;
use crate::migrate;

/// Bytes of key the cipher wants: a 256-bit raw key.
///
/// Raw, not a passphrase: the key is generated once and kept in the Keychain or the Android
/// Keystore, so there is nothing to derive it from and no reason to pay for a KDF on every
/// open.
pub const KEY_LEN: usize = 32;

/// How long a statement waits for another writer before giving up, in milliseconds.
const BUSY_TIMEOUT_MS: u32 = 5_000;

/// One person's ledger, open.
///
/// Holds a single connection behind a mutex. Honest for what the device database is —
/// one writer, queries that run in microseconds against a few thousand rows — and visibly
/// the first thing to change if a screen ever waits on it. A read pool is a decision to
/// make against a profile, not in advance.
#[derive(Debug)]
pub struct Store {
    connection: Mutex<Connection>,
    device_id: DeviceId,
    clock: Arc<dyn Clock>,
    changes: ChangeBus,
}

impl Store {
    /// Opens or creates the database at `path`, encrypted under `key`.
    ///
    /// Creates the schema on a new file, and refuses a file written by a newer build. The
    /// key is a raw 256-bit key, not a passphrase; see [`KEY_LEN`].
    ///
    /// # Errors
    /// [`StoreError::KeyLength`] for a key of the wrong size, [`StoreError::Unreadable`]
    /// if the file will not decrypt under this key, [`StoreError::SchemaTooNew`] for a file
    /// from a later build, or [`StoreError::Database`] if the file will not open.
    pub fn open(path: &Path, key: &[u8]) -> Result<Self, StoreError> {
        Self::open_with(path, key, Arc::new(SystemClock))
    }

    /// As [`Store::open`], with the clock supplied. Tests hand over a fixed one.
    ///
    /// # Errors
    /// As [`Store::open`].
    pub fn open_with(path: &Path, key: &[u8], clock: Arc<dyn Clock>) -> Result<Self, StoreError> {
        if key.len() != KEY_LEN {
            return Err(StoreError::KeyLength {
                expected: KEY_LEN,
                supplied: key.len(),
            });
        }

        let mut connection = Connection::open(path)
            .map_err(|error| StoreError::database("open the database", &error))?;
        unlock(&connection, key)?;
        configure(&connection)?;
        migrate::migrate(&mut connection)?;
        let device_id = resolve_device_id(&connection)?;

        Ok(Self {
            connection: Mutex::new(connection),
            device_id,
            clock,
            changes: ChangeBus::default(),
        })
    }

    /// This installation's id, as carried on every row it writes.
    ///
    /// Minted here on first open rather than by the apps: it identifies the database, two
    /// apps would have invented it two ways, and sync reads it back out of the same row.
    #[must_use]
    pub const fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// The revision of the last committed write, or 0 before any.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.changes.revision()
    }

    /// Registers an observer, and hands back the handle that removes it.
    ///
    /// # Errors
    /// [`StoreError::LockPoisoned`].
    pub fn subscribe(
        &self,
        observer: Arc<dyn ChangeObserver>,
    ) -> Result<SubscriptionId, StoreError> {
        self.changes.subscribe(observer)
    }

    /// Removes an observer. Removing one that is already gone is not an error.
    ///
    /// # Errors
    /// [`StoreError::LockPoisoned`].
    pub fn unsubscribe(&self, id: SubscriptionId) -> Result<(), StoreError> {
        self.changes.unsubscribe(id)
    }

    /// The connection, for the repositories in this crate.
    pub(crate) fn connection(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.connection.lock().map_err(|_| StoreError::LockPoisoned)
    }

    /// The clock the store stamps rows with.
    pub(crate) fn now(&self) -> time::OffsetDateTime {
        self.clock.now()
    }

    /// Announces a committed write. Called after the commit, never inside it.
    pub(crate) fn published(&self, tables: &[Table]) -> Result<(), StoreError> {
        self.changes.publish(tables).map(|_| ())
    }
}

/// Applies the key, then proves it was the right one.
///
/// SQLCipher accepts any key: it decrypts the first page with whatever it is given and only
/// fails when the result is not a database header. So the read below is the key check, and
/// it has to be the first thing that touches a page.
fn unlock(connection: &Connection, key: &[u8]) -> Result<(), StoreError> {
    let mut hex = String::with_capacity(key.len().saturating_mul(2));
    for byte in key {
        use core::fmt::Write as _;
        // Writing to a String cannot fail; the result is checked rather than discarded
        // because the workspace denies `unwrap`, and a key that did not fully render is
        // worth refusing rather than opening a file under half of one.
        write!(hex, "{byte:02x}").map_err(|_| StoreError::KeyLength {
            expected: KEY_LEN,
            supplied: key.len(),
        })?;
    }
    // `x'...'` is SQLCipher's raw-key spelling: no KDF, the bytes are the key. The string
    // is hex this function just wrote, so there is nothing in it to escape.
    connection
        .execute_batch(&format!("PRAGMA key = \"x'{hex}'\";"))
        .map_err(|error| StoreError::database("apply the database key", &error))?;

    connection
        .query_row("SELECT count(*) FROM sqlite_schema", [], |row| {
            row.get::<_, i64>(0)
        })
        .map(|_| ())
        .map_err(|_| StoreError::Unreadable)
}

/// The pragmas that have to be set per connection rather than per database.
fn configure(connection: &Connection) -> Result<(), StoreError> {
    // Foreign keys are off by default in SQLite and the schema leans on them; WAL is what
    // lets a read run while a write is in flight; the busy timeout turns a lost race into
    // a wait rather than an error on a device that has a sync running in the background.
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|error| StoreError::database("enable foreign keys", &error))?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(|error| StoreError::database("enable write-ahead logging", &error))?;
    connection
        .busy_timeout(core::time::Duration::from_millis(u64::from(
            BUSY_TIMEOUT_MS,
        )))
        .map_err(|error| StoreError::database("set the busy timeout", &error))
}

/// Reads this installation's id, minting it on first open.
fn resolve_device_id(connection: &Connection) -> Result<DeviceId, StoreError> {
    let existing =
        match connection.query_row("SELECT device_id FROM sync_state WHERE id = 1", [], |row| {
            row.get::<_, String>(0)
        }) {
            Ok(text) => Some(text),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(other) => return Err(StoreError::database("read the device id", &other)),
        };

    match existing {
        Some(text) => DeviceId::parse(&text)
            .map_err(|error| StoreError::column("sync_state", 1, "device_id", error)),
        None => {
            let minted = DeviceId::from(Uuid::now_v7());
            connection
                .execute(
                    "INSERT INTO sync_state (id, device_id, pulled_through) VALUES (1, ?1, 0)",
                    [minted.to_string()],
                )
                .map_err(|error| StoreError::database("record the device id", &error))?;
            Ok(minted)
        }
    }
}
