# Schema diagrams

Revision 1. Generated from `docs/diagrams/*.mmd` by `docs/diagrams/build.py` — edit the
`.mmd` files, not this page. The tables are described in [SCHEMA.md](SCHEMA.md).

GitHub renders the diagrams below. For a local render, open `docs/diagrams/index.html` in a
browser.

Every synced table also carries `created_at`, `updated_at`, `deleted_at` and `device_id`; the
Postgres copies carry `user_id` and `server_seq` as well. Those are left off the diagrams so
the shape stays readable.

## The ledger

Seven tables that exist in both databases and sync between them, plus the reporting rates. This is the part of the schema the product is actually about.

```mermaid
erDiagram
    accounts {
        uuid   id                    PK "UUIDv7, device-generated"
        text   name                  "1..64 chars"
        text   kind                  "cash | momo | bank | card"
        char3  currency              "one currency per account, fixed"
        bigint opening_balance_minor "the walk starts here"
        ts     archived_at           "hidden, not deleted"
        bigint server_seq            "per-user, stamped in commit order"
        bool   dirty                 "device only -- not yet pushed"
    }

    categories {
        uuid id        PK
        text name      "1..64 chars"
        uuid parent_id FK "one level of nesting"
        text kind      "income | expense"
        text icon
        bool is_system "shipped by a rule pack"
    }

    category_rules {
        uuid id            PK
        text pattern       "data, not code; rejected loudly at save"
        text matched_field "label | counterparty | raw_text"
        uuid category_id   FK
        int  priority      "lowest first, ties break on id"
        bool is_regex
        bool enabled
        text origin        "user | pack"
        uuid pack_id       "set iff origin = pack"
    }

    transactions {
        uuid   id           PK
        uuid   account_id   FK
        text   direction    "in | out -- carries the sign"
        bigint amount_minor "never negative"
        char3  currency     "kept for ever"
        ts     booked_at    "may be in the future"
        uuid   category_id  FK "null on a transfer leg"
        text   label
        text   counterparty
        text   note
        text   status       "pending | confirmed"
        text   source       "manual | sms | csv | ofx | receipt | recurring | adjustment"
        text   source_ref
        text   external_id  "unique per account"
        uuid   recurring_id FK
        uuid   transfer_id  "shared by the two legs"
        bigint server_seq   "per-user, stamped in commit order"
        bool   dirty        "device only -- not yet pushed"
    }

    recurring {
        uuid   id                PK
        text   name
        uuid   account_id        FK
        text   direction
        bigint amount_minor
        char3  currency
        uuid   category_id       FK
        text   label
        text   freq              "daily | weekly | monthly | yearly"
        int    interval          ">= 1"
        int    by_month_day      "1..31, clamps in short months"
        int    by_weekday        "0..6, Monday = 0"
        date   starts_on
        date   ends_on
        date   generated_through "makes generation idempotent"
        text   timezone          "cadence reckoned locally"
    }

    budgets {
        uuid   id          PK
        uuid   category_id FK "null caps all spending"
        text   period      "weekly | monthly | yearly"
        bigint cap_minor   "> 0"
        char3  currency    "a cedi cap and a dollar cap are two budgets"
        date   starts_on
        date   ends_on
        int    warn_at_bps "8000 = warn at 80%"
    }

    attachments {
        uuid   id             PK
        uuid   transaction_id FK
        text   relative_path  "never absolute"
        text   media_type
        bigint byte_size
        char64 sha256
    }

    fx_rates {
        char3  base    PK "reporting only"
        char3  quote   PK
        bigint rate_e8 "rate x 100,000,000 -- never a float"
        date   as_of   PK "the rate for the transaction's own date"
    }

    accounts     ||--o{ transactions   : "holds"
    accounts     ||--o{ recurring      : "generates into"
    categories   |o--o{ transactions   : "classifies"
    categories   |o--o{ recurring      : "classifies"
    categories   ||--o{ category_rules : "assigned by"
    categories   |o--o{ budgets        : "capped by"
    categories   |o--o{ categories     : "parent of"
    recurring    |o--o{ transactions   : "generates"
    transactions ||--o{ attachments    : "evidenced by"
    transactions }o--o| transactions   : "paired by transfer_id"
```

