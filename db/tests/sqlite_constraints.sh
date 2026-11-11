#!/usr/bin/env bash
# Proves the device schema's constraints bite. Builds a throwaway database from
# db/sqlite/0001_init.sql, seeds it, and checks that each statement below is refused.
#
#   db/tests/sqlite_constraints.sh
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
db="$(mktemp -t moneytrack-schema-XXXXXX).db"
trap 'rm -f "$db"' EXIT

run() { sqlite3 "$db" "PRAGMA foreign_keys = ON; $1"; }

failures=0

must_fail() {
    local what="$1" stmt="$2" output
    if output="$(run "$stmt" 2>&1)"; then
        echo "FAIL: expected a refusal — $what"
        failures=$((failures + 1))
    else
        echo "ok: $what"
    fi
}

must_pass() {
    local what="$1" stmt="$2" output
    if output="$(run "$stmt" 2>&1)"; then
        echo "ok: $what"
    else
        echo "FAIL: $what — $output"
        failures=$((failures + 1))
    fi
}

must_equal() {
    local what="$1" query="$2" expected="$3" actual
    actual="$(run "$query" 2>&1)"
    if [ "$actual" = "$expected" ]; then
        echo "ok: $what"
    else
        echo "FAIL: $what — expected '$expected', got '$actual'"
        failures=$((failures + 1))
    fi
}

sqlite3 "$db" < "$here/db/sqlite/0001_init.sql" || { echo "FAIL: schema did not apply"; exit 1; }

DEV="'0d000000-0000-7000-8000-000000000001'"
MOMO="'0a000000-0000-7000-8000-000000000001'"
BANK="'0a000000-0000-7000-8000-000000000002'"
USDACC="'0a000000-0000-7000-8000-000000000003'"
CAT="'0c000000-0000-7000-8000-000000000001'"
TRANSFER="'0f000000-0000-7000-8000-000000000001'"
tx() { echo "'07000000-0000-7000-8000-0000000000$1'"; }
NOW=1762000000000

run "INSERT INTO accounts (id, name, kind, currency, created_at, updated_at, device_id)
     VALUES ($MOMO, 'MoMo', 'momo', 'GHS', 0, 0, $DEV),
            ($BANK, 'GCB', 'bank', 'GHS', 0, 0, $DEV),
            ($USDACC, 'Dollar account', 'bank', 'USD', 0, 0, $DEV);
     INSERT INTO categories (id, name, kind, created_at, updated_at, device_id)
     VALUES ($CAT, 'Transport', 'expense', 0, 0, $DEV);"

# ---------------------------------------------------------------- ledger invariants ----

must_pass "a plain expense row inserts" \
    "INSERT INTO transactions (id, account_id, direction, amount_minor, currency, booked_at, label, status, source, created_at, updated_at, device_id, external_id)
     VALUES ($(tx 01), $MOMO, 'out', 2000, 'GHS', $NOW, 'Trotro', 'confirmed', 'sms', 0, 0, $DEV, 'MTN-REF-1');"

must_equal "a new row starts dirty, with no server_seq" \
    "SELECT dirty, ifnull(server_seq, 'null') FROM transactions WHERE id = $(tx 01);" "1|null"

must_fail "an unknown account is refused" \
    "INSERT INTO transactions (id, account_id, direction, amount_minor, currency, booked_at, label, status, source, created_at, updated_at, device_id)
     VALUES ($(tx 02), '0a000000-0000-7000-8000-00000000ffff', 'out', 100, 'GHS', 0, 'Nowhere', 'confirmed', 'manual', 0, 0, $DEV);"

must_fail "a row cannot be in a different currency from its account" \
    "INSERT INTO transactions (id, account_id, direction, amount_minor, currency, booked_at, label, status, source, created_at, updated_at, device_id)
     VALUES ($(tx 03), $MOMO, 'out', 100, 'USD', 0, 'Dollars in a cedi account', 'confirmed', 'manual', 0, 0, $DEV);"

must_pass "the same amount in its own account's currency is fine" \
    "INSERT INTO transactions (id, account_id, direction, amount_minor, currency, booked_at, label, status, source, created_at, updated_at, device_id)
     VALUES ($(tx 04), $USDACC, 'out', 100, 'USD', 0, 'Dollars in a dollar account', 'confirmed', 'manual', 0, 0, $DEV);"

