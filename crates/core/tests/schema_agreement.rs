//! Holds the SQL schemas and the Rust types together.
//!
//! The spellings in `db/postgres/0001_init.sql` and `db/sqlite/0001_init.sql` are the same
//! strings `mt-core` reads and writes, and the two files are meant to describe the same
//! tables. Nothing in the compiler connects any of that, so this test does: add a variant,
//! a currency or a column on one side only and it fails here rather than on someone's phone.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeSet;

use mt_core::enums::{
    AccountKind, BudgetPeriod, CategoryKind, Direction, InboxSource, InboxStatus, MatchedField,
    RecurrenceFreq, RuleOrigin, TxSource, TxStatus,
};
use mt_core::money::Currency;

const POSTGRES: &str = include_str!("../../../db/postgres/0001_init.sql");
const SQLITE: &str = include_str!("../../../db/sqlite/0001_init.sql");

/// Both schemas, for the vocabulary that appears in each.
const BOTH: &[(&str, &str)] = &[("postgres", POSTGRES), ("sqlite", SQLITE)];
/// The device schema alone. Inbox rows never leave the handset, so the inbox enums have no
/// Postgres column to constrain.
const DEVICE_ONLY: &[(&str, &str)] = &[("sqlite", SQLITE)];

/// The tables that exist in both schemas and sync between them.
const SYNCED_TABLES: &[&str] = &[
    "accounts",
    "categories",
    "recurring",
    "transactions",
    "category_rules",
    "budgets",
    "attachments",
];

/// Columns a synced table is expected to have on one side and not the other.
///
/// `user_id` is the server's tenant key; the device holds one person's data and has no use
/// for it. `dirty` is the device's "this row has local changes the server has not
/// acknowledged" flag; the server is the thing being acknowledged, so it has no use for it.
const POSTGRES_ONLY_COLUMNS: &[&str] = &["user_id"];
const DEVICE_ONLY_COLUMNS: &[&str] = &["dirty"];

