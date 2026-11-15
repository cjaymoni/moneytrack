//! The repositories: one module per table, every row going in and out through `mt-core`.
//!
//! Two rules hold here. A row is validated by the core before it is written, so the error a
//! caller gets names the field rather than saying `CHECK constraint failed`. And nothing
//! `rusqlite` owns leaves this crate: rows arrive as core values and leave as core values,
//! which is what keeps the published surface above free of storage.

pub mod accounts;
pub mod transactions;
