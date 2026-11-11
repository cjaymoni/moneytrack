# moneytrack (working name)

Personal income and spending tracker. Android, iOS, desktop and web, with automatic
capture from MoMo and bank SMS on Android. Intended for publication, not just personal use.

The thinking so far:

- [docs/PLAN.md](docs/PLAN.md) — what it does, in what order, and what will bite.
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — how it is put together and why.
- [docs/SCHEMA.md](docs/SCHEMA.md) — the data model, and how the Rust types and the two
  database schemas are kept in agreement.
- [docs/SCHEMA-DIAGRAM.md](docs/SCHEMA-DIAGRAM.md) — the same thing as three diagrams.

Built so far: `crates/core`, which is phase 0 complete — money, ids, enums and the row types,
plus the five rules that turn rows into answers: the balance walk, the categorisation ladder,
recurrence expansion, duplicate scoring and budget evaluation. The Postgres and SQLite schemas
are in `db/`. `cargo test` covers the lot; `db/tests/` covers the database constraints.

Decided: a Rust core and server, a Flutter UI, integer minor units for money, on-device
SMS parsing, and pending transactions as the forecast.
