-- moneytrack server schema, revision 2: statement import and the review inbox.
--
-- Revision 1 kept the inbox on the device alone, so that no message body and no unconfirmed
-- SMS could reach the server. The web client changed who holds an imported statement: a
-- person uploads it to us, so the rows waiting for their review have to wait here.
-- docs/decisions/0003-server-inbox.md argues the change.
--
-- Rule 4 of revision 1 still holds, and this file keeps it structurally. The server's inbox
-- accepts a row only from a statement the person uploaded (`csv` or `ofx`) — never `sms`,
-- never `receipt` — and, like the device's, it has no column a message body or a raw
-- statement line could be put in. What a parser produced is kept; what it read is not.

BEGIN;

-- ------------------------------------------------------------------------------- inbox ----

CREATE TABLE inbox_items (
    user_id                 UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    id                      UUID NOT NULL,
    -- Uploaded statements only. An SMS candidate is parsed and reviewed on the handset; the
    -- server receives the confirmed row or nothing.
    source                  TEXT NOT NULL CHECK (source IN ('csv', 'ofx')),
    received_at             ledger_instant NOT NULL,
    -- One per upload. Rows accepted from it carry it in their source_ref, which is how the
    -- duplicate check knows not to offer one line of a statement as a copy of another line
    -- of the same statement: two identical fares on one statement are two fares, and the
    -- fingerprint has already told them apart.
    import_id               UUID NOT NULL,
    -- The uploaded file's name, so the inbox can say where a row came from.
    origin_ref              TEXT CHECK (char_length(btrim(origin_ref)) BETWEEN 1 AND 200),
    -- Where the row was in that file, so the person can find it there.
    line_no                 INTEGER NOT NULL CHECK (line_no >= 1),
    -- mt-core's digest of what the line says and how many identical lines came before it.
    -- Unique per person across every status, so uploading a statement again, or next
    -- month's overlapping one, offers only the lines not seen before — and a line someone
    -- rejected stays rejected.
    fingerprint             TEXT NOT NULL CHECK (fingerprint ~ '^[0-9a-f]{64}$'),
    -- A statement goes into the one account the person chose for it.
    account_id              UUID NOT NULL,
    direction               TEXT NOT NULL CHECK (direction IN ('in', 'out')),
    amount_minor            amount_minor NOT NULL,
    currency                currency_code NOT NULL,
    booked_at               ledger_instant NOT NULL,
    label                   TEXT NOT NULL CHECK (char_length(btrim(label)) BETWEEN 1 AND 200),
    counterparty            TEXT CHECK (char_length(btrim(counterparty)) BETWEEN 1 AND 120),
    -- What the categorisation ladder proposed when the statement came in, and how sure it
    -- was. The person's choice on accepting is what reaches the ledger.
    category_id             UUID,
    category_confidence_bps basis_points NOT NULL DEFAULT 0,
    external_id             TEXT CHECK (char_length(btrim(external_id)) BETWEEN 1 AND 128),
    reported_balance_minor  BIGINT CHECK (reported_balance_minor
                                BETWEEN -1000000000000000 AND 1000000000000000),
    status                  TEXT NOT NULL DEFAULT 'pending'
                            CHECK (status IN ('pending', 'accepted', 'rejected', 'merged')),
    transaction_id          UUID,
    duplicate_of            UUID,
    created_at              ledger_instant NOT NULL,
    updated_at              ledger_instant NOT NULL,
    PRIMARY KEY (user_id, id),
    CONSTRAINT inbox_items_timeline CHECK (updated_at >= created_at),
    CONSTRAINT inbox_accepted_has_transaction
        CHECK ((status = 'accepted') = (transaction_id IS NOT NULL)),
    CONSTRAINT inbox_merged_has_duplicate
        CHECK ((status = 'merged') = (duplicate_of IS NOT NULL)),
    -- The same currency pin as a transaction's: a row waiting to go into a GHS account is
    -- in GHS.
    FOREIGN KEY (user_id, account_id, currency)
        REFERENCES accounts (user_id, id, currency) ON DELETE CASCADE,
    FOREIGN KEY (user_id, category_id) REFERENCES categories (user_id, id),
    -- Ledger rows are soft-deleted, so neither of these is ever orphaned by a delete.
    FOREIGN KEY (user_id, transaction_id) REFERENCES transactions (user_id, id),
    FOREIGN KEY (user_id, duplicate_of) REFERENCES transactions (user_id, id)
);

-- "Have I seen this line already?"
CREATE UNIQUE INDEX inbox_items_fingerprint ON inbox_items (user_id, fingerprint);
-- The inbox screen: what is still waiting, oldest statement line first.
CREATE INDEX inbox_items_waiting ON inbox_items (user_id, booked_at)
    WHERE status = 'pending';

-- ---------------------------------------------------------------------- import mappings ----

-- Which column of an account's CSV statements holds what, kept so the person says it once.
-- The shape is mt-core's `ingest::csv::Mapping`, serialised by serde; it is validated there
-- on every read, so a mapping this build cannot use is refused rather than half-applied.
CREATE TABLE import_mappings (
    user_id     UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    account_id  UUID NOT NULL,
    mapping     JSONB NOT NULL CHECK (jsonb_typeof(mapping) = 'object'
                                      AND octet_length(mapping::text) <= 4096),
    updated_at  ledger_instant NOT NULL,
    PRIMARY KEY (user_id, account_id),
    FOREIGN KEY (user_id, account_id) REFERENCES accounts (user_id, id) ON DELETE CASCADE
);

COMMIT;