must_fail "a currency outside the registry is refused" \
    "INSERT INTO accounts (id, name, kind, currency, created_at, updated_at, device_id)
     VALUES ('0a000000-0000-7000-8000-00000000000a', 'Zed', 'cash', 'ZZZ', 0, 0, $DEV);"

must_fail "a lowercase currency code is refused" \
    "INSERT INTO accounts (id, name, kind, currency, created_at, updated_at, device_id)
     VALUES ('0a000000-0000-7000-8000-00000000000b', 'Lower', 'cash', 'ghs', 0, 0, $DEV);"

must_fail "a negative amount is refused; the sign lives in direction" \
    "INSERT INTO transactions (id, account_id, direction, amount_minor, currency, booked_at, label, status, source, created_at, updated_at, device_id)
     VALUES ($(tx 05), $MOMO, 'out', -100, 'GHS', 0, 'Negative', 'confirmed', 'manual', 0, 0, $DEV);"

must_fail "an amount large enough to overflow a balance walk is refused" \
    "INSERT INTO transactions (id, account_id, direction, amount_minor, currency, booked_at, label, status, source, created_at, updated_at, device_id)
     VALUES ($(tx 06), $MOMO, 'out', 9223372036854775807, 'GHS', 0, 'Absurd', 'confirmed', 'manual', 0, 0, $DEV);"

must_fail "a date in the twenty-third century is refused" \
    "INSERT INTO transactions (id, account_id, direction, amount_minor, currency, booked_at, label, status, source, created_at, updated_at, device_id)
     VALUES ($(tx 07), $MOMO, 'out', 100, 'GHS', 99999999999999, 'Far future', 'confirmed', 'manual', 0, 0, $DEV);"

must_fail "there is no transfer direction" \
    "INSERT INTO transactions (id, account_id, direction, amount_minor, currency, booked_at, label, status, source, created_at, updated_at, device_id)
     VALUES ($(tx 08), $MOMO, 'transfer', 100, 'GHS', 0, 'Bad', 'confirmed', 'manual', 0, 0, $DEV);"

must_fail "a transfer leg cannot carry a category" \
    "INSERT INTO transactions (id, account_id, direction, amount_minor, currency, booked_at, label, status, source, created_at, updated_at, device_id, transfer_id, category_id)
     VALUES ($(tx 09), $MOMO, 'out', 100, 'GHS', 0, 'To bank', 'confirmed', 'manual', 0, 0, $DEV, $TRANSFER, $CAT);"

must_pass "the two legs of a transfer insert as a pair" \
    "INSERT INTO transactions (id, account_id, direction, amount_minor, currency, booked_at, label, status, source, created_at, updated_at, device_id, transfer_id)
     VALUES ($(tx 10), $MOMO, 'out', 50000, 'GHS', $NOW, 'To GCB', 'confirmed', 'manual', 0, 0, $DEV, $TRANSFER),
            ($(tx 11), $BANK, 'in', 50000, 'GHS', $NOW, 'From MoMo', 'confirmed', 'manual', 0, 0, $DEV, $TRANSFER);"

must_fail "the same institution reference cannot be imported into an account twice" \
    "INSERT INTO transactions (id, account_id, direction, amount_minor, currency, booked_at, label, status, source, created_at, updated_at, device_id, external_id)
     VALUES ($(tx 12), $MOMO, 'out', 2000, 'GHS', 0, 'Trotro again', 'confirmed', 'csv', 0, 0, $DEV, 'MTN-REF-1');"

must_pass "the same reference in a different account is a different movement" \
    "INSERT INTO transactions (id, account_id, direction, amount_minor, currency, booked_at, label, status, source, created_at, updated_at, device_id, external_id)
     VALUES ($(tx 13), $BANK, 'out', 2000, 'GHS', 0, 'Coincidence', 'confirmed', 'csv', 0, 0, $DEV, 'MTN-REF-1');"

must_fail "a fractional amount cannot be stored at all (STRICT)" \
    "INSERT INTO transactions (id, account_id, direction, amount_minor, currency, booked_at, label, status, source, created_at, updated_at, device_id)
     VALUES ($(tx 14), $MOMO, 'out', 20.50, 'GHS', 0, 'Float money', 'confirmed', 'manual', 0, 0, $DEV);"

