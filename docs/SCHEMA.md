# Schema

Companion to [ARCHITECTURE.md](ARCHITECTURE.md). Written 2026-11-10, revision 1. Describes
the three places the shape of the data is written down and how they are kept in agreement:

| Where | File | Holds |
|---|---|---|
| Rust | `crates/core/src/model.rs`, `enums.rs`, `money.rs`, `ids.rs` | the types every rule reads |
| Server | `db/postgres/0001_init.sql` | Postgres behind `mt-server`, one database for everyone |
| Device | `db/sqlite/0001_init.sql` | SQLite on the phone or desktop, one database per person |

Drawn out in [SCHEMA-DIAGRAM.md](SCHEMA-DIAGRAM.md).

Nothing in the compiler connects them, so `crates/core/tests/schema_agreement.rs` does. It
reads both SQL files and fails if an enum variant or a currency has no matching `CHECK`
constraint, if the two schemas declare different columns on a synced table, if a
floating-point or stored-balance column appears, if a synced Postgres table is not keyed on
its owner or not stamped by its trigger, or if a synced device table cannot say what still
needs pushing.

## Conventions

**Money** is a `BIGINT`/`INTEGER` of minor units in an `*_minor` column, beside a
three-letter currency column constrained to the registry in `money.rs`. There is no
`NUMERIC`, `REAL` or `DOUBLE PRECISION` in either file, and `clippy::float_arithmetic` is
denied across the workspace. A transaction keeps the currency it happened in for ever;
conversion is a presentation concern using the rate for that transaction's own date.

Amounts are capped at 10^15 minor units. Not a currency limit — a guard: `i64` reaches 92
quadrillion, and one row near that ceiling makes every balance walk over its account return
`Overflow` with nothing in the error to say which row caused it.

**Amounts are never negative.** `direction` is `in` or `out` and carries the sign.
`Transaction::signed_amount` is the only place the two are combined.

**Ids** are UUIDv7: time-ordered, so appending stays index-friendly, and generated on
whichever device created the row. Postgres stores them as `UUID`, SQLite as the canonical
hyphenated `TEXT` with a shape check, so an id that syncs fine locally is not rejected on
push.

**Instants** are UTC: `TIMESTAMPTZ` in Postgres, `INTEGER` milliseconds since the epoch in
SQLite, bounded to 1900–2200 either way. **Dates** — the ones that are genuinely a calendar
day, like `starts_on` — are `DATE` and `TEXT` validated with `date()`, which refuses both
`abcd-ef-gh` and the 31st of February.

**Text** is trimmed, then measured in characters, against the same limit in all three places
(`check_text`, `length(trim(…))`, `char_length(btrim(…))`). Three different answers to "is
this name valid" is how a row becomes unwritable on the device that did not create it.

**On the wire**, instants are RFC 3339 and dates are `YYYY-MM-DD`. That is a protocol
commitment, not a dependency's default, and `crates/core/tests/wire.rs` pins it.

**Deletes are soft.** Every synced table has `deleted_at`; a delete on one device is a row
update, so another device that has not pulled yet cannot resurrect it.

Every SQLite table is `STRICT`, which is what refuses `20.50` in an amount column.

## Tables

`accounts` · `categories` · `category_rules` · `recurring` · `transactions` · `budgets` ·
`attachments` exist in both schemas and sync. `fx_rates` exists in both but is reference data,
not user data.

Device only, never synced:

- `inbox_items` — parsed-but-unconfirmed candidates. **There is no column for a message body.**
  An SMS is parsed in memory and the text discarded; what is kept is the parsed fields, the
  allow-listed sender and a fingerprint so the same alert is not offered twice. The Play
  exception for SMS-based money management forbids holding non-financial or personal SMS
  history, and a schema with nowhere to put it cannot drift into holding it.
- `sms_senders` — the allow-list the receiver checks *before* reading a body into memory.
- `unparsed_counts` — a tally per sender per day of allow-listed messages no pattern claimed.
  `pack_version` is `0`, not `NULL`, when no pack is installed: a `STRICT` table makes every
  primary-key column `NOT NULL`, so a nullable one would throw on exactly the messages that
  arrive before the first pack lands — the ones this tally exists to catch.
- `sync_state`, `settings` — local bookkeeping.

Server only:

- `users`, `devices`, `refresh_tokens` — identity. Passwords are argon2id; refresh tokens are
  stored as a SHA-256 digest with a `rotated_from` chain, so reuse of a rotated token is
  detectable as theft.
- `rule_packs` — versioned SMS patterns and default categories with a detached signature.
- `redacted_samples` — a message the device could not parse, redacted on the device and sent
  only because the user pressed the button and saw exactly what would be sent. Deliberately
  not foreign-keyed to `users`: a pattern-writing session should not be a window onto a person.

## The decisions worth knowing about

**Tenant isolation is structural.** Every synced Postgres table is keyed on `(user_id, id)`,
and every foreign key carries `user_id` through — `FOREIGN KEY (user_id, account_id, currency)
REFERENCES accounts (user_id, id, currency)`. A transaction physically cannot reference
another user's account, whatever a handler forgets to check.

