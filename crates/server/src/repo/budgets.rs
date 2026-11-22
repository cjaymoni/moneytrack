//! Budgets: caps on spending, per category and currency. Every query is scoped to one user.
//!
//! Evaluating a budget is `mt-core`'s. What is here is keeping them, and one decision about
//! changing one that the schema's own comment asks for: a new cap applies from the period it
//! is changed in, not retroactively, so last month's verdict does not move because this
//! month's cap did.

use deadpool_postgres::Pool;
use mt_core::budget::period_containing;
use mt_core::enums::BudgetPeriod;
use mt_core::ids::{BudgetId, CategoryId, DeviceId, UserId};
use mt_core::model::{Budget, SyncMeta};
use mt_core::money::Money;
use time::{Date, OffsetDateTime};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::db;
use crate::error::ApiError;
use crate::repo::categories;
use crate::repo::rows::Reader;

/// The table, for error messages.
const ENTITY: &str = "budget";

/// The columns every read selects.
const COLUMNS: &str = "id, category_id, period, cap_minor::bigint AS cap_minor,
     currency::text AS currency, starts_on, ends_on, warn_at_bps::integer AS warn_at_bps,
     created_at::timestamptz AS created_at, updated_at::timestamptz AS updated_at,
     deleted_at, device_id";

/// What a caller supplies to set a budget.
#[derive(Debug, Clone)]
pub struct NewBudget {
    /// The category capped, or `None` for all spending in the currency.
    pub category_id: Option<CategoryId>,
    /// Weekly, monthly or yearly.
    pub period: BudgetPeriod,
    /// The cap, in the budget's currency.
    pub cap: Money,
    /// The first day of the first period.
    pub starts_on: Date,
    /// Basis points of the cap at which to warn.
    pub warn_at_bps: u16,
}

/// Sets a budget.
///
/// # Errors
/// [`ApiError::NotFound`] if the category is not this user's, [`ApiError::Invalid`] for a
/// row the model refuses, [`ApiError::Conflict`] if a live budget already covers the same
/// scope over overlapping dates, or [`ApiError::Internal`].
pub async fn create(
    pool: &Pool,
    user: UserId,
    device: DeviceId,
    new: &NewBudget,
) -> Result<Budget, ApiError> {
    if let Some(category) = new.category_id {
        categories::get(pool, user, category).await?;
    }
    let now = OffsetDateTime::now_utc();
    let budget = Budget {
        id: BudgetId::from(Uuid::now_v7()),
        category_id: new.category_id,
        period: new.period,
        cap: new.cap,
        starts_on: new.starts_on,
        ends_on: None,
        warn_at_bps: new.warn_at_bps,
        sync: SyncMeta {
            created_at: now,
            updated_at: now,
            deleted_at: None,
            device_id: device,
        },
    };
    check(&budget)?;
    let client = db::client(pool).await?;
    insert(&client, user, &budget).await
}

