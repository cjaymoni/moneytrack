//! Categories: what a row was for. Every query is scoped to one user.
//!
//! Nesting is one level, and the rule lives in `mt-core` (`Category::validate_under`): the
//! schema can refuse a category that is its own parent, but not a grandparent chain, so the
//! parent is read here first and handed to the core to judge.

use deadpool_postgres::{GenericClient, Pool};
use mt_core::enums::CategoryKind;
use mt_core::ids::{CategoryId, DeviceId, UserId};
use mt_core::model::{Category, SyncMeta};
use time::OffsetDateTime;
use tokio_postgres::Row;
use uuid::Uuid;

use crate::db;
use crate::error::ApiError;
use crate::repo::rows::Reader;

/// The table, for error messages.
const ENTITY: &str = "category";

/// The columns every read selects.
const COLUMNS: &str = "id, name, parent_id, kind, icon, is_system, sort_order,
     created_at::timestamptz AS created_at, updated_at::timestamptz AS updated_at,
     deleted_at, device_id";

/// The categories a new ledger starts with.
///
/// Written for the person the product is built for first — a mobile-money wallet in Accra —
/// so "Airtime & data" and "MoMo & bank fees" are here and a mortgage is not. They are
/// ordinary rows: renamed, deleted or added to like any other. They are not `is_system`,
/// which means "shipped by a rule pack and not deletable"; a pack can claim that later.
///
/// Flat on purpose. One level of nesting is allowed, but a new ledger with subcategories
/// asks someone to understand a tree before they have recorded a single cedi.
pub const STARTERS: &[(CategoryKind, &str)] = &[
    (CategoryKind::Expense, "Food & groceries"),
    (CategoryKind::Expense, "Eating out"),
    (CategoryKind::Expense, "Transport"),
    (CategoryKind::Expense, "Airtime & data"),
    (CategoryKind::Expense, "Utilities"),
    (CategoryKind::Expense, "Rent & housing"),
    (CategoryKind::Expense, "Health"),
    (CategoryKind::Expense, "School fees"),
    (CategoryKind::Expense, "Family & support"),
    (CategoryKind::Expense, "Giving"),
    (CategoryKind::Expense, "Clothing"),
    (CategoryKind::Expense, "MoMo & bank fees"),
    (CategoryKind::Expense, "Other spending"),
    (CategoryKind::Income, "Salary"),
    (CategoryKind::Income, "Business"),
    (CategoryKind::Income, "Gifts & support received"),
    (CategoryKind::Income, "Refunds"),
    (CategoryKind::Income, "Other income"),
];

/// What a caller supplies to add a category.
#[derive(Debug, Clone)]
pub struct NewCategory {
    /// What to call it.
    pub name: String,
    /// Income or expense.
    pub kind: CategoryKind,
    /// The category it sits under, if any. Must be a root of the same kind.
    pub parent_id: Option<CategoryId>,
}

/// Gives a new ledger its starting categories, all at once or not at all.
///
/// Does nothing if the person already has any, so calling it twice cannot double the list.
///
/// # Errors
/// [`ApiError::Internal`] if the inserts fail; none of them is kept if one does.
pub async fn seed(pool: &Pool, user: UserId, device: DeviceId) -> Result<(), ApiError> {
    let mut client = db::client(pool).await?;
    let transaction = client
        .transaction()
        .await
        .map_err(|error| ApiError::database("begin seeding categories", &error))?;

    let existing = transaction
        .query_one(
            "SELECT count(*) AS n FROM categories WHERE user_id = $1",
            &[user.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("count categories", &error))?;
    let count: i64 = existing
        .try_get("n")
        .map_err(|error| ApiError::database("count categories", &error))?;
    if count > 0 {
        return Ok(());
    }

    let now = OffsetDateTime::now_utc();
    for (order, (kind, name)) in STARTERS.iter().enumerate() {
        let order = i32::try_from(order).map_err(|_| ApiError::Internal {
            operation: "seed categories",
            reason: "too many starters".to_owned(),
        })?;
        transaction
            .execute(
                "INSERT INTO categories (
                     user_id, id, name, kind, sort_order, created_at, updated_at, device_id
                 ) VALUES (
                     $1, $2, $3, $4, $5,
                     $6::timestamptz::ledger_instant, $6::timestamptz::ledger_instant, $7
                 )",
                &[
                    user.as_uuid(),
                    &Uuid::now_v7(),
                    name,
                    &kind.as_str(),
                    &order,
                    &now,
                    device.as_uuid(),
                ],
            )
            .await
            .map_err(|error| ApiError::database("seed a category", &error))?;
    }

    transaction
        .commit()
        .await
        .map_err(|error| ApiError::database("commit seeded categories", &error))
}

