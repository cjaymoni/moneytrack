-- moneytrack server schema, revision 1.
--
-- Postgres behind mt-server. The device schema in db/sqlite/0001_init.sql holds the same
-- tables for one person; docs/SCHEMA.md maps one onto the other and records where they
-- differ on purpose.
--
-- Four rules this file exists to enforce structurally rather than by discipline:
--
--  1. Multi-tenancy. Every per-person table is keyed on (user_id, id) and every foreign key
--     carries user_id through, so a transaction physically cannot reference another user's
--     account. Retrofitting an owner check onto each query later is how IDOR bugs ship.
--  2. Money is BIGINT minor units plus an ISO code from a fixed list. There is no NUMERIC
--     and no DOUBLE PRECISION in this file, and no stored balance anywhere: balances are
--     derived by the walk in mt-core.
--  3. A row's currency always matches its account's, enforced by a composite foreign key,
--     because a USD row in a GHS account corrupts the one number the product exists to show.
--  4. No raw SMS. There is nowhere in this schema to put a message body. Parsing happens on
--     the device and only a confirmed transaction ever reaches the server.

BEGIN;

CREATE EXTENSION IF NOT EXISTS citext;
-- Lets the budgets exclusion constraint mix plain equality with range overlap.
CREATE EXTENSION IF NOT EXISTS btree_gist;

-- ------------------------------------------------------------------------- vocabulary ----

-- One definition of what a currency code is, shared by every money column. The list is the
-- registry in crates/core/src/money.rs, and crates/core/tests/schema_agreement.rs fails if
-- the two drift apart. A code outside it would deserialise into nothing mt-core can hold.
CREATE DOMAIN currency_code AS CHAR(3)
    CHECK (VALUE IN ('GHS', 'NGN', 'XOF', 'XAF', 'KES', 'TZS', 'UGX', 'RWF', 'ZAR', 'EGP', 'MAD', 'USD', 'EUR', 'GBP', 'CAD', 'AUD', 'CHF', 'CNY', 'INR', 'JPY', 'AED'));

-- Minor units, non-negative, and bounded well below the i64 ceiling. The bound is a guard,
-- not a currency limit: one row near 9.2 quintillion makes every balance walk over its
-- account return Overflow, with nothing in the error to say which row caused it.
CREATE DOMAIN amount_minor AS BIGINT
    CHECK (VALUE >= 0 AND VALUE <= 1000000000000000);

-- Future-dated rows are the forecast, but a forecast reaching the twenty-third century is a
-- corrupt date rather than a plan.
CREATE DOMAIN ledger_instant AS TIMESTAMPTZ
    CHECK (VALUE >= TIMESTAMPTZ '1900-01-01 00:00:00+00'
       AND VALUE <= TIMESTAMPTZ '2200-01-01 00:00:00+00');

-- Fractions of a whole. 8000 is 80%.
CREATE DOMAIN basis_points AS INTEGER
    CHECK (VALUE BETWEEN 0 AND 10000);

-- ---------------------------------------------------------------------------- identity ----

CREATE TABLE users (
    id              UUID PRIMARY KEY,
    email           CITEXT NOT NULL CHECK (char_length(email) BETWEEN 3 AND 320),
    password_hash   TEXT NOT NULL CHECK (char_length(password_hash) BETWEEN 1 AND 512),
    -- Per-user sync counter. See next_sync_seq() below for why this is a column on a row
    -- the push transaction locks, rather than a shared sequence.
    sync_seq        BIGINT NOT NULL DEFAULT 0,
    email_verified_at TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    CONSTRAINT users_email_shape CHECK (email ~ '^[^@[:space:]]+@[^@[:space:]]+$')
);

-- Deleted accounts release their address so the person can sign up again.
CREATE UNIQUE INDEX users_email_live ON users (email) WHERE deleted_at IS NULL;

CREATE TABLE devices (
    user_id         UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    id              UUID NOT NULL,
    name            TEXT NOT NULL CHECK (char_length(btrim(name)) BETWEEN 1 AND 64),
    platform        TEXT NOT NULL CHECK (platform IN ('android', 'ios', 'macos', 'windows', 'linux', 'web')),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at    TIMESTAMPTZ,
    -- Highest sync_seq this device has pulled. The server's half of the watermark.
    pulled_through  BIGINT NOT NULL DEFAULT 0,
    revoked_at      TIMESTAMPTZ,
    PRIMARY KEY (user_id, id)
);

