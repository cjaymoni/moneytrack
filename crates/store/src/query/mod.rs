//! The reads behind the screens.
//!
//! A screen asks a question it can render — "what does this account hold", "what does each
//! of them hold" — and gets an answer already shaped for it. It does not assemble one from
//! a join it wrote itself, because then it would be two joins, written twice, drifting.
//!
//! Every answer here is the core's. The store loads the rows and `mt-core` decides what
//! they add up to, which is the same code the server runs.

pub mod balances;
