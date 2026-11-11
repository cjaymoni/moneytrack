-- moneytrack device schema, revision 1.
--
-- SQLite on the phone, desktop and (via the same drift definitions) anywhere else the app
-- runs. This is the working copy; the server is the merge point. docs/SCHEMA.md maps this
-- onto db/postgres/0001_init.sql and records where the two differ on purpose.
--
-- Conventions, chosen once so drift and mt-core agree:
--   * ids           TEXT, the canonical hyphenated form of a UUIDv7
--   * instants      INTEGER, milliseconds since the Unix epoch, always UTC
--   * dates         TEXT, 'YYYY-MM-DD', validated with date() rather than by shape
--   * money         INTEGER minor units in a named column, plus a currency from a fixed list
--   * booleans      INTEGER 0 or 1
--   * every table   STRICT, so a fractional amount cannot be stored at all
--
-- There is no user_id here: the device database holds exactly one person's data, and the
-- file itself is encrypted at rest.
--
-- Every synced table carries two columns the server schema does not need:
--   * dirty       1 while the row has local changes the server has not acknowledged
--   * server_seq  the number the server gave this row; NULL means it has never been pushed
-- Push reads `WHERE dirty = 1`. It cannot read `updated_at > last_push_at`, because
-- updated_at on a pulled row is another device's clock: a peer running fast would hide a
-- local edit behind a watermark that had already passed it.

PRAGMA foreign_keys = ON;

-- ------------------------------------------------------------------------------ ledger ----

CREATE TABLE accounts (
    id                    TEXT NOT NULL PRIMARY KEY
                          CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    name                  TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 64),
    kind                  TEXT NOT NULL CHECK (kind IN ('cash', 'momo', 'bank', 'card')),
    currency              TEXT NOT NULL CHECK (currency IN ('GHS', 'NGN', 'XOF', 'XAF', 'KES', 'TZS', 'UGX', 'RWF', 'ZAR', 'EGP', 'MAD', 'USD', 'EUR', 'GBP', 'CAD', 'AUD', 'CHF', 'CNY', 'INR', 'JPY', 'AED')),
    opening_balance_minor INTEGER NOT NULL DEFAULT 0
                          CHECK (opening_balance_minor BETWEEN -1000000000000000 AND 1000000000000000),
    sort_order            INTEGER NOT NULL DEFAULT 0,
    archived_at           INTEGER,
    created_at            INTEGER NOT NULL CHECK (created_at BETWEEN -2208988800000 AND 7258118400000),
    updated_at            INTEGER NOT NULL CHECK (updated_at >= created_at),
    deleted_at            INTEGER CHECK (deleted_at IS NULL OR deleted_at >= created_at),
    device_id             TEXT NOT NULL,
    dirty                 INTEGER NOT NULL DEFAULT 1 CHECK (dirty IN (0, 1)),
    server_seq            INTEGER,
    -- The parent side of the composite foreign key that pins a row's currency to its
    -- account's, mirroring accounts_currency_key on the server.
    UNIQUE (id, currency)
) STRICT;

CREATE INDEX accounts_dirty ON accounts (updated_at) WHERE dirty = 1;

CREATE TABLE categories (
    id          TEXT NOT NULL PRIMARY KEY
                CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    name        TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 64),
    parent_id   TEXT REFERENCES categories (id) ON DELETE SET NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('income', 'expense')),
    icon        TEXT CHECK (icon IS NULL OR length(trim(icon)) BETWEEN 1 AND 64),
    is_system   INTEGER NOT NULL DEFAULT 0 CHECK (is_system IN (0, 1)),
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL CHECK (created_at BETWEEN -2208988800000 AND 7258118400000),
    updated_at  INTEGER NOT NULL CHECK (updated_at >= created_at),
    deleted_at  INTEGER CHECK (deleted_at IS NULL OR deleted_at >= created_at),
    device_id   TEXT NOT NULL,
    dirty       INTEGER NOT NULL DEFAULT 1 CHECK (dirty IN (0, 1)),
    server_seq  INTEGER,
    -- One level of nesting is the product rule; SQL can refuse self-parenting but not a
    -- grandparent chain, so Category::validate_under carries the rest.
    CONSTRAINT categories_not_own_parent CHECK (parent_id IS NULL OR parent_id <> id)
) STRICT;