**A row's currency is pinned to its account's** by that same composite key, in both schemas.
A USD row in a GHS account would make the balance walk either fail for that account for ever
or sum two currencies — and since balances are derived, there is no stored copy to notice the
difference.

**Sync pages on a per-user counter handed out in commit order.** `next_sync_seq()` increments
`users.sync_seq` and holds that row locked until the transaction commits; a `BEFORE INSERT OR
UPDATE` trigger on every synced table calls it. Two things make this worth the machinery:

- A `DEFAULT nextval(…)` fires on INSERT only, so an edit or a soft delete would keep its old
  number and never be returned by `server_seq > watermark` again. That is the resurrection
  bug soft deletes exist to prevent.
- A sequence hands out numbers when a transaction *starts*, while rows become visible when it
  *commits*. A slow writer takes 21, a fast writer takes 22 and commits first, a client pulls
  22, and 21 is lost for ever. `db/tests/postgres_sync_ordering.sh` runs two concurrent
  writers and proves the second waits.

**The device knows what it has not pushed.** Every synced SQLite table carries `dirty` (1
until the server acknowledges) and `server_seq` (`NULL` until first pushed). Push reads
`WHERE dirty = 1`. It cannot read `updated_at > last_push_at`, because `updated_at` on a
pulled row is another device's clock: a peer running fast would hide a local edit behind a
watermark that had already passed it.

**Transfers are two rows, not a direction.** There is no `transfer` value in the `direction`
check. A transfer is an `out` leg and an `in` leg sharing a `transfer_id`, which the balance
walk nets. A leg may not carry a category, enforced by a `CHECK`. *What the database cannot
enforce* — exactly two legs, opposite directions, same amount, same instant — lives in
`validate_transfer_pair`, because the legs arrive separately over sync and no SQL constraint
counts rows sharing a column at write time. Cross-currency transfers are refused there:
netting two legs of different currencies has no meaning without a rate, and inventing one
would put an invented number into the walk.

**One level of category nesting.** SQL can refuse self-parenting; it cannot refuse a
grandparent chain without recursion, so `Category::validate_under` carries that.

**No stored balances.** Account balances come from the walk in `mt-core`. The prior art keeps
a denormalised balance column and ships a cron command to repair it when it drifts; that is a
warning, not a pattern.

**`external_id` is unique per account.** A re-imported statement does not create a second copy
of every row. Per *account*, because two banks can easily use the same reference format.

**Budgets may not overlap, rather than being unique.** Postgres uses an `EXCLUDE USING gist`
over `(user_id, category, currency, period, daterange)`, so "am I over budget" has exactly one
answer *and* next year's cap can be set while this year's is still live. A plain unique key
would force you to delete the cap last quarter's verdict was measured against in order to
raise it. SQLite has no exclusion constraint, so there the rule is enforced by
`Budget::validate_against` before the insert, with a unique index catching exact duplicates.

**Attachment paths are relative and cannot climb.** Absolute paths, `..` segments, backslashes
and padding are refused in both schemas and in `check_relative_path`. The value arrives over
sync from another device and is joined to a directory the app owns.

**FX rates are scaled integers**: `rate = rate_e8 / 100_000_000`. Server rows are reference
data every device copies; a rate the user types in is theirs alone and is never pushed back.

## Running the checks

```sh
cargo test -p mt-core                     # types, validation, wire format, schema agreement
db/tests/sqlite_constraints.sh            # device schema, needs sqlite3

# server schema, needs a Postgres
docker run --rm -d --name mt-pg -e POSTGRES_PASSWORD=pw postgres:17-alpine
docker exec -i mt-pg psql -v ON_ERROR_STOP=1 -U postgres < db/postgres/0001_init.sql
docker exec -i mt-pg psql -v ON_ERROR_STOP=1 -U postgres < db/tests/postgres_constraints.sql
db/tests/postgres_sync_ordering.sh        # two concurrent writers; commit-order paging
docker rm -f mt-pg
```

## Still open

- **Migrations.** Both files are revision 1 and apply to an empty database. Once there is a
  released build, the server needs a migration runner (`sqlx migrate`, most likely) and the
  device needs drift's schema versions with a tested upgrade path. Neither is wired up yet.
- **Drift definitions.** `db/sqlite/0001_init.sql` is the statement of intent; the Flutter app
  will declare the same tables in drift, generated to match this file.
- **Attachment blobs on the server.** The `attachments` table holds metadata. Where the bytes
  live when syncing between devices is not decided.
- **Row-level security.** The composite keys make cross-tenant references impossible; Postgres
  RLS would also stop a `SELECT` that forgets its `WHERE user_id`. Worth adding when the
  repository layer exists, so the policy and the queries are written together.
- **Cross-currency transfers.** Refused for now. Supporting them means storing both legs'
  amounts and the rate that connects them, which is a schema change, not a validation change.
