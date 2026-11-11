-- Proves the constraints that matter actually bite. Run against a database with
-- db/postgres/0001_init.sql applied; it rolls everything back.
--
--   psql -v ON_ERROR_STOP=1 -f db/tests/postgres_constraints.sql
--
-- Commit-order sync paging is checked separately, by db/tests/postgres_sync_ordering.sh,
-- because it needs two concurrent connections.

BEGIN;

CREATE OR REPLACE FUNCTION must_fail(stmt TEXT, what TEXT) RETURNS VOID AS $$
BEGIN
    BEGIN
        EXECUTE stmt;
    EXCEPTION WHEN others THEN
        RAISE NOTICE 'ok: %', what;
        RETURN;
    END;
    RAISE EXCEPTION 'expected failure but statement succeeded: %', what;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION must_equal(query TEXT, expected TEXT, what TEXT) RETURNS VOID AS $$
DECLARE actual TEXT;
BEGIN
    EXECUTE query INTO actual;
    IF actual IS DISTINCT FROM expected THEN
        RAISE EXCEPTION '% — expected %, got %', what, expected, actual;
    END IF;
    RAISE NOTICE 'ok: %', what;
END;
$$ LANGUAGE plpgsql;

INSERT INTO users (id, email, password_hash) VALUES
    ('11111111-1111-1111-1111-111111111111', 'ama@example.test', 'argon2id$placeholder'),
    ('22222222-2222-2222-2222-222222222222', 'kofi@example.test', 'argon2id$placeholder');

INSERT INTO devices (user_id, id, name, platform) VALUES
    ('11111111-1111-1111-1111-111111111111', 'aaaaaaaa-0000-0000-0000-000000000001', 'Ama phone', 'android'),
    ('22222222-2222-2222-2222-222222222222', 'bbbbbbbb-0000-0000-0000-000000000001', 'Kofi phone', 'android');