must_fail "a decimal string is not a valid amount either" \
    "INSERT INTO transactions (id, account_id, direction, amount_minor, currency, booked_at, label, status, source, created_at, updated_at, device_id)
     VALUES ($(tx 15), $MOMO, 'out', '20.50', 'GHS', 0, 'Float money', 'confirmed', 'manual', 0, 0, $DEV);"

must_fail "updated_at before created_at is refused" \
    "INSERT INTO transactions (id, account_id, direction, amount_minor, currency, booked_at, label, status, source, created_at, updated_at, device_id)
     VALUES ($(tx 16), $MOMO, 'out', 100, 'GHS', 0, 'Backwards', 'confirmed', 'manual', 1000, 999, $DEV);"

must_fail "an id that is not a UUID is refused" \
    "INSERT INTO accounts (id, name, kind, currency, created_at, updated_at, device_id)
     VALUES ('acc-momo', 'Shorthand', 'cash', 'GHS', 0, 0, $DEV);"

must_fail "a blank account name is refused" \
    "INSERT INTO accounts (id, name, kind, currency, created_at, updated_at, device_id)
     VALUES ('0a000000-0000-7000-8000-00000000000c', '   ', 'cash', 'GHS', 0, 0, $DEV);"

must_fail "an unknown account kind is refused" \
    "INSERT INTO accounts (id, name, kind, currency, created_at, updated_at, device_id)
     VALUES ('0a000000-0000-7000-8000-00000000000d', 'Wallet', 'crypto', 'GHS', 0, 0, $DEV);"

# ------------------------------------------------------------------------ recurrence ----

must_pass "a monthly template with a day of the month inserts" \
    "INSERT INTO recurring (id, name, account_id, direction, amount_minor, currency, label, freq, interval, by_month_day, starts_on, created_at, updated_at, device_id)
     VALUES ('0e000000-0000-7000-8000-000000000001', 'Rent', $MOMO, 'out', 150000, 'GHS', 'Rent', 'monthly', 1, 1, '2026-12-01', 0, 0, $DEV);"

must_fail "a weekly template with a day of the month is refused" \
    "INSERT INTO recurring (id, name, account_id, direction, amount_minor, currency, label, freq, interval, by_month_day, starts_on, created_at, updated_at, device_id)
     VALUES ('0e000000-0000-7000-8000-000000000002', 'Odd', $MOMO, 'out', 1000, 'GHS', 'Odd', 'weekly', 1, 15, '2026-12-01', 0, 0, $DEV);"

must_fail "a monthly template with no day at all is refused" \
    "INSERT INTO recurring (id, name, account_id, direction, amount_minor, currency, label, freq, interval, starts_on, created_at, updated_at, device_id)
     VALUES ('0e000000-0000-7000-8000-000000000003', 'Vague', $MOMO, 'out', 1000, 'GHS', 'Vague', 'monthly', 1, '2026-12-01', 0, 0, $DEV);"

RENT="'0e000000-0000-7000-8000-000000000001'"

must_pass "a template generates its first instance" \
    "INSERT INTO transactions (id, account_id, direction, amount_minor, currency, booked_at, label, status, source, created_at, updated_at, device_id, recurring_id)
     VALUES ($(tx 20), $MOMO, 'out', 150000, 'GHS', 1796083200, 'Rent', 'pending', 'recurring', 0, 0, $DEV, $RENT);"

# Generation is idempotent through the template's generated_through watermark, but only if
# the rows and the new watermark are written in one transaction. This is the backstop for
# when they are not, which would otherwise show the user next month's rent twice over.
must_fail "a template cannot generate the same instant twice" \
    "INSERT INTO transactions (id, account_id, direction, amount_minor, currency, booked_at, label, status, source, created_at, updated_at, device_id, recurring_id)
     VALUES ($(tx 21), $MOMO, 'out', 150000, 'GHS', 1796083200, 'Rent', 'pending', 'recurring', 0, 0, $DEV, $RENT);"

