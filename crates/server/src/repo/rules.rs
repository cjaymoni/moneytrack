//! Category rules: the patterns rung 2 of the categorisation ladder tries, per user.
//!
//! The browser has no screen for writing rules by hand yet. They are learned: a person who
//! files an imported row somewhere other than where the ladder proposed leaves a rule behind
//! (`mt_core::categorise::lesson` decides which), so the next row from that merchant lands
//! where they put this one.

use deadpool_postgres::{GenericClient, Pool};
use mt_core::categorise::Lesson;
use mt_core::enums::RuleOrigin;
use mt_core::ids::{CategoryRuleId, DeviceId, UserId};
use mt_core::model::{CategoryRule, SyncMeta};
use time::OffsetDateTime;
use tokio_postgres::Row;
use uuid::Uuid;

use crate::db;
use crate::error::ApiError;
use crate::repo::rows::Reader;

/// The table, for error messages.
const ENTITY: &str = "category rule";

/// Where a learned rule sits in the order. Rules a person writes by hand, when they can,
/// will default lower and so be tried first.
const LEARNED_PRIORITY: i32 = 100;

/// The columns every read selects.
const COLUMNS: &str = "id, pattern, matched_field, category_id, priority, is_regex, enabled,
     origin, pack_id, created_at::timestamptz AS created_at,
     updated_at::timestamptz AS updated_at, deleted_at, device_id";

/// Every live rule this user has, enabled or not, in the order they are tried.
///
/// # Errors
/// [`ApiError::Internal`] if the query fails or a row will not read back.
pub async fn list(pool: &Pool, user: UserId) -> Result<Vec<CategoryRule>, ApiError> {
    let client = db::client(pool).await?;
    let rows = client
        .query(
            &format!(
                "SELECT {COLUMNS} FROM category_rules
                  WHERE user_id = $1 AND deleted_at IS NULL
                  ORDER BY priority, id"
            ),
            &[user.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("list category rules", &error))?;

    rows.iter().map(read).collect()
}

/// Records what a correction taught, on the caller's database transaction.
///
/// A learned rule for the same text in the same field is moved to the new category rather
/// than joined by a second one: a person who changes their mind about a merchant wants the
/// latest answer, not two rules that disagree and are settled by id order.
///
/// Two corrections of the same merchant committed at the same moment can still leave two
/// rules. Both name the category the person chose last in their own request, and the
/// ladder tries them in a fixed order, so that is untidy rather than wrong.
///
/// # Errors
/// [`ApiError::Internal`] if a query fails.
pub async fn learn(
    client: &impl GenericClient,
    user: UserId,
    device: DeviceId,
    lesson: &Lesson,
) -> Result<(), ApiError> {
    let now = OffsetDateTime::now_utc();
    let moved = client
        .execute(
            "UPDATE category_rules
                SET category_id = $4,
                    updated_at = greatest($5::timestamptz, created_at::timestamptz)
                                 ::ledger_instant,
                    device_id = $6
              WHERE user_id = $1 AND matched_field = $2
                AND lower(btrim(pattern)) = lower(btrim($3))
                AND origin = 'user' AND NOT is_regex AND deleted_at IS NULL",
            &[
                user.as_uuid(),
                &lesson.matched_field.as_str(),
                &lesson.pattern,
                lesson.category_id.as_uuid(),
                &now,
                device.as_uuid(),
            ],
        )
        .await
        .map_err(|error| ApiError::database("update a learned rule", &error))?;
    if moved > 0 {
        return Ok(());
    }

    let rule = CategoryRule {
        id: CategoryRuleId::from(Uuid::now_v7()),
        pattern: lesson.pattern.clone(),
        matched_field: lesson.matched_field,
        category_id: lesson.category_id,
        priority: LEARNED_PRIORITY,
        is_regex: false,
        enabled: true,
        origin: RuleOrigin::User,
        pack_id: None,
        sync: SyncMeta {
            created_at: now,
            updated_at: now,
            deleted_at: None,
            device_id: device,
        },
    };
    rule.validate().map_err(|reason| ApiError::Internal {
        operation: "learn a category rule",
        reason: reason.to_string(),
    })?;
    client
        .execute(
            "INSERT INTO category_rules (
                 user_id, id, pattern, matched_field, category_id, priority, is_regex, enabled,
                 origin, created_at, updated_at, device_id
             ) VALUES (
                 $1, $2, $3, $4, $5, $6, FALSE, TRUE, 'user',
                 $7::timestamptz::ledger_instant, $7::timestamptz::ledger_instant, $8
             )",
            &[
                user.as_uuid(),
                rule.id.as_uuid(),
                &rule.pattern,
                &rule.matched_field.as_str(),
                rule.category_id.as_uuid(),
                &rule.priority,
                &now,
                device.as_uuid(),
            ],
        )
        .await
        .map_err(|error| ApiError::database("insert a learned rule", &error))?;
    Ok(())
}

/// Reads one row.
fn read(row: &Row) -> Result<CategoryRule, ApiError> {
    let reader = Reader::new(ENTITY, row)?;
    Ok(CategoryRule {
        id: reader.id(),
        pattern: reader.text("pattern")?,
        matched_field: reader.variant("matched_field")?,
        category_id: reader.uuid("category_id")?,
        priority: reader.narrowed("priority")?,
        is_regex: reader.flag("is_regex")?,
        enabled: reader.flag("enabled")?,
        origin: reader.variant("origin")?,
        pack_id: reader.optional_uuid("pack_id")?,
        sync: SyncMeta {
            created_at: reader.instant("created_at")?,
            updated_at: reader.instant("updated_at")?,
            deleted_at: reader.optional_instant("deleted_at")?,
            device_id: reader.uuid("device_id")?,
        },
    })
}