## On the device only

The review inbox and the SMS machinery. None of it syncs: only a confirmed transaction is allowed to leave the handset.

```mermaid
erDiagram
    inbox_items {
        uuid   id                      PK
        text   source                  "sms | csv | ofx | receipt"
        ts     received_at
        text   origin_ref              "allow-listed sender, or file name"
        text   fingerprint             "unique -- never offer the same alert twice"
        uuid   account_id              FK
        text   direction
        bigint amount_minor
        char3  currency
        ts     booked_at
        text   label
        text   counterparty
        uuid   category_id             FK
        int    category_confidence_bps "10000 = the source said so"
        text   external_id
        bigint reported_balance_minor  "what the alert claimed"
        text   status                  "pending | accepted | rejected | merged"
        uuid   transaction_id          FK "set iff accepted"
        uuid   duplicate_of            FK "set iff merged"
    }

    sms_senders {
        text sender  PK "checked BEFORE a body is read into memory"
        text label
        bool enabled
        text origin  "user | pack"
    }

    unparsed_counts {
        text sender       PK "a tally, never the message"
        date on_date      PK
        int  pack_version PK
        int  tally
    }

    sync_state {
        int  id             PK "exactly one row"
        text device_id
        int  pulled_through "highest server_seq seen"
        ts   last_pull_at
        ts   last_push_at
        int  pack_version
    }

    settings {
        text key   PK "default account, reporting currency, theme"
        text value
    }

    accounts     ||--o{ inbox_items : "proposed for"
    categories   |o--o{ inbox_items : "proposed as"
    transactions |o--o| inbox_items : "became, on accept"
    transactions |o--o| inbox_items : "merged into, on duplicate"
```

## On the server only

Identity, rule packs, and the columns every synced row carries. `ledger_tables` stands in for all seven, which share the same ownership and sync columns.

```mermaid
erDiagram
    users {
        uuid id                PK
        text email             "unique while live"
        text password_hash     "argon2id"
        int  sync_seq          "per-user counter; locked until commit"
        ts   email_verified_at
        ts   deleted_at        "frees the address for re-use"
    }

    devices {
        uuid id             PK
        uuid user_id        PK "part of the key, not just a column"
        text name
        text platform       "android | ios | macos | windows | linux | web"
        int  pulled_through "server's half of the watermark"
        ts   last_seen_at
        ts   revoked_at
    }

    refresh_tokens {
        uuid  id           PK
        uuid  user_id      FK
        uuid  device_id    FK
        bytea token_hash   "SHA-256; the token itself is never stored"
        ts    expires_at
        ts    revoked_at
        uuid  rotated_from "reuse of a rotated token means theft"
    }

    rule_packs {
        uuid  id           PK
        int   version      "unique per channel"
        text  channel      "stable | beta"
        jsonb manifest     "patterns and categories, validated before publish"
        bytea signature    "detached; clients verify before applying"
        ts    published_at
    }

    redacted_samples {
        uuid id            PK
        text sender
        text redacted_body "digits and names stripped ON THE DEVICE"
        text app_version
        int  pack_version
        ts   resolved_at
    }

    ledger_tables {
        uuid   user_id    PK "accounts, categories, category_rules,"
        uuid   id         PK "recurring, transactions, budgets, attachments"
        bigint server_seq "stamped by trigger on INSERT *and* UPDATE"
        ts     updated_at "last-write-wins tiebreak"
        ts     deleted_at "soft, so a delete is not resurrected"
        uuid   device_id  "last writer"
    }

    users ||--o{ devices        : "signs in from"
    users ||--o{ refresh_tokens : "holds sessions"
    users ||--o{ ledger_tables  : "owns every row of"
    devices ||--o{ refresh_tokens : "issued to"
    refresh_tokens |o--o| refresh_tokens : "rotated_from"
```
