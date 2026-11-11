# Architecture

Companion to [PLAN.md](PLAN.md), which covers *what* is being built and in what order. This
document covers *how* it is put together and why. Written 2026-11-10. Nothing is built yet, so
treat every signature below as a sketch that first contact with a compiler will change.

`moneytrack` is a working name.

---

## 1. The decision

**A Rust core and server, a Flutter UI.**

Everything that decides what a number is — money arithmetic, the balance walk, the
categorisation ladder, recurrence expansion, duplicate scoring, budget evaluation, SMS parsing
— is one Rust crate. It is compiled into the mobile and desktop apps through
`flutter_rust_bridge`, into the web client as WASM, and linked directly into the Axum server.
Everything a person touches is Flutter.

### Why

The product syncs, which means the client and the server both compute balances, budget state
and whether two rows are the same transaction. Two implementations of those rules will
eventually disagree, and a client and server disagreeing about someone's balance is the bug
class that destroys trust in a finance app and consumes its support budget. One compiled
implementation cannot drift.

Secondarily, the hard logic here is pure: bytes in, values out, no I/O. That is the shape where
the bridge stays narrow and where Rust's type system actually earns its keep — `Direction`,
`TxStatus` and `RecurrenceScope` are exactly the enums you want matched exhaustively when the
subject is other people's money.

### Alternatives rejected

**Flutter alone, core in Dart.** Simplest build, fewest moving parts, and the right answer for
a personal app. Rejected because the server then needs a second implementation of every rule,
in a second language, kept in agreement by discipline.

**Rust alone, UI in Tauri 2 mobile or Dioxus.** Attractive on paper and closest to the existing
`credvault-rs` muscle. Rejected because there is almost no body of published consumer finance
apps on that path, and the project's hardest single feature — an Android broadcast receiver
that survives app kill and feeds a parser — sits exactly where that stack is least travelled.
Time would go into the framework rather than the product.

**React Native.** Third on the merits for a three-target product, and the local toolchain would
need upgrading before the first line of code. See PLAN.md.

### Cost accepted

A cross-compilation matrix — Android (NDK), iOS, macOS, Windows, Linux and WASM — maintained
in CI for the life of the project. And web is the soft spot: `flutter_rust_bridge` has
supported WASM since its 2.0 line and tests web in CI, but it is not obviously battle-tested
for production, so the web target gets built in phase 0 rather than discovered in phase 5.

**Containment.** If WASM disappoints, the web client calls the server for anything the core
computes. That costs nothing, because the server is already running the same Rust. The risk is
absorbed by the architecture rather than by optimism.

---

## 2. Shape

```
                 ┌──────────────────────────────────────────┐
                 │               Flutter UI                 │
                 │  screens · drift/SQLite · sync client    │
                 │  platform plugins (SMS, camera, files)   │
                 └───────────────┬──────────────────────────┘
                                 │  flutter_rust_bridge
                                 │  plain structs, value in / value out
                 ┌───────────────▼──────────────────────────┐
                 │            mt-core  (Rust)               │
                 │  money · ledger · rules · recurrence     │
                 │  dedup · budget · parse                  │
                 │          no I/O, no clock, no DB         │
                 └───────────────┬──────────────────────────┘
                                 │  same crate, linked
                 ┌───────────────▼──────────────────────────┐
                 │        mt-server (Rust / Axum)           │
                 │  auth · sync · rule packs · Postgres     │
                 └──────────────────────────────────────────┘
```

The web client is the same Flutter tree with `mt-core` compiled to WASM, or falling back to
server-side computation.

### Repository layout

```
moneytrack/
├── docs/                  PLAN.md, ARCHITECTURE.md, decisions/
├── crates/
│   ├── core/              mt-core — pure, no I/O. The only place rules live.
│   ├── server/            mt-server — Axum, Postgres, auth, sync, rule packs.
│   └── bridge/            frb glue: the published API surface, nothing else.
├── app/                   Flutter: UI, drift schema, sync client, plugins.
│   └── android/.../sms/   Kotlin broadcast receiver + plugin channel.
├── packs/                 versioned rule packs (patterns, categories) as data.
├── site/                  static marketing site. Not Flutter.
└── .github/workflows/     build matrix, rule-pack release, store pipelines.
```

---

## 3. The bridge boundary