CREATE INDEX categories_parent ON categories (parent_id);
CREATE INDEX categories_dirty ON categories (updated_at) WHERE dirty = 1;

CREATE TABLE recurring (
    id                TEXT NOT NULL PRIMARY KEY
                      CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    name              TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 64),
    account_id        TEXT NOT NULL,
    direction         TEXT NOT NULL CHECK (direction IN ('in', 'out')),
    amount_minor      INTEGER NOT NULL CHECK (amount_minor BETWEEN 0 AND 1000000000000000),
    currency          TEXT NOT NULL CHECK (currency IN ('GHS', 'NGN', 'XOF', 'XAF', 'KES', 'TZS', 'UGX', 'RWF', 'ZAR', 'EGP', 'MAD', 'USD', 'EUR', 'GBP', 'CAD', 'AUD', 'CHF', 'CNY', 'INR', 'JPY', 'AED')),
    category_id       TEXT REFERENCES categories (id) ON DELETE SET NULL,
    label             TEXT NOT NULL CHECK (length(trim(label)) BETWEEN 1 AND 200),
    counterparty      TEXT CHECK (counterparty IS NULL OR length(trim(counterparty)) BETWEEN 1 AND 120),
    freq              TEXT NOT NULL CHECK (freq IN ('daily', 'weekly', 'monthly', 'yearly')),
    interval          INTEGER NOT NULL CHECK (interval >= 1),
    by_month_day      INTEGER CHECK (by_month_day IS NULL OR by_month_day BETWEEN 1 AND 31),
    by_weekday        INTEGER CHECK (by_weekday IS NULL OR by_weekday BETWEEN 0 AND 6),
    starts_on         TEXT NOT NULL CHECK (starts_on IS date(starts_on)),
    ends_on           TEXT CHECK (ends_on IS NULL OR ends_on IS date(ends_on)),
    generated_through TEXT CHECK (generated_through IS NULL OR generated_through IS date(generated_through)),
    timezone          TEXT NOT NULL DEFAULT 'Africa/Accra'
                      CHECK (length(trim(timezone)) BETWEEN 1 AND 64),
    created_at        INTEGER NOT NULL CHECK (created_at BETWEEN -2208988800000 AND 7258118400000),
    updated_at        INTEGER NOT NULL CHECK (updated_at >= created_at),
    deleted_at        INTEGER CHECK (deleted_at IS NULL OR deleted_at >= created_at),
    device_id         TEXT NOT NULL,
    dirty             INTEGER NOT NULL DEFAULT 1 CHECK (dirty IN (0, 1)),
    server_seq        INTEGER,
    CONSTRAINT recurring_window CHECK (ends_on IS NULL OR ends_on >= starts_on),
    -- A cadence and its day fields have to agree, or rows generate on the wrong date.
    CONSTRAINT recurring_cadence CHECK (
        CASE freq
            WHEN 'daily'   THEN by_month_day IS NULL AND by_weekday IS NULL
            WHEN 'weekly'  THEN by_weekday IS NOT NULL AND by_month_day IS NULL
            ELSE by_month_day IS NOT NULL AND by_weekday IS NULL
        END),
    FOREIGN KEY (account_id, currency) REFERENCES accounts (id, currency)
) STRICT;

CREATE INDEX recurring_dirty ON recurring (updated_at) WHERE dirty = 1;

