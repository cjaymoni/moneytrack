#!/usr/bin/env bash
# Proves that sync numbers are handed out in commit order, so a client's watermark can never
# skip a row.
#
# The bug this guards against: a sequence hands out numbers when a transaction *starts*, but
# rows become visible when it *commits*. A slow writer takes 21, a fast writer takes 22 and
# commits first, a client pulls and advances its watermark to 22, and row 21 is never
# returned by `server_seq > watermark` again. A silently missing transaction.
#
# next_sync_seq() holds the user's row locked until commit, so a second writer for the same
# user blocks until the first is visible. This script checks that it really does.
#
#   db/tests/postgres_sync_ordering.sh [container-name]   (default: mt-pg)
set -uo pipefail

container="${1:-mt-pg}"
psql() { docker exec -i "$container" psql -qtAX -v ON_ERROR_STOP=1 -U postgres -d postgres "$@"; }

U='99999999-9999-9999-9999-999999999999'
D='99999999-0000-0000-0000-000000000001'
A='99999999-0000-0000-0000-00000000000a'
SLOW='99999999-0000-0000-0000-0000000000s1'
FAST='99999999-0000-0000-0000-0000000000f1'
SLOW="${SLOW//s/1}"
FAST="${FAST//f/2}"

cleanup() { psql -c "DELETE FROM users WHERE id = '$U';" >/dev/null 2>&1; }
trap cleanup EXIT
cleanup

psql <<SQL >/dev/null
INSERT INTO users (id, email, password_hash) VALUES ('$U', 'ordering@example.test', 'placeholder');
INSERT INTO devices (user_id, id, name, platform) VALUES ('$U', '$D', 'Test device', 'android');
INSERT INTO accounts (user_id, id, name, kind, currency, created_at, updated_at, device_id)
VALUES ('$U', '$A', 'MoMo', 'momo', 'GHS', now(), now(), '$D');
SQL

row_sql() {
    echo "INSERT INTO transactions (user_id, id, account_id, direction, amount_minor, currency,
          booked_at, label, status, source, created_at, updated_at, device_id)
          VALUES ('$U', '$1', '$A', 'out', $2, 'GHS', now(), '$3', 'confirmed', 'manual',
                  now(), now(), '$D');"
}

# The slow writer takes its number first, then sits on it for three seconds.
( psql <<SQL >/dev/null 2>&1
BEGIN;
$(row_sql "$SLOW" 100 'slow writer')
SELECT pg_sleep(3);
COMMIT;
SQL
) &
slow_pid=$!

sleep 1

# The fast writer would, with a plain sequence, take a higher number and commit immediately.
started=$(date +%s)
psql <<SQL >/dev/null 2>&1
BEGIN;
$(row_sql "$FAST" 200 'fast writer')
COMMIT;
SQL
blocked_for=$(( $(date +%s) - started ))

wait "$slow_pid"

slow_seq=$(psql -c "SELECT server_seq FROM transactions WHERE id = '$SLOW';")
fast_seq=$(psql -c "SELECT server_seq FROM transactions WHERE id = '$FAST';")

failures=0

if [ -z "$slow_seq" ] || [ -z "$fast_seq" ]; then
    echo "FAIL: one of the two rows was not written (slow='$slow_seq' fast='$fast_seq')"
    exit 1
fi

if [ "$slow_seq" -lt "$fast_seq" ]; then
    echo "ok: the row that committed first has the lower sync number ($slow_seq < $fast_seq)"
else
    echo "FAIL: sync numbers are out of commit order (slow=$slow_seq fast=$fast_seq)"
    failures=$((failures + 1))
fi

if [ "$blocked_for" -ge 1 ]; then
    echo "ok: the second writer waited ${blocked_for}s for the first to commit, so no client can pull past it"
else
    echo "FAIL: the second writer did not wait; a client could pull $fast_seq and never see $slow_seq"
    failures=$((failures + 1))
fi

gap=$(psql -c "SELECT count(*) FROM generate_series(1, (SELECT sync_seq FROM users WHERE id = '$U')) s
               WHERE NOT EXISTS (SELECT 1 FROM transactions WHERE user_id = '$U' AND server_seq = s)
                 AND NOT EXISTS (SELECT 1 FROM accounts WHERE user_id = '$U' AND server_seq = s);")
if [ "$gap" = "0" ]; then
    echo "ok: the user's sync numbers are contiguous, with nothing stranded"
else
    echo "FAIL: $gap sync number(s) were handed out and never landed on a visible row"
    failures=$((failures + 1))
fi

if [ "$failures" -gt 0 ]; then
    echo "$failures check(s) failed"
    exit 1
fi
echo "all sync ordering checks passed"