The single rule: **values cross, handles do not.** The core never holds a database connection,
a file handle, a socket or a clock. Time is passed in. Rows are passed in. Results come back as
owned structs. This keeps `flutter_rust_bridge` in the regime where it is pleasant, and it is
also what makes the core testable without a device.

What stays in Dart, deliberately:

- **Storage.** drift/SQLite. Moving persistence into Rust would widen the bridge enormously and
  give up drift's reactive queries, which are what make the inbox and the running balance
  update live as rows change.
- **Sync transport.** HTTP, retries, auth tokens, background scheduling.
- **Platform plugins.** The Kotlin SMS receiver, camera, file picking.

What crosses, as built:

```rust
// crates/bridge/src/api.rs — the entire public surface, kept small on purpose.

pub fn walk_balance(account: AccountDto, txs: Vec<TransactionDto>, basis: BalanceBasis)
    -> Result<WalkDto, BridgeError>;
pub fn account_balances(..) -> Result<Vec<AccountBalanceDto>, BridgeError>;
pub fn currency_totals(..) -> Result<Vec<MoneyDto>, BridgeError>;
pub fn period_flows(..) -> Result<FlowsDto, BridgeError>;
pub fn evaluate_budgets(.., on: String, basis: BalanceBasis, zone: ZoneDto)
    -> Result<Vec<BudgetStatusDto>, BridgeError>;
pub fn expand_recurrence(rule: RecurringRuleDto, through: String, now: String, zone: ZoneDto)
    -> Result<GenerationDto, BridgeError>;
pub fn categorise_candidate(..) -> Result<CategorisationDto, BridgeError>;
pub fn instances_in_scope(rule: RecurringRuleDto, scope: RecurrenceScope, ..)
    -> Result<Vec<String>, BridgeError>;      // this | future | past | all, asked not guessed
pub fn score_duplicates(..) -> Result<Vec<DuplicateReportDto>, BridgeError>;
pub fn compile_rules(..) -> Result<(), BridgeError>;      // refuse a bad pattern at save time
pub fn validate_transfer(..) -> Result<(), BridgeError>;  // refuse legs that disagree
pub fn parse_amount(input: String, currency: String) -> Result<MoneyDto, BridgeError>;
```

Four differences from the sketch this section used to carry, each one the design meeting
reality. `parse_message` is absent because the parsers are phase 2 and there is nothing
behind it yet. Every function returns a `Result`: the input is a row read out of SQLite by a
client that may be a release ahead, so the boundary validates and names the field rather than
computing a plausible wrong number. Ids, instants and dates cross as text in the spellings
the sync wire already commits to, and money crosses as minor units plus a code — never as a
decimal string, never as a double. And the zone crosses as a table of offsets the platform
supplies, because the core refuses to carry tzdata and the handset already has it — with the
window that table was built for, so a question reaching past it is refused rather than
answered from the base offset and quietly put in the wrong month.

Note what is absent: no `save`, no `fetch`, no `sync`. Those are Dart's. The clock is Dart's
too — `now` is an argument — with one exception, `expand_recurrence`, which mints a UUIDv7
per row because a row needs an id before it can be written.

### What codegen actually produced

`flutter_rust_bridge` 2.13 is wired (`app/flutter_rust_bridge.yaml`, `rust_input: crate::api`),
so the contract above is no longer a claim. Every function crosses as a plain Dart future
taking named arguments, every DTO as a plain Dart class, and `BridgeError` as a sealed union
Dart can switch on exhaustively. **No opaque pointers**: not one handle crosses, which is the
property the whole boundary was shaped around. The Rust doc comments come through onto the
Dart side, so the rule about what `raw_text` may never do is readable from Flutter.

One cost, paid knowingly. The generated glue is FFI and uses `unsafe`, and `forbid` cannot
be switched off by an inner `allow` — so a crate holding that glue cannot forbid unsafe at
its root. The workspace therefore denies rather than forbids, and `forbid` is applied one
level in, where it still bites: fourteen of the workspace's fifteen crate roots forbid it
outright, as do `mt_bridge`'s six hand-written modules. The exception is
`crates/bridge/src/lib.rs` itself, which stays on `deny` so the generated module it holds
can carry the single `allow`. The policy is written out in full in the workspace
`Cargo.toml`, beside the lint it governs.

---

