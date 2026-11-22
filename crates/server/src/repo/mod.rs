//! The repositories: every query, and every one of them scoped to a user.
//!
//! The rule this layer exists to enforce: **no accessor takes anything less than a
//! `UserId`.** Not "the handlers remember to check", not "the service layer adds a filter" —
//! there is no function here that can read or write a row without being told whose it is,
//! and `tests/tenancy.rs` fails the build if one appears. Retrofitting an owner check onto
//! each query later is how IDOR bugs ship.
//!
//! The schema does its half structurally: every per-person table is keyed on
//! `(user_id, id)` and every foreign key carries `user_id` through, so a transaction cannot
//! reference another user's account even if this layer asked it to.
//!
//! Nothing `sqlx` owns leaves this module. Rows arrive as `mt-core` values and leave as
//! `mt-core` values, so the handlers above never see a `PgRow` or an error code.

pub mod accounts;
pub mod budgets;
pub mod categories;
pub mod rows;
pub mod transactions;
pub mod users;