/// Adds a category for this user.
///
/// # Errors
/// [`ApiError::NotFound`] if the parent is not this user's, [`ApiError::Invalid`] if the row
/// or its place in the tree is refused by the model, or [`ApiError::Internal`].
pub async fn create(
    pool: &Pool,
    user: UserId,
    device: DeviceId,
    new: &NewCategory,
) -> Result<Category, ApiError> {
    let parent = match new.parent_id {
        Some(id) => Some(get(pool, user, id).await?),
        None => None,
    };
    if let Some(parent) = &parent {
        // A subcategory of the other kind would put spending under an income heading, and
        // the breakdown would count it on the wrong side.
        if parent.kind != new.kind {
            return Err(ApiError::Invalid {
                field: "parent_id".to_owned(),
                reason: format!(
                    "it is an {} category and this one is {}",
                    parent.kind.as_str(),
                    new.kind.as_str()
                ),
            });
        }
    }

    let now = OffsetDateTime::now_utc();
    let category = Category {
        id: CategoryId::from(Uuid::now_v7()),
        name: new.name.trim().to_owned(),
        parent_id: new.parent_id,
        kind: new.kind,
        icon: None,
        is_system: false,
        sort_order: 0,
        sync: SyncMeta {
            created_at: now,
            updated_at: now,
            deleted_at: None,
            device_id: device,
        },
    };
    category
        .validate()
        .and_then(|()| category.validate_under(parent.as_ref()))
        .map_err(|reason| ApiError::Invalid {
            field: "category".to_owned(),
            reason: reason.to_string(),
        })?;

    let parent_id = category.parent_id.map(|id| *id.as_uuid());
    let client = db::client(pool).await?;
    let row = client
        .query_one(
            &format!(
                "INSERT INTO categories (
                     user_id, id, name, parent_id, kind, sort_order,
                     created_at, updated_at, device_id
                 ) VALUES (
                     $1, $2, $3, $4, $5,
                     (SELECT coalesce(max(sort_order) + 1, 0) FROM categories WHERE user_id = $1),
                     $6::timestamptz::ledger_instant, $6::timestamptz::ledger_instant, $7
                 )
                 RETURNING {COLUMNS}"
            ),
            &[
                user.as_uuid(),
                category.id.as_uuid(),
                &category.name,
                &parent_id,
                &category.kind.as_str(),
                &now,
                device.as_uuid(),
            ],
        )
        .await
        .map_err(|error| ApiError::database("insert a category", &error))?;

    read(&row)
}