/// The SQL with `--` comments removed, so prose about `DOUBLE PRECISION` is not mistaken
/// for a column declared as one.
fn without_comments(schema: &str) -> String {
    schema
        .lines()
        .map(|line| line.split("--").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The SQL as one line with runs of whitespace collapsed.
///
/// Lets the constraint assertions below survive a `CHECK` being wrapped across two lines,
/// which is formatting, not meaning.
fn flattened(schema: &str) -> String {
    without_comments(schema)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The `IN (...)` list a `CHECK` constraint would have to carry for this set of values.
fn in_list(values: &[&str]) -> String {
    let quoted: Vec<String> = values.iter().map(|v| format!("'{v}'")).collect();
    format!("IN ({})", quoted.join(", "))
}

/// The column names declared by one `CREATE TABLE`, in order.
///
/// Deliberately simple: a column line is indented exactly four spaces and starts with an
/// identifier that is not a table-constraint keyword. Both schema files are written that
/// way, and keeping the parser dumb keeps it from quietly matching the wrong thing.
fn columns_of(schema: &str, table: &str) -> Vec<String> {
    const CONSTRAINT_KEYWORDS: &[&str] = &[
        "CONSTRAINT",
        "PRIMARY",
        "FOREIGN",
        "UNIQUE",
        "CHECK",
        "EXCLUDE",
    ];

    let schema = without_comments(schema);
    let header = format!("CREATE TABLE {table} (");
    let start = schema
        .find(&header)
        .unwrap_or_else(|| panic!("no `{table}` table in this schema"))
        + header.len();
    let body = schema.get(start..).unwrap_or_default();
    let end = body
        .find("\n)")
        .unwrap_or_else(|| panic!("`{table}` has no closing paren on its own line"));
    let body = body.get(..end).unwrap_or_default();

    body.lines()
        .filter(|line| line.starts_with("    ") && !line.starts_with("     "))
        .filter_map(|line| line.split_whitespace().next())
        // Skips the tail of a multi-line table constraint, whose closing paren also sits at
        // four spaces.
        .filter(|token| {
            token.starts_with(|c: char| c.is_ascii_alphabetic())
                && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
        .filter(|token| !CONSTRAINT_KEYWORDS.contains(&token.to_uppercase().as_str()))
        .map(str::to_owned)
        .collect()
}

macro_rules! assert_check_constraints_cover {
    ($schemas:expr; $($ty:ty),* $(,)?) => {
        $(
            {
                let spellings: Vec<&str> = <$ty>::ALL.iter().map(|v| v.as_str()).collect();
                let expected = in_list(&spellings);
                for (name, schema) in $schemas {
                    assert!(
                        flattened(schema).contains(&expected),
                        "the {name} schema has no CHECK constraint matching {} — expected to \
                         find `{expected}`. Widen the constraint, or fix the variant order.",
                        stringify!($ty)
                    );
                }
            }
        )*
    };
}

#[test]
fn both_schemas_constrain_every_enum_column_to_the_variants_the_core_knows() {
    assert_check_constraints_cover!(
        BOTH;
        AccountKind,
        Direction,
        TxStatus,
        TxSource,
        CategoryKind,
        MatchedField,
        BudgetPeriod,
        RecurrenceFreq,
        RuleOrigin,
    );
}

#[test]
fn the_device_schema_constrains_the_inbox_enums() {
    assert_check_constraints_cover!(DEVICE_ONLY; InboxSource, InboxStatus);
}

#[test]
fn both_schemas_accept_exactly_the_currencies_the_registry_knows() {
    // A code either store accepts but `Currency::parse` does not produces a row mt-core
    // cannot deserialize — and since an unknown code is a hard error, one such row poisons
    // the whole pull or report batch it arrives in.
    let codes: Vec<&str> = Currency::ALL.iter().map(Currency::code).collect();
    let expected = in_list(&codes);
    for (name, schema) in BOTH {
        assert!(
            flattened(schema).contains(&expected),
            "the {name} schema does not constrain currency to the registry in money.rs — \
             expected to find `{expected}`"
        );
    }
}

#[test]
fn the_inbox_is_absent_from_the_server_schema() {
    // Only a confirmed transaction is allowed to leave the handset. An inbox table on the
    // server would be a place for unreviewed, parsed message content to accumulate.
    assert!(
        !POSTGRES.contains("CREATE TABLE inbox_items"),
        "inbox rows are device-only and must not exist on the server"
    );
}

#[test]
fn neither_schema_can_hold_a_floating_point_number() {
    // Money is integer minor units everywhere. A REAL column is how a rounding error gets
    // into a ledger, so the type must not appear at all — not in an amount, not in a rate.
    for (name, schema) in BOTH {
        let schema = without_comments(schema);
        for forbidden in [
            " REAL",
            " FLOAT",
            " DOUBLE PRECISION",
            " NUMERIC",
            " DECIMAL",
            " MONEY",
        ] {
            assert!(
                !schema.to_uppercase().contains(forbidden),
                "the {name} schema uses `{}`; money is integer minor units",
                forbidden.trim()
            );
        }
    }
}

#[test]
fn neither_schema_stores_a_balance() {
    // Balances are derived by the walk in mt-core. A stored balance is a second source of
    // truth that drifts, and then needs a repair job, which is the prior art's mistake.
    for (name, schema) in BOTH {
        let lowered = without_comments(schema).to_lowercase();
        for line in lowered.lines() {
            let line = line.trim();
            assert!(
                !line.contains("balance_minor")
                    || line.contains("opening_balance_minor")
                    || line.contains("reported_balance_minor"),
                "the {name} schema looks like it stores a running balance: {line}"
            );
        }
    }
}

#[test]
fn the_sqlite_schema_has_nowhere_to_put_a_raw_message() {
    // On-device parsing with no retention is a Play policy requirement for the SMS-based
    // money management exception, and the cheapest way to keep it true is to have no column
    // the text would fit in.
    let lowered = SQLITE.to_lowercase();
    for forbidden in ["raw_body", "message_body", "sms_body", "raw_message"] {
        assert!(
            !lowered.contains(forbidden),
            "the device schema has a `{forbidden}` column; raw SMS text is never stored"
        );
    }
}

#[test]
fn the_two_schemas_declare_the_same_columns_on_every_synced_table() {
    // The claim this file's name makes. Without it, the two files can drift apart column by
    // column — a bound on one side only, a field the other never gained — and the first
    // sign of it is a push rejected on a device that cannot say why.
    for table in SYNCED_TABLES {
        let server: BTreeSet<String> = columns_of(POSTGRES, table).into_iter().collect();
        let device: BTreeSet<String> = columns_of(SQLITE, table).into_iter().collect();

        let server_extra: Vec<&String> = server
            .difference(&device)
            .filter(|column| !POSTGRES_ONLY_COLUMNS.contains(&column.as_str()))
            .collect();
        let device_extra: Vec<&String> = device
            .difference(&server)
            .filter(|column| !DEVICE_ONLY_COLUMNS.contains(&column.as_str()))
            .collect();

        assert!(
            server_extra.is_empty(),
            "`{table}` has {server_extra:?} on the server and not on the device"
        );
        assert!(
            device_extra.is_empty(),
            "`{table}` has {device_extra:?} on the device and not on the server"
        );
    }
}

#[test]
fn every_synced_postgres_table_is_keyed_on_the_owner_and_stamped_for_sync() {
    // Tenant isolation is structural: the primary key is (user_id, id) and foreign keys
    // carry user_id through, so a row physically cannot reference another user's row.
    for table in SYNCED_TABLES {
        let start = POSTGRES
            .find(&format!("CREATE TABLE {table} ("))
            .unwrap_or_else(|| panic!("no {table} table in the postgres schema"));
        let body = POSTGRES.get(start..).unwrap_or_default();
        let end = body.find("\n);").unwrap_or(body.len());
        let body = body.get(..end).unwrap_or_default();

        assert!(
            body.contains("PRIMARY KEY (user_id, id)"),
            "{table} is not keyed on (user_id, id)"
        );
        assert!(
            body.contains("server_seq"),
            "{table} has no server_seq, so sync cannot page through it"
        );
        // A default would fire on INSERT only, leaving edits and soft deletes invisible to
        // every other device.
        assert!(
            !body.contains("server_seq          BIGINT NOT NULL DEFAULT"),
            "{table}.server_seq has a default; it must be stamped by the trigger on every \
             INSERT *and* UPDATE"
        );
        assert!(
            POSTGRES.contains(&format!(
                "CREATE TRIGGER {table}_stamp BEFORE INSERT OR UPDATE ON {table}"
            )),
            "{table} has no BEFORE INSERT OR UPDATE trigger to stamp server_seq"
        );
    }
}

#[test]
fn every_synced_device_table_can_say_what_still_needs_pushing() {
    // Push reads `WHERE dirty = 1`. It cannot read `updated_at > last_push_at`, because
    // updated_at on a pulled row is another device's clock.
    for table in SYNCED_TABLES {
        let columns = columns_of(SQLITE, table);
        for required in ["dirty", "server_seq"] {
            assert!(
                columns.iter().any(|column| column == required),
                "the device's `{table}` has no `{required}` column, so push cannot find \
                 local changes without trusting another device's clock"
            );
        }
    }
}
