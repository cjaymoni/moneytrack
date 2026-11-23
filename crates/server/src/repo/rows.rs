//! Reading Postgres columns back into core values.
//!
//! The schema keeps money as `BIGINT` minor units beside a currency code, instants as
//! `TIMESTAMPTZ`, ids as `UUID` and the closed sets as text in their own spellings. This is
//! the only place in the crate that knows that, so changing an encoding is a change here
//! rather than a hunt.
//!
//! Every read names the row it failed on. A column that will not read back is a row written
//! by something that disagrees with this build about what the column means, and the only
//! safe thing is to stop and say which row and which column.

use core::str::FromStr;
use mt_core::money::{Currency, Money};
use time::OffsetDateTime;
use tokio_postgres::Row;
use uuid::Uuid;

use crate::error::ApiError;

/// One row being read, with the context its errors need.
pub(crate) struct Reader<'a> {
    entity: &'static str,
    id: Uuid,
    row: &'a Row,
}

impl<'a> Reader<'a> {
    /// Wraps a row, reading its `id` first so every later failure can name it.
    pub(crate) fn new(entity: &'static str, row: &'a Row) -> Result<Self, ApiError> {
        let id: Uuid = row
            .try_get("id")
            .map_err(|error| fail(entity, Uuid::nil(), "id", &error))?;
        Ok(Self { entity, id, row })
    }

    fn column<T>(&self, column: &'static str) -> Result<T, ApiError>
    where
        T: for<'r> tokio_postgres::types::FromSql<'r>,
    {
        self.row
            .try_get::<_, T>(column)
            .map_err(|error| fail(self.entity, self.id, column, &error))
    }

    /// The row's own id, read when the reader was built — precisely so that a failure on
    /// any later column can name the row it came from.
    pub(crate) fn id<T: From<Uuid>>(&self) -> T {
        T::from(self.id)
    }

    /// A `TEXT` column.
    pub(crate) fn text(&self, column: &'static str) -> Result<String, ApiError> {
        self.column(column)
    }

    /// A nullable `TEXT` column.
    pub(crate) fn optional_text(&self, column: &'static str) -> Result<Option<String>, ApiError> {
        self.column(column)
    }

    /// A `BOOLEAN` column.
    pub(crate) fn flag(&self, column: &'static str) -> Result<bool, ApiError> {
        self.column(column)
    }

    /// An `INTEGER` column narrowed to the width its field wants.
    pub(crate) fn narrowed<T: TryFrom<i32>>(&self, column: &'static str) -> Result<T, ApiError> {
        let value: i32 = self.column(column)?;
        T::try_from(value).map_err(|_| ApiError::Internal {
            operation: "read a row",
            reason: format!("{}.{column} is out of range: {value}", self.entity),
        })
    }

    /// A typed id.
    pub(crate) fn uuid<T: From<Uuid>>(&self, column: &'static str) -> Result<T, ApiError> {
        self.column::<Uuid>(column).map(T::from)
    }

    /// A nullable typed id. Absent is `None`; present and malformed is an error, never a
    /// quiet `None`.
    pub(crate) fn optional_uuid<T: From<Uuid>>(
        &self,
        column: &'static str,
    ) -> Result<Option<T>, ApiError> {
        Ok(self.column::<Option<Uuid>>(column)?.map(T::from))
    }

    /// One of the closed sets the schema stores as text.
    pub(crate) fn variant<T>(&self, column: &'static str) -> Result<T, ApiError>
    where
        T: FromStr,
        T::Err: core::fmt::Display,
    {
        let text = self.text(column)?;
        T::from_str(&text).map_err(|error| ApiError::Internal {
            operation: "read a row",
            reason: format!("{}.{column} does not read back: {error}", self.entity),
        })
    }

    /// An ISO 4217 code.
    pub(crate) fn currency(&self, column: &'static str) -> Result<Currency, ApiError> {
        let text = self.text(column)?;
        Currency::parse(text.trim()).map_err(|error| ApiError::Internal {
            operation: "read a row",
            reason: format!("{}.{column} does not read back: {error}", self.entity),
        })
    }

    /// Minor units in `column`, denominated in an already-read currency.
    pub(crate) fn money(
        &self,
        column: &'static str,
        currency: Currency,
    ) -> Result<Money, ApiError> {
        Ok(Money::new(self.column::<i64>(column)?, currency))
    }

    /// An instant.
    pub(crate) fn instant(&self, column: &'static str) -> Result<OffsetDateTime, ApiError> {
        self.column(column)
    }

    /// A `DATE` column.
    pub(crate) fn date(&self, column: &'static str) -> Result<time::Date, ApiError> {
        self.column(column)
    }

    /// A nullable `DATE` column.
    pub(crate) fn optional_date(
        &self,
        column: &'static str,
    ) -> Result<Option<time::Date>, ApiError> {
        self.column(column)
    }

    /// A nullable instant.
    pub(crate) fn optional_instant(
        &self,
        column: &'static str,
    ) -> Result<Option<OffsetDateTime>, ApiError> {
        self.column(column)
    }
}

/// A column that will not read back.
fn fail(entity: &'static str, id: Uuid, column: &str, error: &tokio_postgres::Error) -> ApiError {
    ApiError::Internal {
        operation: "read a row",
        reason: format!("{entity} {id}: column {column} does not read back: {error}"),
    }
}