CREATE TABLE transactions (
    id            TEXT NOT NULL PRIMARY KEY
                  CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    account_id    TEXT NOT NULL,
    -- No 'transfer' direction: a transfer is two rows sharing transfer_id, and the balance
    -- walk nets them. "Exactly two legs, opposite directions, same amount" is not
    -- expressible here, because the legs arrive separately over sync; mt-core's
    -- validate_transfer_pair carries it.
    direction     TEXT NOT NULL CHECK (direction IN ('in', 'out')),
    -- Non-negative; the sign is carried by direction. Bounded well below the i64 ceiling so
    -- one bad row cannot make every balance walk over its account overflow.
    amount_minor  INTEGER NOT NULL CHECK (amount_minor BETWEEN 0 AND 1000000000000000),
    -- Pinned to the account's currency by the composite foreign key below.
    currency      TEXT NOT NULL CHECK (currency IN ('GHS', 'NGN', 'XOF', 'XAF', 'KES', 'TZS', 'UGX', 'RWF', 'ZAR', 'EGP', 'MAD', 'USD', 'EUR', 'GBP', 'CAD', 'AUD', 'CHF', 'CNY', 'INR', 'JPY', 'AED')),
    -- May be in the future. Future-dated pending rows are the forecast.
    booked_at     INTEGER NOT NULL CHECK (booked_at BETWEEN -2208988800000 AND 7258118400000),
    category_id   TEXT REFERENCES categories (id) ON DELETE SET NULL,
    label         TEXT NOT NULL CHECK (length(trim(label)) BETWEEN 1 AND 200),
    counterparty  TEXT CHECK (counterparty IS NULL OR length(trim(counterparty)) BETWEEN 1 AND 120),
    note          TEXT CHECK (note IS NULL OR length(trim(note)) BETWEEN 1 AND 2000),
    status        TEXT NOT NULL CHECK (status IN ('pending', 'confirmed')),
    source        TEXT NOT NULL CHECK (source IN ('manual', 'sms', 'csv', 'ofx', 'receipt', 'recurring', 'adjustment')),
    source_ref    TEXT CHECK (source_ref IS NULL OR length(trim(source_ref)) BETWEEN 1 AND 200),
    external_id   TEXT CHECK (external_id IS NULL OR length(trim(external_id)) BETWEEN 1 AND 128),
    recurring_id  TEXT REFERENCES recurring (id) ON DELETE SET NULL,
    transfer_id   TEXT,
    created_at    INTEGER NOT NULL CHECK (created_at BETWEEN -2208988800000 AND 7258118400000),
    updated_at    INTEGER NOT NULL CHECK (updated_at >= created_at),
    deleted_at    INTEGER CHECK (deleted_at IS NULL OR deleted_at >= created_at),
    device_id     TEXT NOT NULL,
    dirty         INTEGER NOT NULL DEFAULT 1 CHECK (dirty IN (0, 1)),
    server_seq    INTEGER,
    CONSTRAINT transactions_transfer_uncategorised
        CHECK (transfer_id IS NULL OR category_id IS NULL),
    FOREIGN KEY (account_id, currency) REFERENCES accounts (id, currency)
) STRICT;

-- The index the balance walk reads: one account, in date order.
CREATE INDEX transactions_walk ON transactions (account_id, booked_at) WHERE deleted_at IS NULL;
-- "Where did the money go this month" reads every account over a date window.
CREATE INDEX transactions_period ON transactions (booked_at) WHERE deleted_at IS NULL;
-- Not partial on category_id: uncategorised rows are exactly the ones a breakdown most
-- needs to surface.
CREATE INDEX transactions_category ON transactions (category_id, booked_at)
    WHERE deleted_at IS NULL;
CREATE INDEX transactions_transfer ON transactions (transfer_id) WHERE transfer_id IS NOT NULL;
CREATE INDEX transactions_pending ON transactions (booked_at) WHERE status = 'pending' AND deleted_at IS NULL;
CREATE INDEX transactions_dirty ON transactions (updated_at) WHERE dirty = 1;
-- Stops a re-imported statement creating a second copy of every row.
CREATE UNIQUE INDEX transactions_external ON transactions (account_id, external_id)
    WHERE external_id IS NOT NULL AND deleted_at IS NULL;
-- A recurring template fires at most once per instant. Generation is idempotent through the
-- template's `generated_through` watermark, but that only holds if the rows and the new
-- watermark are written in one transaction; this is the backstop for the case where they are
-- not, which would otherwise show the user next month's rent two or three times over.
CREATE UNIQUE INDEX transactions_recurring_instance ON transactions (recurring_id, booked_at)
    WHERE recurring_id IS NOT NULL AND deleted_at IS NULL;