## 4. The core crate

```
crates/core/src/
├── money.rs        Money(i64 minor units, Currency). No floats, ever.
├── model.rs        Tx, Account, Category, Budget, RecurringRule, Candidate.
├── ledger.rs       the balance walk — the heart of the product.
├── rules.rs        the categorisation ladder.
├── recurrence.rs   cadence expansion + RecurrenceScope edit semantics.
├── dedup.rs        duplicate scoring across sources.
├── budget.rs       cap evaluation, thresholds.
└── parse/          sms.rs, csv.rs, ofx.rs — one trait, many handlers.
```

### Money

```rust
pub struct Money { minor: i64, currency: Currency }
```

Integer minor units with the currency attached, always. There is no `f64` anywhere in this
crate and the lint config forbids it. Addition of two different currencies does not compile
into a silent result — it returns `Result<Money, CurrencyMismatch>`. A transaction keeps the
currency it happened in forever; conversion is a presentation concern using the rate for that
transaction's date, so historical reports do not move when the cedi does.

### The balance walk

The central idea from PLAN.md: a transaction's `status` is `Pending` or `Confirmed`, and
future-dated pending rows *are* the forecast. The walk does not distinguish past from future.

```rust
pub fn walk_balance(opening: Money, mut txs: Vec<Tx>, until: DateTime<Utc>) -> Vec<BalancePoint>
```

Sort by `booked_at`, accumulate, emit a point per day. One function serves the history screen,
the outlook screen and the "when do I go negative" warning. There is no projection engine and
no second code path, which is the whole reason the pending model was adopted.

Transfers are a pair of rows linked by `transfer_id`; the walk nets them so moving money from
bank to MoMo is neither income nor spending. Getting this wrong makes every report wrong, so it
is covered by tests before anything else in `ledger.rs`.

### The categorisation ladder

Borrowed from Expensave (ideas only — it is GPL-3.0, see PLAN.md). First hit wins:

1. a category named explicitly by the source file
2. a `category_rules` pattern match, by priority
3. **the user's own most recent transaction with the same label or counterparty**
4. Uncategorised

Step 3 is the one that makes it feel clever, and it is a lookup, not machine learning.
Corrections made in the inbox write back as new rules, so the system learns by being used.

Patterns are data, not code. They live in rule packs (§7) and carry `matched_field` and
`priority`. A malformed pattern fails loudly at save time — never compiled with errors
suppressed, which is the trap in the prior art.

### Parsing

```rust
pub trait SourceHandler {
    fn supports(&self, input: &RawInput) -> bool;
    fn parse(&self, input: &RawInput) -> Result<Vec<Candidate>, ParseError>;
}
```

A resolver walks registered handlers until one claims the input. Adding a Ghanaian bank is a
new handler or, better, a new pattern in a rule pack. Handlers never produce a `Tx` directly —
they produce a `Candidate`, which lands in the inbox for confirmation. Nothing parsed is ever
committed to the ledger unreviewed; SMS formats change without notice and a silent mis-parse
corrupts the numbers quietly.

---

## 5. Data flow

### SMS capture (Android only)

```
BroadcastReceiver (Kotlin)
  └─ sender allow-list check            ← before the body is read into memory
     └─ MethodChannel → Dart
        └─ mt-core::parse_message       ← on-device, always
           └─ mt-core::categorise
              └─ inbox_items (SQLite)   ← local
                 └─ user confirms
                    └─ transactions     ← only this ever syncs
```

The allow-list check happening *first* is a policy requirement, not an optimisation — the Play
exception for "SMS-based money management" forbids a budgeting app from exfiltrating
non-financial or personal SMS history. Raw message text never leaves the device, never reaches
the server, and is not retained after parsing. An unmatched message is counted, not stored, and
can only be shared by an explicit user action that sends a redacted sample (§7).

### Manual entry

The target is under five seconds: amount, category, done, with account and date defaulted.
Writes straight to `transactions` as `Confirmed`, bypassing the inbox. This path must work
perfectly on iOS, where SMS does not exist and never will.

### Import

File → resolver → handler → `Vec<Candidate>` → `score_duplicates` against existing rows →
suspected pairs surfaced for merge → inbox. Matching is amount + account + a date window of a
day or two + a counterparty fingerprint. Not amount-and-same-day alone, which would quietly
merge two separate GHS 20 trotro fares.