INSERT INTO accounts (user_id, id, name, kind, currency, created_at, updated_at, device_id) VALUES
    ('11111111-1111-1111-1111-111111111111', 'a0000000-0000-0000-0000-000000000001', 'MoMo', 'momo', 'GHS', now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001'),
    ('11111111-1111-1111-1111-111111111111', 'a0000000-0000-0000-0000-000000000002', 'Dollar account', 'bank', 'USD', now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001'),
    ('22222222-2222-2222-2222-222222222222', 'b0000000-0000-0000-0000-000000000001', 'MoMo', 'momo', 'GHS', now(), now(), 'bbbbbbbb-0000-0000-0000-000000000001');

INSERT INTO categories (user_id, id, name, kind, created_at, updated_at, device_id) VALUES
    ('11111111-1111-1111-1111-111111111111', 'c0000000-0000-0000-0000-000000000001', 'Transport', 'expense', now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001');

-- The happy path, so the failures below mean something.
INSERT INTO transactions (user_id, id, account_id, direction, amount_minor, currency, booked_at,
                          label, status, source, created_at, updated_at, device_id, external_id)
VALUES ('11111111-1111-1111-1111-111111111111', '70000000-0000-0000-0000-000000000001',
        'a0000000-0000-0000-0000-000000000001', 'out', 2000, 'GHS', now(),
        'Trotro', 'confirmed', 'sms', now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001', 'MTN-REF-1');

-- ------------------------------------------------------------------ tenant isolation ----

-- Ama cannot book a transaction against Kofi's account. This is the whole reason the
-- primary key is (user_id, id) and the foreign key carries user_id through.
SELECT must_fail($$
    INSERT INTO transactions (user_id, id, account_id, direction, amount_minor, currency,
                              booked_at, label, status, source, created_at, updated_at, device_id)
    VALUES ('11111111-1111-1111-1111-111111111111', '70000000-0000-0000-0000-00000000000e',
            'b0000000-0000-0000-0000-000000000001', 'out', 100, 'GHS', now(),
            'Cross-tenant', 'confirmed', 'manual', now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001');
$$, 'a transaction cannot reference another user''s account');

SELECT must_fail($$
    INSERT INTO budgets (user_id, id, category_id, period, cap_minor, currency, starts_on,
                         created_at, updated_at, device_id)
    VALUES ('22222222-2222-2222-2222-222222222222', 'd0000000-0000-0000-0000-00000000000e',
            'c0000000-0000-0000-0000-000000000001', 'monthly', 1000, 'GHS', DATE '2026-11-01',
            now(), now(), 'bbbbbbbb-0000-0000-0000-000000000001');
$$, 'a budget cannot reference another user''s category');

-- ----------------------------------------------------------------- ledger invariants ----

-- A USD row in a GHS account makes the balance walk either fail for ever or sum two
-- currencies. The composite foreign key stops it in the schema, not in a handler.
SELECT must_fail($$
    INSERT INTO transactions (user_id, id, account_id, direction, amount_minor, currency,
                              booked_at, label, status, source, created_at, updated_at, device_id)
    VALUES ('11111111-1111-1111-1111-111111111111', '70000000-0000-0000-0000-00000000000f',
            'a0000000-0000-0000-0000-000000000001', 'out', 100, 'USD', now(),
            'Dollars in a cedi account', 'confirmed', 'manual', now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001');
$$, 'a transaction cannot be in a different currency from its account');

INSERT INTO transactions (user_id, id, account_id, direction, amount_minor, currency, booked_at,
                          label, status, source, created_at, updated_at, device_id)
VALUES ('11111111-1111-1111-1111-111111111111', '70000000-0000-0000-0000-000000000010',
        'a0000000-0000-0000-0000-000000000002', 'out', 100, 'USD', now(),
        'Dollars in a dollar account', 'confirmed', 'manual', now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001');

SELECT must_fail($$
    INSERT INTO accounts (user_id, id, name, kind, currency, created_at, updated_at, device_id)
    VALUES ('11111111-1111-1111-1111-111111111111', 'a0000000-0000-0000-0000-00000000000a',
            'Zed', 'cash', 'ZZZ', now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001');
$$, 'a currency outside the registry is refused');

SELECT must_fail($$
    INSERT INTO accounts (user_id, id, name, kind, currency, created_at, updated_at, device_id)
    VALUES ('11111111-1111-1111-1111-111111111111', 'a0000000-0000-0000-0000-00000000000b',
            '   ', 'cash', 'GHS', now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001');
$$, 'a name of nothing but whitespace is refused, as on the device');

SELECT must_fail($$
    INSERT INTO transactions (user_id, id, account_id, direction, amount_minor, currency,
                              booked_at, label, status, source, created_at, updated_at, device_id)
    VALUES ('11111111-1111-1111-1111-111111111111', '70000000-0000-0000-0000-00000000000d',
            'a0000000-0000-0000-0000-000000000001', 'out', -100, 'GHS', now(),
            'Negative', 'confirmed', 'manual', now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001');
$$, 'a negative amount is refused; the sign lives in direction');

SELECT must_fail($$
    INSERT INTO transactions (user_id, id, account_id, direction, amount_minor, currency,
                              booked_at, label, status, source, created_at, updated_at, device_id)
    VALUES ('11111111-1111-1111-1111-111111111111', '70000000-0000-0000-0000-00000000001a',
            'a0000000-0000-0000-0000-000000000001', 'out', 9223372036854775807, 'GHS', now(),
            'Absurd', 'confirmed', 'manual', now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001');
$$, 'an amount large enough to overflow a balance walk is refused');

SELECT must_fail($$
    INSERT INTO transactions (user_id, id, account_id, direction, amount_minor, currency,
                              booked_at, label, status, source, created_at, updated_at, device_id)
    VALUES ('11111111-1111-1111-1111-111111111111', '70000000-0000-0000-0000-00000000001b',
            'a0000000-0000-0000-0000-000000000001', 'out', 100, 'GHS', TIMESTAMPTZ '2500-01-01+00',
            'Far future', 'confirmed', 'manual', now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001');
$$, 'a booking date in the twenty-sixth century is refused');

SELECT must_fail($$
    INSERT INTO transactions (user_id, id, account_id, direction, amount_minor, currency,
                              booked_at, label, status, source, created_at, updated_at, device_id)
    VALUES ('11111111-1111-1111-1111-111111111111', '70000000-0000-0000-0000-00000000001c',
            'a0000000-0000-0000-0000-000000000001', 'out', 100, 'GHS', now(),
            'Backwards', 'confirmed', 'manual', now(), now() - INTERVAL '1 hour', 'aaaaaaaa-0000-0000-0000-000000000001');
$$, 'updated_at before created_at is refused');

SELECT must_fail($$
    INSERT INTO transactions (user_id, id, account_id, direction, amount_minor, currency,
                              booked_at, label, status, source, created_at, updated_at, device_id,
                              transfer_id, category_id)
    VALUES ('11111111-1111-1111-1111-111111111111', '70000000-0000-0000-0000-00000000000c',
            'a0000000-0000-0000-0000-000000000001', 'out', 100, 'GHS', now(),
            'Move to bank', 'confirmed', 'manual', now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001',
            'f0000000-0000-0000-0000-000000000001', 'c0000000-0000-0000-0000-000000000001');
$$, 'a transfer leg cannot carry a category');

SELECT must_fail($$
    INSERT INTO transactions (user_id, id, account_id, direction, amount_minor, currency,
                              booked_at, label, status, source, created_at, updated_at, device_id, external_id)
    VALUES ('11111111-1111-1111-1111-111111111111', '70000000-0000-0000-0000-00000000000b',
            'a0000000-0000-0000-0000-000000000001', 'out', 2000, 'GHS', now(),
            'Trotro again', 'confirmed', 'csv', now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001', 'MTN-REF-1');
$$, 'the same institution reference cannot be imported into an account twice');

SELECT must_fail($$
    INSERT INTO transactions (user_id, id, account_id, direction, amount_minor, currency,
                              booked_at, label, status, source, created_at, updated_at, device_id)
    VALUES ('11111111-1111-1111-1111-111111111111', '70000000-0000-0000-0000-00000000000a',
            'a0000000-0000-0000-0000-000000000001', 'transfer', 100, 'GHS', now(),
            'Bad direction', 'confirmed', 'manual', now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001');
$$, 'there is no transfer direction');

-- ------------------------------------------------------------------------ recurrence ----

INSERT INTO recurring (user_id, id, name, account_id, direction, amount_minor, currency, label,
                       freq, interval, by_month_day, starts_on, created_at, updated_at, device_id)
VALUES ('11111111-1111-1111-1111-111111111111', 'e1000000-0000-0000-0000-000000000001', 'Rent',
        'a0000000-0000-0000-0000-000000000001', 'out', 150000, 'GHS', 'Rent',
        'monthly', 1, 1, DATE '2026-12-01', now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001');

SELECT must_fail($$
    INSERT INTO recurring (user_id, id, name, account_id, direction, amount_minor, currency, label,
                           freq, interval, by_month_day, starts_on, created_at, updated_at, device_id)
    VALUES ('11111111-1111-1111-1111-111111111111', 'e1000000-0000-0000-0000-000000000002', 'Odd',
            'a0000000-0000-0000-0000-000000000001', 'out', 1000, 'GHS', 'Odd',
            'weekly', 1, 15, DATE '2026-12-01', now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001');
$$, 'a weekly template cannot carry a day of the month');

SELECT must_fail($$
    INSERT INTO recurring (user_id, id, name, account_id, direction, amount_minor, currency, label,
                           freq, interval, starts_on, created_at, updated_at, device_id)
    VALUES ('11111111-1111-1111-1111-111111111111', 'e1000000-0000-0000-0000-000000000003', 'Vague',
            'a0000000-0000-0000-0000-000000000001', 'out', 1000, 'GHS', 'Vague',
            'monthly', 1, DATE '2026-12-01', now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001');
$$, 'a monthly template needs a day of the month');

-- Generation is idempotent through the template's generated_through watermark, but only if
-- the rows and the new watermark are written in one transaction. This is the backstop for
-- when they are not, which would otherwise show the user next month's rent twice over.
INSERT INTO transactions (user_id, id, account_id, direction, amount_minor, currency, booked_at,
                          label, status, source, created_at, updated_at, device_id, recurring_id)
VALUES ('11111111-1111-1111-1111-111111111111', '70000000-0000-0000-0000-00000000001a',
        'a0000000-0000-0000-0000-000000000001', 'out', 150000, 'GHS',
        TIMESTAMPTZ '2026-12-01 00:00:00+00', 'Rent', 'pending', 'recurring', now(), now(),
        'aaaaaaaa-0000-0000-0000-000000000001', 'e1000000-0000-0000-0000-000000000001');

SELECT must_fail($$
    INSERT INTO transactions (user_id, id, account_id, direction, amount_minor, currency,
                              booked_at, label, status, source, created_at, updated_at,
                              device_id, recurring_id)
    VALUES ('11111111-1111-1111-1111-111111111111', '70000000-0000-0000-0000-00000000001b',
            'a0000000-0000-0000-0000-000000000001', 'out', 150000, 'GHS',
            TIMESTAMPTZ '2026-12-01 00:00:00+00', 'Rent', 'pending', 'recurring', now(), now(),
            'aaaaaaaa-0000-0000-0000-000000000001', 'e1000000-0000-0000-0000-000000000001');
$$, 'a template cannot generate the same instant twice');

INSERT INTO transactions (user_id, id, account_id, direction, amount_minor, currency, booked_at,
                          label, status, source, created_at, updated_at, device_id, recurring_id)
VALUES ('11111111-1111-1111-1111-111111111111', '70000000-0000-0000-0000-00000000001c',
        'a0000000-0000-0000-0000-000000000001', 'out', 150000, 'GHS',
        TIMESTAMPTZ '2027-01-01 00:00:00+00', 'Rent', 'pending', 'recurring', now(), now(),
        'aaaaaaaa-0000-0000-0000-000000000001', 'e1000000-0000-0000-0000-000000000001');

-- --------------------------------------------------------------------------- budgets ----

INSERT INTO budgets (user_id, id, category_id, period, cap_minor, currency, starts_on, ends_on,
                     created_at, updated_at, device_id)
VALUES ('11111111-1111-1111-1111-111111111111', 'd0000000-0000-0000-0000-000000000001',
        'c0000000-0000-0000-0000-000000000001', 'monthly', 50000, 'GHS', DATE '2026-01-01', DATE '2026-12-31',
        now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001');

SELECT must_fail($$
    INSERT INTO budgets (user_id, id, category_id, period, cap_minor, currency, starts_on,
                         created_at, updated_at, device_id)
    VALUES ('11111111-1111-1111-1111-111111111111', 'd0000000-0000-0000-0000-000000000002',
            'c0000000-0000-0000-0000-000000000001', 'monthly', 60000, 'GHS', DATE '2026-06-01',
            now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001');
$$, 'two live budgets cannot cover the same scope over overlapping dates');

-- The point of an exclusion constraint rather than a unique key: next year's cap can be set
-- today, without deleting the cap last quarter's verdict was measured against.
INSERT INTO budgets (user_id, id, category_id, period, cap_minor, currency, starts_on,
                     created_at, updated_at, device_id)
VALUES ('11111111-1111-1111-1111-111111111111', 'd0000000-0000-0000-0000-000000000003',
        'c0000000-0000-0000-0000-000000000001', 'monthly', 60000, 'GHS', DATE '2027-01-01',
        now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001');

-- A cedi cap and a dollar cap on the same category are two budgets, not a conflict.
INSERT INTO budgets (user_id, id, category_id, period, cap_minor, currency, starts_on,
                     created_at, updated_at, device_id)
VALUES ('11111111-1111-1111-1111-111111111111', 'd0000000-0000-0000-0000-000000000004',
        'c0000000-0000-0000-0000-000000000001', 'monthly', 10000, 'USD', DATE '2026-01-01',
        now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001');

-- ----------------------------------------------------------------------- attachments ----

INSERT INTO attachments (user_id, id, transaction_id, relative_path, media_type, byte_size, sha256,
                         created_at, updated_at, device_id)
VALUES ('11111111-1111-1111-1111-111111111111', 'a1000000-0000-0000-0000-000000000001',
        '70000000-0000-0000-0000-000000000001', 'receipts/2026/11/a.jpg', 'image/jpeg', 1024,
        '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
        now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001');

SELECT must_fail($$
    INSERT INTO attachments (user_id, id, transaction_id, relative_path, media_type, byte_size, sha256,
                             created_at, updated_at, device_id)
    VALUES ('11111111-1111-1111-1111-111111111111', 'a1000000-0000-0000-0000-000000000002',
            '70000000-0000-0000-0000-000000000001', '../../etc/passwd', 'image/jpeg', 1024,
            '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
            now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001');
$$, 'a path that climbs out of the attachment directory is refused');

SELECT must_fail($$
    INSERT INTO attachments (user_id, id, transaction_id, relative_path, media_type, byte_size, sha256,
                             created_at, updated_at, device_id)
    VALUES ('11111111-1111-1111-1111-111111111111', 'a1000000-0000-0000-0000-000000000003',
            '70000000-0000-0000-0000-000000000001', '/etc/passwd', 'image/jpeg', 1024,
            '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
            now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001');
$$, 'an absolute attachment path is refused');

-- ----------------------------------------------------------------------- other rules ----

SELECT must_fail($$
    INSERT INTO fx_rates (base, quote, rate_e8, as_of)
    VALUES ('USD', 'USD', 100000000, DATE '2026-11-10');
$$, 'an fx rate between a currency and itself is refused');

SELECT must_fail($$
    INSERT INTO category_rules (user_id, id, pattern, matched_field, category_id, origin, pack_id,
                                created_at, updated_at, device_id)
    VALUES ('11111111-1111-1111-1111-111111111111', 'e0000000-0000-0000-0000-000000000001',
            'TROTRO', 'label', 'c0000000-0000-0000-0000-000000000001', 'pack', NULL,
            now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001');
$$, 'a pack-origin rule must name its pack');

-- ----------------------------------------------------------------------- sync paging ----

-- An edit has to reach the other devices. A sequence default fires only on INSERT, so the
-- row would keep its old number and never be returned by `server_seq > watermark` again —
-- and the same goes for the soft delete that soft deletes exist to propagate.
DO $$
DECLARE before_seq BIGINT; after_edit BIGINT; after_delete BIGINT;
BEGIN
    SELECT server_seq INTO before_seq FROM transactions
        WHERE id = '70000000-0000-0000-0000-000000000001';
    UPDATE transactions SET label = 'Trotro to Madina', updated_at = now()
        WHERE id = '70000000-0000-0000-0000-000000000001';
    SELECT server_seq INTO after_edit FROM transactions
        WHERE id = '70000000-0000-0000-0000-000000000001';
    UPDATE transactions SET deleted_at = now(), updated_at = now()
        WHERE id = '70000000-0000-0000-0000-000000000001';
    SELECT server_seq INTO after_delete FROM transactions
        WHERE id = '70000000-0000-0000-0000-000000000001';

    IF after_edit <= before_seq THEN
        RAISE EXCEPTION 'an edit did not advance server_seq (% -> %)', before_seq, after_edit;
    END IF;
    IF after_delete <= after_edit THEN
        RAISE EXCEPTION 'a soft delete did not advance server_seq (% -> %)', after_edit, after_delete;
    END IF;
    RAISE NOTICE 'ok: an edit and a soft delete each advance server_seq (% -> % -> %)',
        before_seq, after_edit, after_delete;
END;
$$;

-- The counter is per user, so one person's activity does not inflate another's watermark.
SELECT must_equal(
    $$SELECT (SELECT sync_seq FROM users WHERE id = '22222222-2222-2222-2222-222222222222')::text$$,
    '1',
    'one user''s writes do not advance another user''s sync counter');

SELECT must_fail($$
    INSERT INTO accounts (user_id, id, name, kind, currency, created_at, updated_at, device_id)
    VALUES ('33333333-3333-3333-3333-333333333333', 'a0000000-0000-0000-0000-0000000000ff',
            'Orphan', 'cash', 'GHS', now(), now(), 'aaaaaaaa-0000-0000-0000-000000000001');
$$, 'a row for an unknown user cannot be stamped, so it cannot be written');

ROLLBACK;