must_pass "the next month's instance is a different instant and inserts" \
    "INSERT INTO transactions (id, account_id, direction, amount_minor, currency, booked_at, label, status, source, created_at, updated_at, device_id, recurring_id)
     VALUES ($(tx 22), $MOMO, 'out', 150000, 'GHS', 1798761600, 'Rent', 'pending', 'recurring', 0, 0, $DEV, $RENT);"

must_pass "a row typed by hand is not an instance and is not constrained" \
    "INSERT INTO transactions (id, account_id, direction, amount_minor, currency, booked_at, label, status, source, created_at, updated_at, device_id)
     VALUES ($(tx 23), $MOMO, 'out', 150000, 'GHS', 1796083200, 'Rent, paid in cash', 'confirmed', 'manual', 0, 0, $DEV);"

must_fail "a date that is not a date is refused" \
    "INSERT INTO recurring (id, name, account_id, direction, amount_minor, currency, label, freq, interval, starts_on, created_at, updated_at, device_id)
     VALUES ('0e000000-0000-7000-8000-000000000004', 'Junk', $MOMO, 'out', 1000, 'GHS', 'Junk', 'daily', 1, 'abcd-ef-gh', 0, 0, $DEV);"

must_fail "the 31st of February is refused" \
    "INSERT INTO recurring (id, name, account_id, direction, amount_minor, currency, label, freq, interval, starts_on, created_at, updated_at, device_id)
     VALUES ('0e000000-0000-7000-8000-000000000005', 'Impossible', $MOMO, 'out', 1000, 'GHS', 'Impossible', 'daily', 1, '2026-02-31', 0, 0, $DEV);"

# --------------------------------------------------------------------------- budgets ----

must_pass "a live budget inserts" \
    "INSERT INTO budgets (id, category_id, period, cap_minor, currency, starts_on, created_at, updated_at, device_id)
     VALUES ('0b000000-0000-7000-8000-000000000001', $CAT, 'monthly', 50000, 'GHS', '2026-11-01', 0, 0, $DEV);"

must_fail "the same cap for the same scope and start is a duplicate" \
    "INSERT INTO budgets (id, category_id, period, cap_minor, currency, starts_on, created_at, updated_at, device_id)
     VALUES ('0b000000-0000-7000-8000-000000000002', $CAT, 'monthly', 60000, 'GHS', '2026-11-01', 0, 0, $DEV);"

must_pass "next year's cap can be set while this year's is still live" \
    "INSERT INTO budgets (id, category_id, period, cap_minor, currency, starts_on, created_at, updated_at, device_id)
     VALUES ('0b000000-0000-7000-8000-000000000003', $CAT, 'monthly', 60000, 'GHS', '2027-01-01', 0, 0, $DEV);"

must_pass "a cap in another currency is a separate budget, not a clash" \
    "INSERT INTO budgets (id, category_id, period, cap_minor, currency, starts_on, created_at, updated_at, device_id)
     VALUES ('0b000000-0000-7000-8000-000000000004', $CAT, 'monthly', 10000, 'USD', '2026-11-01', 0, 0, $DEV);"

must_fail "a budget cap of zero is refused" \
    "INSERT INTO budgets (id, period, cap_minor, currency, starts_on, created_at, updated_at, device_id)
     VALUES ('0b000000-0000-7000-8000-000000000005', 'monthly', 0, 'GHS', '2026-11-01', 0, 0, $DEV);"

must_fail "a warning threshold above 100% is refused" \
    "INSERT INTO budgets (id, period, cap_minor, currency, starts_on, warn_at_bps, created_at, updated_at, device_id)
     VALUES ('0b000000-0000-7000-8000-000000000006', 'monthly', 1000, 'GHS', '2026-11-01', 10001, 0, 0, $DEV);"

# ----------------------------------------------------------------------- attachments ----

must_pass "a receipt with a relative path and a hex digest inserts" \
    "INSERT INTO attachments (id, transaction_id, relative_path, media_type, byte_size, sha256, created_at, updated_at, device_id)
     VALUES ('0a100000-0000-7000-8000-000000000001', $(tx 01), 'receipts/2026/11/a.jpg', 'image/jpeg', 1024,
             '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef', 0, 0, $DEV);"