CREATE TABLE refresh_tokens (
    id              UUID PRIMARY KEY,
    user_id         UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    device_id       UUID,
    -- SHA-256 of the token. The token itself is never stored, so a database leak does not
    -- hand over live sessions.
    token_hash      BYTEA NOT NULL,
    issued_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at      TIMESTAMPTZ NOT NULL,
    revoked_at      TIMESTAMPTZ,
    -- Set when this token replaced another. Reuse of a rotated token means theft: the whole
    -- chain is revoked.
    rotated_from    UUID REFERENCES refresh_tokens (id) ON DELETE SET NULL,
    CONSTRAINT refresh_tokens_window CHECK (expires_at > issued_at),
    FOREIGN KEY (user_id, device_id) REFERENCES devices (user_id, id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX refresh_tokens_hash ON refresh_tokens (token_hash);
CREATE INDEX refresh_tokens_live ON refresh_tokens (user_id, expires_at) WHERE revoked_at IS NULL;

-- ------------------------------------------------------------------------ sync paging ----

-- Hands out the next sync number for one user, and -- the point of the exercise -- holds
-- that user's row locked until the transaction commits.
--
-- The obvious implementation, `DEFAULT nextval('a_sequence')`, is wrong twice over. It fires
-- only on INSERT, so an edit or a soft delete keeps its old number and never reaches another
-- device, which is exactly the resurrection the soft-delete design exists to prevent. And a
-- sequence hands out numbers in the order transactions *start*, while rows become visible in
-- the order they *commit*: a slow writer takes 21, a fast writer takes 22 and commits first,
-- a client pulls 22, and 21 is never returned by `server_seq > watermark` again. A silently
-- missing transaction is the worst failure this product has.
--
-- Locking the user row makes sequence order equal commit order for that user, which is the
-- only ordering a client's watermark depends on. Different users never block each other.
CREATE FUNCTION next_sync_seq(p_user_id UUID) RETURNS BIGINT AS $$
DECLARE
    v_seq BIGINT;
BEGIN
    UPDATE users SET sync_seq = sync_seq + 1 WHERE id = p_user_id RETURNING sync_seq INTO v_seq;
    IF v_seq IS NULL THEN
        RAISE EXCEPTION 'cannot stamp a sync sequence for unknown user %', p_user_id;
    END IF;
    RETURN v_seq;
END;
$$ LANGUAGE plpgsql;

-- Stamps every insert and every update, so a client cannot set its own server_seq and a
-- handler cannot forget to bump one.
CREATE FUNCTION stamp_sync_seq() RETURNS TRIGGER AS $$
BEGIN
    NEW.server_seq := next_sync_seq(NEW.user_id);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- ------------------------------------------------------------------------------ ledger ----

CREATE TABLE accounts (
    user_id             UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    id                  UUID NOT NULL,
    name                TEXT NOT NULL CHECK (char_length(btrim(name)) BETWEEN 1 AND 64),
    kind                TEXT NOT NULL CHECK (kind IN ('cash', 'momo', 'bank', 'card')),
    -- One currency per account, fixed at creation. An account holding two currencies is two
    -- accounts; conflating account, container and currency is what blocks multi-currency
    -- reporting later.
    currency            currency_code NOT NULL,
    opening_balance_minor BIGINT NOT NULL DEFAULT 0
        CHECK (opening_balance_minor BETWEEN -1000000000000000 AND 1000000000000000),
    sort_order          INTEGER NOT NULL DEFAULT 0,
    archived_at         TIMESTAMPTZ,
    created_at          ledger_instant NOT NULL,
    updated_at          ledger_instant NOT NULL,
    deleted_at          TIMESTAMPTZ,
    device_id           UUID NOT NULL,
    server_seq          BIGINT NOT NULL,
    PRIMARY KEY (user_id, id),
    CONSTRAINT accounts_timeline CHECK (updated_at >= created_at
        AND (deleted_at IS NULL OR deleted_at >= created_at)),
    -- The parent side of the composite foreign key that pins a row's currency to its
    -- account's. Redundant with the primary key, and that is what makes the key below legal.
    CONSTRAINT accounts_currency_key UNIQUE (user_id, id, currency)
);

CREATE INDEX accounts_sync ON accounts (user_id, server_seq);
CREATE TRIGGER accounts_stamp BEFORE INSERT OR UPDATE ON accounts
    FOR EACH ROW EXECUTE FUNCTION stamp_sync_seq();

CREATE TABLE categories (
    user_id         UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    id              UUID NOT NULL,
    name            TEXT NOT NULL CHECK (char_length(btrim(name)) BETWEEN 1 AND 64),
    parent_id       UUID,
    kind            TEXT NOT NULL CHECK (kind IN ('income', 'expense')),
    icon            TEXT CHECK (char_length(btrim(icon)) BETWEEN 1 AND 64),
    is_system       BOOLEAN NOT NULL DEFAULT FALSE,
    sort_order      INTEGER NOT NULL DEFAULT 0,
    created_at      ledger_instant NOT NULL,
    updated_at      ledger_instant NOT NULL,
    deleted_at      TIMESTAMPTZ,
    device_id       UUID NOT NULL,
    server_seq      BIGINT NOT NULL,
    PRIMARY KEY (user_id, id),
    CONSTRAINT categories_timeline CHECK (updated_at >= created_at
        AND (deleted_at IS NULL OR deleted_at >= created_at)),
    -- One level of nesting is the product rule; SQL can refuse self-parenting but not a
    -- grandparent chain, so Category::validate_under carries the rest.
    CONSTRAINT categories_not_own_parent CHECK (parent_id IS DISTINCT FROM id),
    FOREIGN KEY (user_id, parent_id) REFERENCES categories (user_id, id) ON DELETE SET NULL
);

CREATE INDEX categories_sync ON categories (user_id, server_seq);
CREATE INDEX categories_parent ON categories (user_id, parent_id) WHERE parent_id IS NOT NULL;
CREATE TRIGGER categories_stamp BEFORE INSERT OR UPDATE ON categories
    FOR EACH ROW EXECUTE FUNCTION stamp_sync_seq();

CREATE TABLE recurring (
    user_id         UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    id              UUID NOT NULL,
    name            TEXT NOT NULL CHECK (char_length(btrim(name)) BETWEEN 1 AND 64),
    account_id      UUID NOT NULL,
    direction       TEXT NOT NULL CHECK (direction IN ('in', 'out')),
    amount_minor    amount_minor NOT NULL,
    currency        currency_code NOT NULL,
    category_id     UUID,
    label           TEXT NOT NULL CHECK (char_length(btrim(label)) BETWEEN 1 AND 200),
    counterparty    TEXT CHECK (char_length(btrim(counterparty)) BETWEEN 1 AND 120),
    freq            TEXT NOT NULL CHECK (freq IN ('daily', 'weekly', 'monthly', 'yearly')),
    interval        INTEGER NOT NULL CHECK (interval >= 1),
    by_month_day    SMALLINT CHECK (by_month_day BETWEEN 1 AND 31),
    by_weekday      SMALLINT CHECK (by_weekday BETWEEN 0 AND 6),
    starts_on       DATE NOT NULL,
    ends_on         DATE,
    -- How far ahead concrete pending rows have been generated, so generation is idempotent.
    generated_through DATE,
    timezone        TEXT NOT NULL DEFAULT 'Africa/Accra'
        CHECK (char_length(btrim(timezone)) BETWEEN 1 AND 64),
    created_at      ledger_instant NOT NULL,
    updated_at      ledger_instant NOT NULL,
    deleted_at      TIMESTAMPTZ,
    device_id       UUID NOT NULL,
    server_seq      BIGINT NOT NULL,
    PRIMARY KEY (user_id, id),
    CONSTRAINT recurring_timeline CHECK (updated_at >= created_at
        AND (deleted_at IS NULL OR deleted_at >= created_at)),
    CONSTRAINT recurring_window CHECK (ends_on IS NULL OR ends_on >= starts_on),
    -- A cadence and its day fields have to agree, or rows generate on the wrong date.
    CONSTRAINT recurring_cadence CHECK (
        CASE freq
            WHEN 'daily'   THEN by_month_day IS NULL AND by_weekday IS NULL
            WHEN 'weekly'  THEN by_weekday IS NOT NULL AND by_month_day IS NULL
            ELSE by_month_day IS NOT NULL AND by_weekday IS NULL
        END),
    FOREIGN KEY (user_id, account_id, currency) REFERENCES accounts (user_id, id, currency),
    FOREIGN KEY (user_id, category_id) REFERENCES categories (user_id, id) ON DELETE SET NULL
);

CREATE INDEX recurring_sync ON recurring (user_id, server_seq);
CREATE TRIGGER recurring_stamp BEFORE INSERT OR UPDATE ON recurring
    FOR EACH ROW EXECUTE FUNCTION stamp_sync_seq();

CREATE TABLE transactions (
    user_id         UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    id              UUID NOT NULL,
    account_id      UUID NOT NULL,
    -- No 'transfer' direction. A transfer is two rows, an 'out' and an 'in', sharing a
    -- transfer_id; the balance walk nets them, so moving money between your own accounts is
    -- neither income nor spending. "Exactly two legs, opposite directions, same amount" is
    -- not expressible here, because the legs arrive separately over sync; mt-core's
    -- validate_transfer_pair carries it.
    direction       TEXT NOT NULL CHECK (direction IN ('in', 'out')),
    -- Always non-negative. The sign lives in direction.
    amount_minor    amount_minor NOT NULL,
    -- The currency the movement happened in, kept for ever. Pinned to the account's
    -- currency by the composite foreign key below.
    currency        currency_code NOT NULL,
    -- May be in the future. A future-dated pending row is the forecast; the balance walk
    -- sorts by this column and does not care which side of today a row falls on.
    booked_at       ledger_instant NOT NULL,
    category_id     UUID,
    label           TEXT NOT NULL CHECK (char_length(btrim(label)) BETWEEN 1 AND 200),
    counterparty    TEXT CHECK (char_length(btrim(counterparty)) BETWEEN 1 AND 120),
    note            TEXT CHECK (char_length(btrim(note)) BETWEEN 1 AND 2000),
    status          TEXT NOT NULL CHECK (status IN ('pending', 'confirmed')),
    source          TEXT NOT NULL CHECK (source IN ('manual', 'sms', 'csv', 'ofx', 'receipt', 'recurring', 'adjustment')),
    source_ref      TEXT CHECK (char_length(btrim(source_ref)) BETWEEN 1 AND 200),
    external_id     TEXT CHECK (char_length(btrim(external_id)) BETWEEN 1 AND 128),
    recurring_id    UUID,
    transfer_id     UUID,
    created_at      ledger_instant NOT NULL,
    updated_at      ledger_instant NOT NULL,
    deleted_at      TIMESTAMPTZ,
    device_id       UUID NOT NULL,
    server_seq      BIGINT NOT NULL,
    PRIMARY KEY (user_id, id),
    CONSTRAINT transactions_timeline CHECK (updated_at >= created_at
        AND (deleted_at IS NULL OR deleted_at >= created_at)),
    -- A transfer leg carries no category: it is netted, so categorising it would count the
    -- move as spending.
    CONSTRAINT transactions_transfer_uncategorised
        CHECK (transfer_id IS NULL OR category_id IS NULL),
    FOREIGN KEY (user_id, account_id, currency) REFERENCES accounts (user_id, id, currency),
    FOREIGN KEY (user_id, category_id) REFERENCES categories (user_id, id) ON DELETE SET NULL,
    FOREIGN KEY (user_id, recurring_id) REFERENCES recurring (user_id, id) ON DELETE SET NULL
);

-- The balance walk reads one account in date order; this is the index it walks.
CREATE INDEX transactions_walk ON transactions (user_id, account_id, booked_at)
    WHERE deleted_at IS NULL;
-- "Where did the money go this month" reads every account over a date window.
CREATE INDEX transactions_period ON transactions (user_id, booked_at)
    WHERE deleted_at IS NULL;
CREATE INDEX transactions_sync ON transactions (user_id, server_seq);
-- Not partial on category_id: uncategorised rows are exactly the ones a breakdown most
-- needs to surface, so excluding them would send that query to a sequential scan.
CREATE INDEX transactions_category ON transactions (user_id, category_id, booked_at)
    WHERE deleted_at IS NULL;
CREATE INDEX transactions_transfer ON transactions (user_id, transfer_id)
    WHERE transfer_id IS NOT NULL;
-- An institution's own reference for a movement, where the source carries one: the thing
-- that stops a re-imported statement creating a second copy of every row. Per account,
-- because two banks can easily use the same reference format.
CREATE UNIQUE INDEX transactions_external ON transactions (user_id, account_id, external_id)
    WHERE external_id IS NOT NULL AND deleted_at IS NULL;
-- A recurring template fires at most once per instant. Generation is idempotent through the
-- template's `generated_through` watermark, but that only holds if the rows and the new
-- watermark are written in one transaction; this is the backstop for the case where they are
-- not, which would otherwise show the user next month's rent two or three times over.
CREATE UNIQUE INDEX transactions_recurring_instance
    ON transactions (user_id, recurring_id, booked_at)
    WHERE recurring_id IS NOT NULL AND deleted_at IS NULL;

CREATE TRIGGER transactions_stamp BEFORE INSERT OR UPDATE ON transactions
    FOR EACH ROW EXECUTE FUNCTION stamp_sync_seq();

CREATE TABLE category_rules (
    user_id         UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    id              UUID NOT NULL,
    -- Data, not code. A malformed pattern is rejected when it is saved, loudly; it is never
    -- compiled with errors suppressed and quietly skipped at match time.
    pattern         TEXT NOT NULL CHECK (char_length(btrim(pattern)) BETWEEN 1 AND 500),
    matched_field   TEXT NOT NULL CHECK (matched_field IN ('label', 'counterparty', 'raw_text')),
    category_id     UUID NOT NULL,
    priority        INTEGER NOT NULL DEFAULT 100,
    is_regex        BOOLEAN NOT NULL DEFAULT FALSE,
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    origin          TEXT NOT NULL DEFAULT 'user' CHECK (origin IN ('user', 'pack')),
    pack_id         UUID,
    created_at      ledger_instant NOT NULL,
    updated_at      ledger_instant NOT NULL,
    deleted_at      TIMESTAMPTZ,
    device_id       UUID NOT NULL,
    server_seq      BIGINT NOT NULL,
    PRIMARY KEY (user_id, id),
    CONSTRAINT category_rules_timeline CHECK (updated_at >= created_at
        AND (deleted_at IS NULL OR deleted_at >= created_at)),
    CONSTRAINT category_rules_pack_origin
        CHECK ((origin = 'pack') = (pack_id IS NOT NULL)),
    FOREIGN KEY (user_id, category_id) REFERENCES categories (user_id, id) ON DELETE CASCADE
);

-- Matching order: lowest priority first, then id, so two rules of equal priority always
-- resolve the same way.
CREATE INDEX category_rules_order ON category_rules (user_id, priority, id)
    WHERE deleted_at IS NULL AND enabled;
CREATE INDEX category_rules_sync ON category_rules (user_id, server_seq);
CREATE TRIGGER category_rules_stamp BEFORE INSERT OR UPDATE ON category_rules
    FOR EACH ROW EXECUTE FUNCTION stamp_sync_seq();

CREATE TABLE budgets (
    user_id         UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    id              UUID NOT NULL,
    -- NULL caps all spending in the currency rather than one category.
    category_id     UUID,
    period          TEXT NOT NULL CHECK (period IN ('weekly', 'monthly', 'yearly')),
    cap_minor       amount_minor NOT NULL CHECK (cap_minor > 0),
    -- Caps are per currency by design: a cedi cap and a dollar cap on the same category are
    -- two budgets, not one converted number, so last month's verdict does not move when the
    -- rate does.
    currency        currency_code NOT NULL,
    starts_on       DATE NOT NULL,
    ends_on         DATE,
    warn_at_bps     basis_points NOT NULL DEFAULT 8000,
    created_at      ledger_instant NOT NULL,
    updated_at      ledger_instant NOT NULL,
    deleted_at      TIMESTAMPTZ,
    device_id       UUID NOT NULL,
    server_seq      BIGINT NOT NULL,
    PRIMARY KEY (user_id, id),
    CONSTRAINT budgets_timeline CHECK (updated_at >= created_at
        AND (deleted_at IS NULL OR deleted_at >= created_at)),
    CONSTRAINT budgets_window CHECK (ends_on IS NULL OR ends_on >= starts_on),
    -- "Am I over budget" must have exactly one answer, which means no two live budgets for
    -- the same scope over overlapping dates. Not a plain unique key: that would also stop
    -- you setting next year's cap while this year's is still live, and would force you to
    -- delete the old cap -- the one last quarter's verdict was measured against -- to raise
    -- it.
    CONSTRAINT budgets_no_overlapping_scope EXCLUDE USING gist (
        user_id WITH =,
        (COALESCE(category_id, '00000000-0000-0000-0000-000000000000'::uuid)) WITH =,
        (currency::text) WITH =,
        period WITH =,
        daterange(starts_on, COALESCE(ends_on, DATE '9999-12-31'), '[]') WITH &&
    ) WHERE (deleted_at IS NULL),
    FOREIGN KEY (user_id, category_id) REFERENCES categories (user_id, id) ON DELETE CASCADE
);

CREATE INDEX budgets_sync ON budgets (user_id, server_seq);
CREATE TRIGGER budgets_stamp BEFORE INSERT OR UPDATE ON budgets
    FOR EACH ROW EXECUTE FUNCTION stamp_sync_seq();

CREATE TABLE attachments (
    user_id         UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    id              UUID NOT NULL,
    transaction_id  UUID NOT NULL,
    -- Path within the user's attachment area, resolved by joining it to a root the app
    -- owns. An absolute path or a `..` segment turns that join into a read somewhere else,
    -- and the value arrives over sync from another device.
    relative_path   TEXT NOT NULL CHECK (
        char_length(relative_path) BETWEEN 1 AND 512
        AND relative_path !~ '^/'
        AND relative_path !~ '(^|/)\.\.(/|$)'
        AND relative_path !~ '[\\\x00]'
        AND relative_path = btrim(relative_path)),
    media_type      TEXT NOT NULL CHECK (char_length(btrim(media_type)) BETWEEN 1 AND 128),
    byte_size       BIGINT NOT NULL CHECK (byte_size >= 0),
    sha256          CHAR(64) NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    created_at      ledger_instant NOT NULL,
    updated_at      ledger_instant NOT NULL,
    deleted_at      TIMESTAMPTZ,
    device_id       UUID NOT NULL,
    server_seq      BIGINT NOT NULL,
    PRIMARY KEY (user_id, id),
    CONSTRAINT attachments_timeline CHECK (updated_at >= created_at
        AND (deleted_at IS NULL OR deleted_at >= created_at)),
    FOREIGN KEY (user_id, transaction_id) REFERENCES transactions (user_id, id) ON DELETE CASCADE
);

CREATE INDEX attachments_sync ON attachments (user_id, server_seq);
CREATE INDEX attachments_transaction ON attachments (user_id, transaction_id);
CREATE TRIGGER attachments_stamp BEFORE INSERT OR UPDATE ON attachments
    FOR EACH ROW EXECUTE FUNCTION stamp_sync_seq();

-- -------------------------------------------------------------------- reference data ----

-- Shared, not per-user: reporting rates the server publishes and every device copies. A
-- rate the user types in themselves is a device row (see the SQLite schema) and does not
-- sync, because it is their opinion of a rate, not ours.
--
-- Scaled integer, never a float: rate = rate_e8 / 100000000.
CREATE TABLE fx_rates (
    base        currency_code NOT NULL,
    quote       currency_code NOT NULL,
    rate_e8     BIGINT NOT NULL CHECK (rate_e8 > 0),
    as_of       DATE NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (base, quote, as_of),
    CONSTRAINT fx_rates_distinct_pair CHECK (base <> quote)
);

-- Versioned SMS patterns and default categories, fetched at runtime so supporting a new
-- Ghanaian bank is a server release rather than a store review.
CREATE TABLE rule_packs (
    id              UUID PRIMARY KEY,
    version         INTEGER NOT NULL CHECK (version > 0),
    channel         TEXT NOT NULL DEFAULT 'stable' CHECK (channel IN ('stable', 'beta')),
    -- Patterns and categories as data. Validated against the pack schema before publishing;
    -- a pattern that does not compile fails here, not on someone's phone.
    manifest        JSONB NOT NULL,
    -- Detached signature over the manifest bytes. Clients verify before applying.
    signature       BYTEA NOT NULL,
    published_at    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX rule_packs_version ON rule_packs (channel, version);

-- A message the device could not parse, redacted on the device and sent only because the
-- user pressed the button and saw exactly what would be sent. There is no automatic upload
-- of unmatched text; that is precisely what the Play policy forbids.
CREATE TABLE redacted_samples (
    id              UUID PRIMARY KEY,
    -- Deliberately not a foreign key to users: the sample is kept detached from the account
    -- that submitted it, so a pattern-writing session is not a window onto a person.
    sender          TEXT NOT NULL CHECK (char_length(btrim(sender)) BETWEEN 1 AND 64),
    -- Digits and names already stripped on the device before this ever left the handset.
    redacted_body   TEXT NOT NULL CHECK (char_length(btrim(redacted_body)) BETWEEN 1 AND 1000),
    app_version     TEXT NOT NULL CHECK (char_length(btrim(app_version)) BETWEEN 1 AND 64),
    pack_version    INTEGER,
    received_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at     TIMESTAMPTZ
);

CREATE INDEX redacted_samples_open ON redacted_samples (sender, received_at)
    WHERE resolved_at IS NULL;

COMMIT;
