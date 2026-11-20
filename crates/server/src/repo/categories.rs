//! Categories: what a row was for. Every query is scoped to one user.
//!
//! Nesting is one level, and the rule lives in `mt-core` (`Category::validate_under`): the
//! schema can refuse a category that is its own parent, but not a grandparent chain, so the
//! parent is read here first and handed to the core to judge.

use deadpool_postgres::Pool;
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