must_fail "a path that climbs out of the attachment directory is refused" \
    "INSERT INTO attachments (id, transaction_id, relative_path, media_type, byte_size, sha256, created_at, updated_at, device_id)
     VALUES ('0a100000-0000-7000-8000-000000000002', $(tx 01), '../../etc/passwd', 'image/jpeg', 1024,
             '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef', 0, 0, $DEV);"

must_fail "an absolute path is refused" \
    "INSERT INTO attachments (id, transaction_id, relative_path, media_type, byte_size, sha256, created_at, updated_at, device_id)
     VALUES ('0a100000-0000-7000-8000-000000000003', $(tx 01), '/etc/passwd', 'image/jpeg', 1024,
             '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef', 0, 0, $DEV);"

must_fail "a digest that is not lowercase hex is refused" \
    "INSERT INTO attachments (id, transaction_id, relative_path, media_type, byte_size, sha256, created_at, updated_at, device_id)
     VALUES ('0a100000-0000-7000-8000-000000000004', $(tx 01), 'receipts/b.jpg', 'image/jpeg', 1024,
             'ZZ23456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef', 0, 0, $DEV);"

# ----------------------------------------------------------------------------- inbox ----

must_pass "a pending inbox item inserts" \
    "INSERT INTO inbox_items (id, source, received_at, fingerprint, direction, amount_minor, currency, booked_at, label, created_at, updated_at, device_id)
     VALUES ('0e100000-0000-7000-8000-000000000001', 'sms', 0, 'fp-1', 'out', 2000, 'GHS', 0, 'Payment to SHOPRITE', 0, 0, $DEV);"

must_fail "the same source message cannot land in the inbox twice" \
    "INSERT INTO inbox_items (id, source, received_at, fingerprint, direction, amount_minor, currency, booked_at, label, created_at, updated_at, device_id)
     VALUES ('0e100000-0000-7000-8000-000000000002', 'sms', 0, 'fp-1', 'out', 2000, 'GHS', 0, 'Payment to SHOPRITE', 0, 0, $DEV);"

must_fail "an accepted inbox item must name the row it became" \
    "UPDATE inbox_items SET status = 'accepted' WHERE id = '0e100000-0000-7000-8000-000000000001';"

must_pass "accepting an inbox item links it to its transaction" \
    "UPDATE inbox_items SET status = 'accepted', transaction_id = $(tx 01) WHERE id = '0e100000-0000-7000-8000-000000000001';"

must_pass "an unparsed message can be tallied before any pack is installed" \
    "INSERT INTO unparsed_counts (sender, on_date, tally) VALUES ('MTN', '2026-11-10', 1);"

must_equal "that tally records pack_version 0 rather than throwing" \
    "SELECT pack_version FROM unparsed_counts WHERE sender = 'MTN';" "0"

# ------------------------------------------------------------------------- reference ----

must_fail "an fx rate between a currency and itself is refused" \
    "INSERT INTO fx_rates (base, quote, rate_e8, as_of) VALUES ('USD', 'USD', 100000000, '2026-11-10');"

must_fail "a non-positive fx rate is refused" \
    "INSERT INTO fx_rates (base, quote, rate_e8, as_of) VALUES ('USD', 'GHS', 0, '2026-11-10');"

must_fail "sync_state holds exactly one row" \
    "INSERT INTO sync_state (id, device_id) VALUES (2, $DEV);"

# Every synced table must be able to answer "what still needs pushing?" without consulting
# another device's clock.
for table in accounts categories recurring transactions category_rules budgets attachments; do
    if ! sqlite3 "$db" "SELECT dirty, server_seq FROM $table LIMIT 0;" >/dev/null 2>&1; then
        echo "FAIL: $table has no dirty/server_seq columns, so push cannot find local changes"
        failures=$((failures + 1))
    fi
done
echo "ok: every synced table carries dirty and server_seq"

# There is nowhere in this schema to put a raw message body, and that is the point.
if sqlite3 "$db" "SELECT name FROM pragma_table_info('inbox_items');" | grep -qiE '^(body|raw_text|message|sms_body)$'; then
    echo "FAIL: the inbox has a column that could hold raw message text"
    failures=$((failures + 1))
else
    echo "ok: the inbox has nowhere to store a raw message body"
fi

if [ "$failures" -gt 0 ]; then
    echo "$failures check(s) failed"
    exit 1
fi
echo "all device schema checks passed"