### Sync

Local SQLite is the working copy; the server is the merge point. Rows carry `updated_at` and a
device id. Push what changed since the last watermark, pull the rest, last-write-wins per row.
Deletes are soft (`deleted_at`) so a delete on one device is not resurrected by another.

Not a CRDT. For one person's handful of devices, row-level LWW is honest and debuggable, and
the conflict story is one a support email can explain.

---

## 6. Server

Axum, Postgres, `mt-core` linked in as a library.

Multi-tenancy is enforced in the repository layer, not the handlers: every query takes a
`UserId` and there is no accessor that does not. Retrofitting an owner check onto each query
later is how IDOR bugs ship, so the layer is built tenant-aware from the first commit and a
test asserts that no query helper exists without it.

Endpoints, in rough shape:

```
POST   /auth/register · /auth/login · /auth/refresh · /auth/reset
POST   /sync/push        rows changed since watermark
GET    /sync/pull        rows since watermark
GET    /packs/latest     rule pack manifest + download
POST   /packs/sample     user-initiated redacted unparsed sample
GET    /me/export        full data export (legal requirement)
DELETE /me               account deletion (legal requirement)
```

Money is `BIGINT` minor units plus a currency column. Balances are **derived** from the walk,
not stored. The prior art keeps a denormalised balance column and ships a cron command to
repair it when it drifts; that is a warning, not a pattern. If a cached balance ever becomes
necessary for performance, it is updated inside the same database transaction as the row that
changed.

---

## 7. Rule packs

Every bank and wallet another user has is a format we do not have. That work is unbounded, so
it is pushed out of the release cycle entirely: SMS patterns and default categories ship as a
versioned pack the app fetches, signed and verified, so adding a Ghanaian bank is a server-side
release rather than a store review.

The feedback loop is explicit and user-initiated: a "this message didn't parse — send a
redacted sample" action, which strips digits and names locally and shows the user exactly what
will be sent before it is sent. There is no automatic upload of unmatched text. That is
precisely what the policy forbids, and it would also be the wrong thing to do.

---

## 8. Security and privacy

- Local database encrypted at rest; device authentication to open the app.
- On-device parsing, allow-list before read, no raw SMS off the device, ever. (§5)
- Tenant isolation enforced in the repository layer with no exceptions. (§6)
- Argon2id for passwords, rotating refresh tokens, rate limiting on auth.
- No third-party analytics or crash reporter that could carry transaction text or labels.
- Account deletion and data export are product features, not scripts someone runs.
- Ghana's Data Protection Act registration as a data controller; GDPR duties from the moment a
  European user signs up.

The Play Data safety form and the App Store privacy labels must describe what the app actually
does. Keeping them true is easier if the data never leaves the device in the first place, which
is the main reason the architecture is shaped this way.

---

## 9. Build and test

| Target | Core | Notes |
|---|---|---|
| Android | native, NDK | arm64 + x86_64 for emulators |
| iOS | native | no SMS path exists on this platform |
| macOS / Windows / Linux | native | desktop |
| Web | WASM | proved in phase 0, server fallback if it disappoints |
| Server | native | same crate, linked directly |

Testing follows the boundary. `mt-core` carries the rule tests — the balance walk, transfer
netting, the categorisation ladder, recurrence scopes, duplicate scoring, budget thresholds,
and a corpus of real anonymised SMS bodies per supported format. These run on every commit in
seconds, with no device, no database and no screen. Flutter carries widget and integration
tests for the flows. The server carries API tests against a real Postgres, including an
explicit cross-tenant access test.

Parser tests are fixtures, not mocks: one file per bank format, with the expected `Candidate`
beside it. When a bank changes its wording, the fix is a fixture and a pattern, and the
regression is caught permanently.

---

## 10. Open decisions

1. **Web client: WASM or server-computed?** Decided empirically in phase 0 by building it.
2. **Is the web client needed at all,** or does desktop cover the sit-down case? Dropping it
   removes the storage asymmetry and a chunk of phase 5.
3. **Self-hosted option alongside the published product?** Costs little given the server is one
   binary, and it is a credible answer to "where is my financial data".
4. **Rule pack signing** — simple detached signature, or full transparency log? Start simple.
5. **Working name.** `moneytrack` is a placeholder.