/// Every live budget of this user's, in the order they were set.
///
/// # Errors
/// [`ApiError::Internal`] if the query fails or a row will not read back.
pub async fn list(pool: &Pool, user: UserId) -> Result<Vec<Budget>, ApiError> {
    let client = db::client(pool).await?;
    let rows = client
        .query(
            &format!(
                "SELECT {COLUMNS} FROM budgets
                  WHERE user_id = $1 AND deleted_at IS NULL
                  ORDER BY created_at, id"
            ),
            &[user.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("list budgets", &error))?;
    rows.iter().map(read).collect()
}

/// One live budget of this user's.
///
/// # Errors
/// [`ApiError::NotFound`] if there is no such budget or it belongs to someone else.
pub async fn get(pool: &Pool, user: UserId, id: BudgetId) -> Result<Budget, ApiError> {
    let client = db::client(pool).await?;
    let found = client
        .query_opt(
            &format!(
                "SELECT {COLUMNS} FROM budgets
                  WHERE user_id = $1 AND id = $2 AND deleted_at IS NULL"
            ),
            &[user.as_uuid(), id.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("read a budget", &error))?;
    match found {
        Some(row) => read(&row),
        None => Err(ApiError::NotFound { entity: ENTITY }),
    }
}

/// Changes a budget's cap and warning threshold, from the period `today` falls in onward.
///
/// If `today` is in the budget's first period, there is no history to protect and the row
/// is changed in place. Otherwise the budget is ended the day before the current period
/// began and a new one carries on from that day with the new cap — the split the schema's
/// exclusion constraint is shaped for — so every past period keeps the cap it was measured
/// against. Both writes are one transaction.
///
/// Returns the budget now in force.
///
/// # Errors
/// [`ApiError::NotFound`] if there is no such budget of this user's, [`ApiError::Invalid`]
/// if it is not running on `today` or the new values are refused, or [`ApiError::Internal`].
pub async fn change(
    pool: &Pool,
    user: UserId,
    device: DeviceId,
    id: BudgetId,
    cap: Money,
    warn_at_bps: u16,
    today: Date,
) -> Result<Budget, ApiError> {
    let existing = get(pool, user, id).await?;
    if cap.currency() != existing.cap.currency() {
        return Err(ApiError::Invalid {
            field: "cap".to_owned(),
            reason: format!(
                "it is in {} and the budget is in {}",
                cap.currency().code(),
                existing.cap.currency().code()
            ),
        });
    }
    let Some(current) = period_containing(&existing, today) else {
        return Err(ApiError::Invalid {
            field: "budget".to_owned(),
            reason: "it is not running today, so there is no period to change".to_owned(),
        });
    };

    let now = OffsetDateTime::now_utc();
    let mut client = db::client(pool).await?;
    let transaction = client
        .transaction()
        .await
        .map_err(|error| ApiError::database("begin changing a budget", &error))?;

    let changed = if current.starts_on <= existing.starts_on {
        let changed = Budget {
            cap,
            warn_at_bps,
            sync: SyncMeta {
                updated_at: now.max(existing.sync.created_at),
                device_id: device,
                ..existing.sync
            },
            ..existing
        };
        check(&changed)?;
        let row = transaction
            .query_one(
                &format!(
                    "UPDATE budgets
                        SET cap_minor = $3::bigint::amount_minor,
                            warn_at_bps = $4::integer::basis_points,
                            updated_at = $5::timestamptz::ledger_instant,
                            device_id = $6
                      WHERE user_id = $1 AND id = $2 AND deleted_at IS NULL
                      RETURNING {COLUMNS}"
                ),
                &[
                    user.as_uuid(),
                    id.as_uuid(),
                    &cap.minor(),
                    &i32::from(warn_at_bps),
                    &changed.sync.updated_at,
                    device.as_uuid(),
                ],
            )
            .await
            .map_err(|error| ApiError::database("change a budget", &error))?;
        read(&row)?
    } else {
        let ended_on = current
            .starts_on
            .previous_day()
            .ok_or_else(|| ApiError::Internal {
                operation: "change a budget",
                reason: "the current period starts on the first day of the calendar".to_owned(),
            })?;
        let successor = Budget {
            id: BudgetId::from(Uuid::now_v7()),
            cap,
            warn_at_bps,
            starts_on: current.starts_on,
            sync: SyncMeta {
                created_at: now,
                updated_at: now,
                deleted_at: None,
                device_id: device,
            },
            ..existing.clone()
        };
        check(&successor)?;
        transaction
            .execute(
                "UPDATE budgets
                    SET ends_on = $3,
                        updated_at = greatest($4::timestamptz, created_at::timestamptz)
                                     ::ledger_instant,
                        device_id = $5
                  WHERE user_id = $1 AND id = $2 AND deleted_at IS NULL",
                &[
                    user.as_uuid(),
                    id.as_uuid(),
                    &ended_on,
                    &now,
                    device.as_uuid(),
                ],
            )
            .await
            .map_err(|error| ApiError::database("end a budget", &error))?;
        insert(&transaction, user, &successor).await?
    };

    transaction
        .commit()
        .await
        .map_err(|error| ApiError::database("commit a budget change", &error))?;
    Ok(changed)
}

/// Removes a budget, leaving a tombstone for sync.
///
/// # Errors
/// [`ApiError::NotFound`] if there is no such live budget of this user's.
pub async fn delete(
    pool: &Pool,
    user: UserId,
    device: DeviceId,
    id: BudgetId,
) -> Result<(), ApiError> {
    let now = OffsetDateTime::now_utc();
    let client = db::client(pool).await?;
    let changed = client
        .execute(
            "UPDATE budgets
                SET deleted_at = greatest($3::timestamptz, created_at::timestamptz),
                    updated_at = greatest($3::timestamptz, created_at::timestamptz)
                                 ::ledger_instant,
                    device_id = $4
              WHERE user_id = $1 AND id = $2 AND deleted_at IS NULL",
            &[user.as_uuid(), id.as_uuid(), &now, device.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("delete a budget", &error))?;
    if changed == 0 {
        return Err(ApiError::NotFound { entity: ENTITY });
    }
    Ok(())
}

/// The model's checks, reported as a refusal naming the field.
fn check(budget: &Budget) -> Result<(), ApiError> {
    budget.validate().map_err(|reason| ApiError::Invalid {
        field: "budget".to_owned(),
        reason: reason.to_string(),
    })
}

/// Writes a budget, turning an overlap into a refusal a person can act on.
async fn insert(
    client: &impl deadpool_postgres::GenericClient,
    user: UserId,
    budget: &Budget,
) -> Result<Budget, ApiError> {
    let category = budget.category_id.map(|id| *id.as_uuid());
    let written = client
        .query_one(
            &format!(
                "INSERT INTO budgets (
                     user_id, id, category_id, period, cap_minor, currency, starts_on, ends_on,
                     warn_at_bps, created_at, updated_at, device_id
                 ) VALUES (
                     $1, $2, $3, $4, $5::bigint::amount_minor, $6::text::currency_code, $7, $8,
                     $9::integer::basis_points,
                     $10::timestamptz::ledger_instant, $10::timestamptz::ledger_instant, $11
                 )
                 RETURNING {COLUMNS}"
            ),
            &[
                user.as_uuid(),
                budget.id.as_uuid(),
                &category,
                &budget.period.as_str(),
                &budget.cap.minor(),
                &budget.cap.currency().code(),
                &budget.starts_on,
                &budget.ends_on,
                &i32::from(budget.warn_at_bps),
                &budget.sync.created_at,
                budget.sync.device_id.as_uuid(),
            ],
        )
        .await;
    match written {
        Ok(row) => read(&row),
        // exclusion_violation: the schema's "one answer to am I over budget" constraint.
        Err(error)
            if error.code() == Some(&tokio_postgres::error::SqlState::EXCLUSION_VIOLATION) =>
        {
            Err(ApiError::Conflict {
                reason: format!(
                    "there is already a {} {} budget for this over those dates",
                    budget.period.as_str(),
                    budget.cap.currency().code()
                ),
            })
        }
        Err(error) => Err(ApiError::database("insert a budget", &error)),
    }
}

/// Reads one row.
fn read(row: &Row) -> Result<Budget, ApiError> {
    let reader = Reader::new(ENTITY, row)?;
    let currency = reader.currency("currency")?;
    Ok(Budget {
        id: reader.id(),
        category_id: reader.optional_uuid("category_id")?,
        period: reader.variant("period")?,
        cap: reader.money("cap_minor", currency)?,
        starts_on: reader.date("starts_on")?,
        ends_on: reader.optional_date("ends_on")?,
        warn_at_bps: reader.narrowed("warn_at_bps")?,
        sync: SyncMeta {
            created_at: reader.instant("created_at")?,
            updated_at: reader.instant("updated_at")?,
            deleted_at: reader.optional_instant("deleted_at")?,
            device_id: reader.uuid("device_id")?,
        },
    })
}