CREATE TABLE category_rules (
    id            TEXT NOT NULL PRIMARY KEY
                  CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    pattern       TEXT NOT NULL CHECK (length(trim(pattern)) BETWEEN 1 AND 500),
    matched_field TEXT NOT NULL CHECK (matched_field IN ('label', 'counterparty', 'raw_text')),
    category_id   TEXT NOT NULL REFERENCES categories (id) ON DELETE CASCADE,
    priority      INTEGER NOT NULL DEFAULT 100,
    is_regex      INTEGER NOT NULL DEFAULT 0 CHECK (is_regex IN (0, 1)),
    enabled       INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    origin        TEXT NOT NULL DEFAULT 'user' CHECK (origin IN ('user', 'pack')),
    pack_id       TEXT,
    created_at    INTEGER NOT NULL CHECK (created_at BETWEEN -2208988800000 AND 7258118400000),
    updated_at    INTEGER NOT NULL CHECK (updated_at >= created_at),
    deleted_at    INTEGER CHECK (deleted_at IS NULL OR deleted_at >= created_at),
    device_id     TEXT NOT NULL,
    dirty         INTEGER NOT NULL DEFAULT 1 CHECK (dirty IN (0, 1)),
    server_seq    INTEGER,
    CONSTRAINT category_rules_pack_origin
        CHECK ((origin = 'pack') = (pack_id IS NOT NULL))
) STRICT;

CREATE INDEX category_rules_order ON category_rules (priority, id)
    WHERE deleted_at IS NULL AND enabled = 1;
CREATE INDEX category_rules_dirty ON category_rules (updated_at) WHERE dirty = 1;

CREATE TABLE budgets (
    id          TEXT NOT NULL PRIMARY KEY
                CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    category_id TEXT REFERENCES categories (id) ON DELETE CASCADE,
    period      TEXT NOT NULL CHECK (period IN ('weekly', 'monthly', 'yearly')),
    cap_minor   INTEGER NOT NULL CHECK (cap_minor BETWEEN 1 AND 1000000000000000),
    currency    TEXT NOT NULL CHECK (currency IN ('GHS', 'NGN', 'XOF', 'XAF', 'KES', 'TZS', 'UGX', 'RWF', 'ZAR', 'EGP', 'MAD', 'USD', 'EUR', 'GBP', 'CAD', 'AUD', 'CHF', 'CNY', 'INR', 'JPY', 'AED')),
    starts_on   TEXT NOT NULL CHECK (starts_on IS date(starts_on)),
    ends_on     TEXT CHECK (ends_on IS NULL OR ends_on IS date(ends_on)),
    warn_at_bps INTEGER NOT NULL DEFAULT 8000 CHECK (warn_at_bps BETWEEN 0 AND 10000),
    created_at  INTEGER NOT NULL CHECK (created_at BETWEEN -2208988800000 AND 7258118400000),
    updated_at  INTEGER NOT NULL CHECK (updated_at >= created_at),
    deleted_at  INTEGER CHECK (deleted_at IS NULL OR deleted_at >= created_at),
    device_id   TEXT NOT NULL,
    dirty       INTEGER NOT NULL DEFAULT 1 CHECK (dirty IN (0, 1)),
    server_seq  INTEGER,
    CONSTRAINT budgets_window CHECK (ends_on IS NULL OR ends_on >= starts_on)
) STRICT;

-- SQLite has no exclusion constraint, so the server's "no two live budgets for the same
-- scope over overlapping dates" rule is enforced by Budget::validate_against in mt-core,
-- which runs before the insert. This index catches the exact-duplicate case cheaply; it
-- deliberately includes starts_on, so replacing a cap with a later one is allowed.
CREATE UNIQUE INDEX budgets_one_per_scope_and_start
    ON budgets (ifnull(category_id, ''), currency, period, starts_on) WHERE deleted_at IS NULL;
CREATE INDEX budgets_dirty ON budgets (updated_at) WHERE dirty = 1;

