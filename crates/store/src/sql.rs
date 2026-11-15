//! Reading columns back into core values, and writing them out again.
//!
//! The device schema stores ids as text, instants as milliseconds, enums as their own
//! spellings and money as minor units beside a currency code — `db/sqlite/0001_init.sql`
//! says so, and `docs/SCHEMA.md` says why. This module is the only place in the crate that
//! knows those encodings, so changing one is a change here rather than a hunt.
//!
//! Every read names the row it failed on. A column that will not read back is not a
//! missing value to substitute a default for: it is a row written by something that
//! disagrees with this build about what the column means, and the only safe thing is to
//! stop and say which row and which column.

use core::str::FromStr;
use mt_core::money::{Currency, Money};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::StoreError;

/// Milliseconds in a nanosecond-denominated instant.
const NANOS_PER_MILLI: i128 = 1_000_000;

/// One row being read, with the context its errors need.
pub(crate) struct Reader<'row, 'stmt> {
    entity: &'static str,
    id: String,
    row: &'row rusqlite::Row<'stmt>,
}

impl<'row, 'stmt> Reader<'row, 'stmt> {
    /// Wraps a row, reading its `id` column first so every later failure can name it.
    ///
    /// # Errors
    /// [`StoreError::UnreadableRow`] if the row has no readable `id`.
    pub(crate) fn new(
        entity: &'static str,
        row: &'row rusqlite::Row<'stmt>,
    ) -> Result<Self, StoreError> {
        let id = row
            .get::<_, String>("id")
            .map_err(|error| StoreError::column(entity, "?", "id", error))?;
        Ok(Self { entity, id, row })
    }

    fn column<T: rusqlite::types::FromSql>(&self, column: &'static str) -> Result<T, StoreError> {
        self.row
            .get::<_, T>(column)
            .map_err(|error| StoreError::column(self.entity, &self.id, column, error))
    }

    fn fail(&self, column: &'static str, reason: impl core::fmt::Display) -> StoreError {
        StoreError::column(self.entity, &self.id, column, reason)
    }

    /// A `TEXT` column.
    pub(crate) fn text(&self, column: &'static str) -> Result<String, StoreError> {
        self.column(column)
    }

    /// A nullable `TEXT` column.
    pub(crate) fn optional_text(&self, column: &'static str) -> Result<Option<String>, StoreError> {
        self.column(column)
    }

    /// An `INTEGER` column.
    pub(crate) fn integer(&self, column: &'static str) -> Result<i64, StoreError> {
        self.column(column)
    }

    /// An `INTEGER` column narrowed to the width its field wants.
    pub(crate) fn narrowed<T: TryFrom<i64>>(&self, column: &'static str) -> Result<T, StoreError> {
        let value = self.integer(column)?;
        T::try_from(value).map_err(|_| self.fail(column, format_args!("{value} is out of range")))
    }

    /// A typed id in its canonical hyphenated form.
    pub(crate) fn id<T: From<Uuid>>(&self, column: &'static str) -> Result<T, StoreError> {
        let text = self.text(column)?;
        Uuid::parse_str(&text)
            .map(T::from)
            .map_err(|error| self.fail(column, error))
    }

    /// A nullable typed id. Absent is `None`; present and malformed is an error, never a
    /// quiet `None` — a row pointing at a category nobody can parse is a bug to surface.
    pub(crate) fn optional_id<T: From<Uuid>>(
        &self,
        column: &'static str,
    ) -> Result<Option<T>, StoreError> {
        match self.optional_text(column)? {
            Some(text) => Uuid::parse_str(&text)
                .map(|id| Some(T::from(id)))
                .map_err(|error| self.fail(column, error)),
            None => Ok(None),
        }
    }

    /// One of the closed sets the schema stores as text.
    pub(crate) fn variant<T>(&self, column: &'static str) -> Result<T, StoreError>
    where
        T: FromStr,
        T::Err: core::fmt::Display,
    {
        let text = self.text(column)?;
        T::from_str(&text).map_err(|error| self.fail(column, error))
    }

    /// An ISO 4217 code.
    pub(crate) fn currency(&self, column: &'static str) -> Result<Currency, StoreError> {
        let text = self.text(column)?;
        Currency::parse(&text).map_err(|error| self.fail(column, error))
    }

    /// Minor units in `column`, denominated in an already-read currency.
    pub(crate) fn money(
        &self,
        column: &'static str,
        currency: Currency,
    ) -> Result<Money, StoreError> {
        Ok(Money::new(self.integer(column)?, currency))
    }

    /// An instant, stored as milliseconds since the epoch.
    pub(crate) fn instant(&self, column: &'static str) -> Result<OffsetDateTime, StoreError> {
        let millis = self.integer(column)?;
        instant_from_millis(millis).map_err(|reason| self.fail(column, reason))
    }

    /// A nullable instant.
    pub(crate) fn optional_instant(
        &self,
        column: &'static str,
    ) -> Result<Option<OffsetDateTime>, StoreError> {
        match self.column::<Option<i64>>(column)? {
            Some(millis) => instant_from_millis(millis)
                .map(Some)
                .map_err(|reason| self.fail(column, reason)),
            None => Ok(None),
        }
    }
}

/// Reads milliseconds since the epoch.
fn instant_from_millis(millis: i64) -> Result<OffsetDateTime, String> {
    let nanos = i128::from(millis)
        .checked_mul(NANOS_PER_MILLI)
        .ok_or_else(|| format!("{millis} is not an instant"))?;
    OffsetDateTime::from_unix_timestamp_nanos(nanos)
        .map_err(|_| format!("{millis} is not an instant"))
}

/// Rounds an instant to the resolution the schema keeps.
///
/// Instants are stored as whole milliseconds, and a clock hands out more precision than
/// that. Without this the row a caller is handed back after a write carries microseconds
/// the database does not have: it compares unequal against the same row read a moment
/// later, and `updated_at` — which is what last-write-wins resolves conflicts on — means
/// one thing in memory and another on disk. So the rounding happens once, before the row is
/// built, and what is returned is what is stored.
///
/// Floors, like the write below, so an instant before 1970 rounds the same way as one
/// after it.
///
/// # Errors
/// As [`millis_from_instant`].
pub(crate) fn to_stored_precision(
    entity: &'static str,
    at: OffsetDateTime,
) -> Result<OffsetDateTime, StoreError> {
    let millis = millis_from_instant(entity, at)?;
    instant_from_millis(millis).map_err(|reason| StoreError::Invalid { entity, reason })
}

/// Writes an instant as milliseconds since the epoch.
///
/// Truncating division toward zero would move an instant before 1970 forward by up to a
/// millisecond; `div_euclid` floors instead, so the rounding is in one direction for every
/// instant and a round trip through the database is stable.
///
/// # Errors
/// [`StoreError::Invalid`] for an instant outside the years the column allows. Rows cannot
/// reach it — `mt-core` bounds every stored instant to 1900..2200 — so this is the
/// defensive branch for a value that never went through a row.
pub(crate) fn millis_from_instant(
    entity: &'static str,
    at: OffsetDateTime,
) -> Result<i64, StoreError> {
    i64::try_from(at.unix_timestamp_nanos().div_euclid(NANOS_PER_MILLI)).map_err(|_| {
        StoreError::Invalid {
            entity,
            reason: "an instant outside the years the schema allows".to_owned(),
        }
    })
}