/// Every category this user has that is not deleted, in display order.
///
/// # Errors
/// [`ApiError::Internal`] if the query fails or a row will not read back.
pub async fn list(pool: &Pool, user: UserId) -> Result<Vec<Category>, ApiError> {
    let client = db::client(pool).await?;
    let rows = client
        .query(
            &format!(
                "SELECT {COLUMNS} FROM categories
                  WHERE user_id = $1 AND deleted_at IS NULL
                  ORDER BY sort_order, id"
            ),
            &[user.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("list categories", &error))?;

    rows.iter().map(read).collect()
}

/// One category of this user's.
///
/// # Errors
/// [`ApiError::NotFound`] if there is no such category or it belongs to someone else.
pub async fn get(pool: &Pool, user: UserId, id: CategoryId) -> Result<Category, ApiError> {
    let client = db::client(pool).await?;
    let found = client
        .query_opt(
            &format!(
                "SELECT {COLUMNS} FROM categories
                  WHERE user_id = $1 AND id = $2 AND deleted_at IS NULL"
            ),
            &[user.as_uuid(), id.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("read a category", &error))?;

    match found {
        Some(row) => read(&row),
        None => Err(ApiError::NotFound { entity: ENTITY }),
    }
}

/// Renames a category.
///
/// # Errors
/// [`ApiError::NotFound`] if the category is not this user's, [`ApiError::Invalid`] for a
/// name the model refuses, or [`ApiError::Internal`].
pub async fn rename(
    pool: &Pool,
    user: UserId,
    device: DeviceId,
    id: CategoryId,
    name: &str,
) -> Result<Category, ApiError> {
    let mut renamed = get(pool, user, id).await?;
    renamed.name = name.trim().to_owned();
    renamed.validate().map_err(|reason| ApiError::Invalid {
        field: "name".to_owned(),
        reason: reason.to_string(),
    })?;

    let now = OffsetDateTime::now_utc();
    let client = db::client(pool).await?;
    let row = client
        .query_one(
            &format!(
                "UPDATE categories
                    SET name = $3,
                        updated_at = greatest($4::timestamptz, created_at::timestamptz)
                                     ::ledger_instant,
                        device_id = $5
                  WHERE user_id = $1 AND id = $2 AND deleted_at IS NULL
                  RETURNING {COLUMNS}"
            ),
            &[
                user.as_uuid(),
                id.as_uuid(),
                &renamed.name,
                &now,
                device.as_uuid(),
            ],
        )
        .await
        .map_err(|error| ApiError::database("rename a category", &error))?;
    read(&row)
}

/// Deletes a category, moving what was filed under it to `move_to`, or to no category.
///
/// Everything that names the category moves in the same database transaction: its
/// transactions, the regular payments that write into it, the rules that file into it, and
/// any waiting statement line it was proposed for. A rule with nowhere to go is deleted with
/// it, so the next statement is not filed under a category that no longer exists.
///
/// A category with subcategories, or with a budget, is refused rather than guessed about:
/// the person moves or removes those first.
///
/// # Errors
/// [`ApiError::NotFound`] if either category is not this user's, [`ApiError::Invalid`] for
/// a `move_to` that is the category itself or of the other kind, [`ApiError::Conflict`] for
/// one with subcategories or a budget, or [`ApiError::Internal`].
pub async fn delete(
    pool: &Pool,
    user: UserId,
    device: DeviceId,
    id: CategoryId,
    move_to: Option<CategoryId>,
) -> Result<(), ApiError> {
    let mut client = db::client(pool).await?;
    let transaction = client
        .transaction()
        .await
        .map_err(|error| ApiError::database("begin deleting a category", &error))?;

    // Both locked in one statement, in id order, so two deletes moving into each other
    // wait for one another rather than deadlock.
    let wanted: Vec<CategoryId> = std::iter::once(id).chain(move_to).collect();
    let locked = lock(&transaction, user, &wanted).await?;
    let find = |wanted: CategoryId| {
        locked
            .iter()
            .find(|category| category.id == wanted)
            .ok_or(ApiError::NotFound { entity: ENTITY })
    };
    let doomed = find(id)?;
    if let Some(target) = move_to {
        if target == id {
            return Err(ApiError::Invalid {
                field: "move_to".to_owned(),
                reason: "it is the category being deleted".to_owned(),
            });
        }
        let target = find(target)?;
        if target.kind != doomed.kind {
            return Err(ApiError::Invalid {
                field: "move_to".to_owned(),
                reason: format!(
                    "it is an {} category and this one is {}",
                    target.kind.as_str(),
                    doomed.kind.as_str()
                ),
            });
        }
    }
    let count = |sql: &'static str| {
        let transaction = &transaction;
        async move {
            transaction
                .query_one(sql, &[user.as_uuid(), id.as_uuid()])
                .await
                .and_then(|row| row.try_get::<_, i64>(0))
                .map_err(|error| ApiError::database("check what a category holds", &error))
        }
    };
    if count(
        "SELECT count(*) FROM categories
          WHERE user_id = $1 AND parent_id = $2 AND deleted_at IS NULL",
    )
    .await?
        > 0
    {
        return Err(ApiError::Conflict {
            reason: "it has subcategories; move or delete those first".to_owned(),
        });
    }
    if count(
        "SELECT count(*) FROM budgets
          WHERE user_id = $1 AND category_id = $2 AND deleted_at IS NULL",
    )
    .await?
        > 0
    {
        return Err(ApiError::Conflict {
            reason: "it has a budget; remove the budget first".to_owned(),
        });
    }

    let now = OffsetDateTime::now_utc();
    let target = move_to.map(|target| *target.as_uuid());
    let moves: [(&str, &'static str); 3] = [
        (
            "move its transactions",
            "UPDATE transactions
                SET category_id = $3,
                    updated_at = greatest($4::timestamptz, created_at::timestamptz)::ledger_instant,
                    device_id = $5
              WHERE user_id = $1 AND category_id = $2 AND deleted_at IS NULL",
        ),
        (
            "move its regular payments",
            "UPDATE recurring
                SET category_id = $3,
                    updated_at = greatest($4::timestamptz, created_at::timestamptz)::ledger_instant,
                    device_id = $5
              WHERE user_id = $1 AND category_id = $2 AND deleted_at IS NULL",
        ),
        (
            "move its rules",
            "UPDATE category_rules
                SET category_id = coalesce($3, category_id),
                    deleted_at = CASE WHEN $3::uuid IS NULL THEN $4::timestamptz END,
                    updated_at = greatest($4::timestamptz, created_at::timestamptz)::ledger_instant,
                    device_id = $5
              WHERE user_id = $1 AND category_id = $2 AND deleted_at IS NULL",
        ),
    ];
    for (operation, sql) in moves {
        transaction
            .execute(
                sql,
                &[
                    user.as_uuid(),
                    id.as_uuid(),
                    &target,
                    &now,
                    device.as_uuid(),
                ],
            )
            .await
            .map_err(|error| ApiError::database(operation, &error))?;
    }
    // A waiting statement line carries only a proposal; it has no device or sync columns.
    transaction
        .execute(
            "UPDATE inbox_items
                SET category_id = $3,
                    updated_at = greatest($4::timestamptz, created_at::timestamptz)::ledger_instant
              WHERE user_id = $1 AND category_id = $2 AND status = 'pending'",
            &[user.as_uuid(), id.as_uuid(), &target, &now],
        )
        .await
        .map_err(|error| ApiError::database("move its waiting lines", &error))?;
    transaction
        .execute(
            "UPDATE categories
                SET deleted_at = $3,
                    updated_at = greatest($3::timestamptz, created_at::timestamptz)::ledger_instant,
                    device_id = $4
              WHERE user_id = $1 AND id = $2",
            &[user.as_uuid(), id.as_uuid(), &now, device.as_uuid()],
        )
        .await
        .map_err(|error| ApiError::database("delete a category", &error))?;

    transaction
        .commit()
        .await
        .map_err(|error| ApiError::database("commit deleting a category", &error))
}

/// The live categories of this user's among `ids`, locked for the rest of the transaction
/// in id order. One that is missing is simply not returned.
async fn lock(
    client: &impl GenericClient,
    user: UserId,
    ids: &[CategoryId],
) -> Result<Vec<Category>, ApiError> {
    let ids: Vec<Uuid> = ids.iter().map(|id| *id.as_uuid()).collect();
    let rows = client
        .query(
            &format!(
                "SELECT {COLUMNS} FROM categories
                  WHERE user_id = $1 AND id = ANY($2) AND deleted_at IS NULL
                  ORDER BY id
                  FOR UPDATE"
            ),
            &[user.as_uuid(), &ids],
        )
        .await
        .map_err(|error| ApiError::database("lock categories", &error))?;
    rows.iter().map(read).collect()
}

/// Reads one row.
fn read(row: &Row) -> Result<Category, ApiError> {
    let reader = Reader::new(ENTITY, row)?;
    Ok(Category {
        id: reader.id(),
        name: reader.text("name")?,
        parent_id: reader.optional_uuid("parent_id")?,
        kind: reader.variant("kind")?,
        icon: reader.optional_text("icon")?,
        is_system: reader.flag("is_system")?,
        sort_order: reader.narrowed("sort_order")?,
        sync: SyncMeta {
            created_at: reader.instant("created_at")?,
            updated_at: reader.instant("updated_at")?,
            deleted_at: reader.optional_instant("deleted_at")?,
            device_id: reader.uuid("device_id")?,
        },
    })
}