CREATE TABLE attachments (
    id             TEXT NOT NULL PRIMARY KEY
                   CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    transaction_id TEXT NOT NULL REFERENCES transactions (id) ON DELETE CASCADE,
    -- Resolved by joining it to a root the app owns. An absolute path or a `..` segment
    -- turns that join into a read somewhere else on the filesystem, and the value arrives
    -- over sync from another device.
    relative_path  TEXT NOT NULL CHECK (
                       length(relative_path) BETWEEN 1 AND 512
                       AND relative_path NOT GLOB '/*'
                       AND relative_path NOT GLOB '*..*'
                       AND instr(relative_path, char(92)) = 0
                       AND relative_path = trim(relative_path)),
    media_type     TEXT NOT NULL CHECK (length(trim(media_type)) BETWEEN 1 AND 128),
    byte_size      INTEGER NOT NULL CHECK (byte_size >= 0),
    sha256         TEXT NOT NULL CHECK (length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*'),
    created_at     INTEGER NOT NULL CHECK (created_at BETWEEN -2208988800000 AND 7258118400000),
    updated_at     INTEGER NOT NULL CHECK (updated_at >= created_at),
    deleted_at     INTEGER CHECK (deleted_at IS NULL OR deleted_at >= created_at),
    device_id      TEXT NOT NULL,
    dirty          INTEGER NOT NULL DEFAULT 1 CHECK (dirty IN (0, 1)),
    server_seq     INTEGER
) STRICT;

CREATE INDEX attachments_transaction ON attachments (transaction_id);
CREATE INDEX attachments_dirty ON attachments (updated_at) WHERE dirty = 1;

-- ------------------------------------------------------------------------------- inbox ----

-- Parsed-but-unconfirmed candidates. Device-only: inbox rows never sync, because only a
-- confirmed transaction is allowed to leave the handset. That is also why this table has no
-- dirty flag and no server_seq.
--
-- Note what is absent: there is no column for a message body. An SMS is parsed in memory and
-- the text is discarded. What is kept is the parsed fields, the allow-listed sender, and a
-- fingerprint so the same alert is not offered twice. The Play exception for SMS-based money
-- management forbids holding non-financial or personal SMS history, and a schema with
-- nowhere to put it cannot drift into holding it.
CREATE TABLE inbox_items (
    id                       TEXT NOT NULL PRIMARY KEY
                             CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    source                   TEXT NOT NULL CHECK (source IN ('sms', 'csv', 'ofx', 'receipt')),
    received_at              INTEGER NOT NULL CHECK (received_at BETWEEN -2208988800000 AND 7258118400000),
    origin_ref               TEXT CHECK (origin_ref IS NULL OR length(trim(origin_ref)) BETWEEN 1 AND 200),
    -- Non-reversible digest of the source message or statement line.
    fingerprint              TEXT NOT NULL CHECK (length(trim(fingerprint)) BETWEEN 1 AND 128),
    account_id               TEXT REFERENCES accounts (id) ON DELETE SET NULL,
    direction                TEXT NOT NULL CHECK (direction IN ('in', 'out')),
    amount_minor             INTEGER NOT NULL CHECK (amount_minor BETWEEN 0 AND 1000000000000000),
    currency                 TEXT NOT NULL CHECK (currency IN ('GHS', 'NGN', 'XOF', 'XAF', 'KES', 'TZS', 'UGX', 'RWF', 'ZAR', 'EGP', 'MAD', 'USD', 'EUR', 'GBP', 'CAD', 'AUD', 'CHF', 'CNY', 'INR', 'JPY', 'AED')),
    booked_at                INTEGER NOT NULL CHECK (booked_at BETWEEN -2208988800000 AND 7258118400000),
    label                    TEXT NOT NULL CHECK (length(trim(label)) BETWEEN 1 AND 200),
    counterparty             TEXT CHECK (counterparty IS NULL OR length(trim(counterparty)) BETWEEN 1 AND 120),
    category_id              TEXT REFERENCES categories (id) ON DELETE SET NULL,
    category_confidence_bps  INTEGER NOT NULL DEFAULT 0 CHECK (category_confidence_bps BETWEEN 0 AND 10000),
    external_id              TEXT CHECK (external_id IS NULL OR length(trim(external_id)) BETWEEN 1 AND 128),
    reported_balance_minor   INTEGER CHECK (reported_balance_minor IS NULL
                                 OR reported_balance_minor BETWEEN -1000000000000000 AND 1000000000000000),
    status                   TEXT NOT NULL DEFAULT 'pending'
                             CHECK (status IN ('pending', 'accepted', 'rejected', 'merged')),
    transaction_id           TEXT REFERENCES transactions (id) ON DELETE SET NULL,
    duplicate_of             TEXT REFERENCES transactions (id) ON DELETE SET NULL,
    created_at               INTEGER NOT NULL CHECK (created_at BETWEEN -2208988800000 AND 7258118400000),
    updated_at               INTEGER NOT NULL CHECK (updated_at >= created_at),
    deleted_at               INTEGER CHECK (deleted_at IS NULL OR deleted_at >= created_at),
    device_id                TEXT NOT NULL,
    CONSTRAINT inbox_accepted_has_transaction
        CHECK ((status = 'accepted') = (transaction_id IS NOT NULL)),
    CONSTRAINT inbox_merged_has_duplicate
        CHECK ((status = 'merged') = (duplicate_of IS NOT NULL))
) STRICT;

-- The inbox screen: oldest waiting first.
CREATE INDEX inbox_items_waiting ON inbox_items (received_at) WHERE status = 'pending' AND deleted_at IS NULL;
-- "Have I seen this alert already?"
CREATE UNIQUE INDEX inbox_items_fingerprint ON inbox_items (fingerprint);

-- Senders the SMS receiver will read a body from. The check happens before the body is read
-- into memory, which is the policy requirement, not an optimisation. Seeded from the active
-- rule pack and editable by the user.
CREATE TABLE sms_senders (
    sender      TEXT NOT NULL PRIMARY KEY CHECK (length(trim(sender)) BETWEEN 1 AND 64),
    label       TEXT NOT NULL CHECK (length(trim(label)) BETWEEN 1 AND 64),
    enabled     INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    origin      TEXT NOT NULL DEFAULT 'pack' CHECK (origin IN ('user', 'pack')),
    created_at  INTEGER NOT NULL
) STRICT;

-- An allow-listed message that no pattern claimed. Counted, never stored: one row per
-- sender per day with a tally, so "MTN changed their wording" is visible without keeping
-- anybody's messages.
--
-- pack_version is 0, not NULL, when no pack is installed yet: a STRICT table makes every
-- primary-key column NOT NULL, so a nullable one here would throw on exactly the messages
-- that arrive before the first pack lands -- the ones this tally exists to catch.
CREATE TABLE unparsed_counts (
    sender       TEXT NOT NULL CHECK (length(trim(sender)) BETWEEN 1 AND 64),
    on_date      TEXT NOT NULL CHECK (on_date IS date(on_date)),
    pack_version INTEGER NOT NULL DEFAULT 0 CHECK (pack_version >= 0),
    tally        INTEGER NOT NULL DEFAULT 1 CHECK (tally > 0),
    PRIMARY KEY (sender, on_date, pack_version)
) STRICT;

-- --------------------------------------------------------------------------- reference ----

-- Rates for reporting. Rows pulled from the server carry origin 'server'; a rate the user
-- types in is theirs alone and is never pushed back.
CREATE TABLE fx_rates (
    base        TEXT NOT NULL CHECK (base IN ('GHS', 'NGN', 'XOF', 'XAF', 'KES', 'TZS', 'UGX', 'RWF', 'ZAR', 'EGP', 'MAD', 'USD', 'EUR', 'GBP', 'CAD', 'AUD', 'CHF', 'CNY', 'INR', 'JPY', 'AED')),
    quote       TEXT NOT NULL CHECK (quote IN ('GHS', 'NGN', 'XOF', 'XAF', 'KES', 'TZS', 'UGX', 'RWF', 'ZAR', 'EGP', 'MAD', 'USD', 'EUR', 'GBP', 'CAD', 'AUD', 'CHF', 'CNY', 'INR', 'JPY', 'AED')),
    rate_e8     INTEGER NOT NULL CHECK (rate_e8 > 0),
    as_of       TEXT NOT NULL CHECK (as_of IS date(as_of)),
    origin      TEXT NOT NULL DEFAULT 'server' CHECK (origin IN ('server', 'user')),
    PRIMARY KEY (base, quote, as_of),
    CONSTRAINT fx_rates_distinct_pair CHECK (base <> quote)
) STRICT;

-- ------------------------------------------------------------------------------- local ----

-- One row. Everything the sync client needs to resume, and nothing a report reads.
CREATE TABLE sync_state (
    id              INTEGER NOT NULL PRIMARY KEY CHECK (id = 1),
    device_id       TEXT NOT NULL,
    -- Highest server_seq pulled. Server-assigned and monotonic in commit order, so a skewed
    -- device clock cannot make this device skip rows.
    pulled_through  INTEGER NOT NULL DEFAULT 0,
    last_pull_at    INTEGER,
    last_push_at    INTEGER,
    -- Active rule pack, so the app knows whether a fetch is needed.
    pack_version    INTEGER
) STRICT;

-- Key/value for user preferences that are not ledger data: default account, reporting
-- currency, theme. Not synced.
CREATE TABLE settings (
    key         TEXT NOT NULL PRIMARY KEY,
    value       TEXT NOT NULL,
    updated_at  INTEGER NOT NULL
) STRICT;

PRAGMA user_version = 1;
