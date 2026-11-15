# Architecture

Companion to [PLAN.md](PLAN.md), which covers *what* is being built and in what order. This
document covers *how* it is put together and why. Written 2026-11-10; revised 2026-11-15 when
the UI decision was reversed — see [decisions/0001-native-uis.md](decisions/0001-native-uis.md)
for that reversal on its own terms. Phase 0 is built, so §4 describes code that exists;
everything describing the store, the sync client and the two apps is a sketch that first
contact with a compiler will change.

`moneytrack` is a working name.

---

## 1. The decision

**A Rust core, store and server. Two native UIs: SwiftUI on iOS, Jetpack Compose on Android.**

Everything that decides what a number is — money arithmetic, the balance walk, the
categorisation ladder, recurrence expansion, duplicate scoring, budget evaluation, SMS parsing
— is one Rust crate. Everything that *keeps* a number — the SQLite database, its migrations,
the queries the screens run, and the sync client that reconciles them with the server — is two
more Rust crates. All of it is compiled into both apps through UniFFI and linked directly into
the Axum server. What is written twice is screens, and only screens.

### Why Rust holds this much

The product syncs, which means the client and the server both compute balances, budget state
and whether two rows are the same transaction. Two implementations of those rules will
eventually disagree, and a client and server disagreeing about someone's balance is the bug
class that destroys trust in a finance app and consumes its support budget. One compiled
implementation cannot drift.

That argument used to stop at the rules, because a single Flutter tree gave one implementation
of the storage layer for free. Two native apps do not. Leaving SQLite to the platforms would
mean the schema, the migration sequence, every query behind every screen and the whole
push/pull/merge cycle written once in Room and once in GRDB, kept in agreement by discipline —
which is the thing this architecture was shaped to avoid, reintroduced one layer down. So the
boundary moves: Rust owns the database.

### Why the UIs are native

Four reasons, in the order they carry weight.

**The design is already two designs.** `design/ADAPTIVE.md` specifies Material 3 on Android and
HIG on iOS — different navigation, different sheet behaviour, different icon sets, different
back gesture, different type scales — and the review screenshots in `.impeccable/review/` come
in `phone-ios-*` and `phone-android-*` pairs. Flutter's economy is one widget tree for both
platforms. We had already given that up on paper before any screen was built.

**The hardest feature is native anyway.** The Android SMS receiver is Kotlin, lives in a
broadcast receiver that must survive app kill, and under Flutter had to hop over a
MethodChannel to reach the parser. Native, it calls Rust directly.

**Web was the soft spot and web is gone.** Flutter's WASM target carried the one risk §1 used
to flag. Dropping desktop and web (PLAN.md, phasing) removes the risk and, with it, most of
what the single tree was buying.

**A published consumer finance app is judged on feel.** Dynamic Type, predictive back,
platform haptics, widgets and the system sheet behaviours are what "it feels like a real app"
is made of, and they are free in the native toolkits and work in Flutter.

### Alternatives rejected

**Flutter, which is what this document said until today.** Still the better answer for a
product with one design across platforms, a web target and a lone developer counting hours.
None of those three conditions survived contact with the design work. The switching cost is
near zero today — `app/` is a scaffold, a wiring check and one round-trip test — and grows
every week it is deferred.

**Kotlin Multiplatform with the rules in Kotlin.** Would give shared logic across both apps and
nothing to the server, which would then need a second implementation in a second language. That
is the Dart-core proposal with a different accent, and it was rejected for the same reason. KMP
over an *unchanged* Rust core adds a third language to the sandwich for no gain.

**Rust UI — Tauri 2 mobile, Dioxus.** Rejected in the original decision and still rejected: no
body of published consumer finance apps on that path, and the broadcast receiver sits exactly
where the stack is least travelled.

**React Native.** Third on the merits before, and now competing against two native toolchains
rather than one cross-platform one. No.

**Hand-rolled C ABI plus JNI.** Total control over the boundary, and a large amount of
hand-written `unsafe` on both sides of it plus a JNI layer maintained by hand. UniFFI generates
that layer from the same Rust signatures, which is the whole point of it.

### Cost accepted

Every screen is built twice, and two screen-test suites run on two simulators. Features will be
tempted to drift between the platforms, and the containment is that nothing a screen *decides*
lives in a screen: both apps call the same Rust for every number, every list and every
validation, so divergence shows up as a layout difference rather than as two different answers.
Feature parity, SMS aside, is a review item at the end of every phase.

The cross-compilation matrix shrinks — Android (NDK) and iOS only, no WASM — but gains an
XCFramework build step and a Gradle module wrapping a `.so`.

---

## 2. Shape

```
   ┌────────────────────────┐          ┌────────────────────────┐
   │   iOS app (SwiftUI)    │          │  Android app (Compose) │
   │  screens · navigation  │          │  screens · navigation  │
   │  camera · files        │          │  SMS receiver (Kotlin) │
   │  Keychain · BGTask     │          │  camera · files        │
   │                        │          │  Keystore · WorkManager│
   └───────────┬────────────┘          └───────────┬────────────┘
               │   UniFFI (Swift)                  │  UniFFI (Kotlin)
               └─────────────────┬─────────────────┘
                 ┌───────────────▼──────────────────────────┐
                 │              mt-ffi                      │
                 │  the published surface, and nothing else │
                 └───────────────┬──────────────────────────┘
                 ┌───────────────▼──────────────────────────┐
                 │        mt-store          mt-sync         │
                 │  SQLite · migrations  ·  push/pull/merge │
                 │  queries · change bus ·  watermarks      │
                 └───────────────┬──────────────────────────┘
                 ┌───────────────▼──────────────────────────┐
                 │            mt-core  (Rust)               │
                 │  money · ledger · rules · recurrence     │
                 │  dedup · budget · parse                  │
                 │          no I/O, no clock, no DB         │
                 └───────────────┬──────────────────────────┘
                                 │  same crates, linked
                 ┌───────────────▼──────────────────────────┐
                 │        mt-server (Rust / Axum)           │
                 │  auth · sync · rule packs · Postgres     │
                 └──────────────────────────────────────────┘
```

`mt-core` stays exactly what it is: pure, no I/O, no clock, no database. The new crates sit
above it and depend on it; it depends on nothing of theirs. The server links `core` and the
sync protocol types directly and never sees `mt-ffi`.

### Repository layout

```
moneytrack/
├── docs/                  PLAN.md, ARCHITECTURE.md, SCHEMA.md, decisions/
├── crates/
│   ├── core/              mt-core — pure, no I/O. The only place rules live.
│   ├── store/             SQLite, migrations, repositories, the change bus.
│   ├── sync/              wire protocol, watermarks, merge, transport trait.
│   ├── server/            mt-server — Axum, Postgres, auth, sync, rule packs.
│   └── ffi/               UniFFI surface: the published API, nothing else.
├── ios/                   Xcode project (app) + MoneytrackKit (the Swift package).
├── android/               Gradle: `core` (bindings + .so) and `app` (Compose screens).
├── tools/                 bindings.sh, ffi-build.sh, android-libs.sh.
├── db/                    the two schemas: sqlite/ and postgres/.
├── design/                HTML references, tokens, ADAPTIVE.md.
├── packs/                 versioned rule packs (patterns, categories) as data.
└── .github/workflows/     build matrix, rule-pack release, store pipelines.
```

`app/` and `crates/bridge/` were what this replaced, and both are gone: the bridge's
hand-written modules moved to `crates/ffi` with their tests, and the Flutter tree was
deleted. `site/` is dropped with the web client.

Nothing generated or built is in source control. `tools/bindings.sh` writes the Swift and
the Kotlin from the library itself, `tools/ffi-build.sh` assembles the XCFramework, and
`tools/android-libs.sh` writes the `.so` per ABI.

---

## 3. The boundary

The old rule was **values cross, handles do not**, and it held because storage lived on the
other side. Now it reads:

> **One handle crosses, and it is the store. Everything else is values.**

An app opens a `Store` once, holds it for the process lifetime, and passes owned values across
every call it makes on it. No connection, cursor, statement, row iterator or file handle is
ever visible to Swift or Kotlin. The core below is untouched by this: it still receives owned
values and still has no idea a database exists.

### What is in Rust

- **Storage.** `mt-store` opens the encrypted SQLite file, runs migrations, and exposes typed
  repositories. `db/sqlite/0001_init.sql` stops being a statement of intent that a Dart schema
  mirrors and becomes the thing that actually runs.
- **Queries behind the screens.** The ledger list, the inbox, account balances, the breakdown,
  the outlook. A screen asks for a page of rows already shaped for display; it does not
  assemble one from a join it wrote itself.
- **Sync.** Watermarks, push and pull, the merge, soft deletes. HTTP is behind a transport
  trait so the tests run against a fake and the apps hand it a real client.
- **Every rule**, as before.

### What stays native

- **Rendering, navigation, state restoration, accessibility.** The entire reason for the pivot.
- **The SMS broadcast receiver** (Kotlin, Android only), which calls into Rust directly.
- **Camera, photo library, file picking, share sheets.**
- **The keychain / keystore** holding the database key, biometric unlock, and the OS scheduler
  that wakes a sync — `BGTaskScheduler` on iOS, `WorkManager` on Android.
- **Notifications and widgets.**

### Reactivity, which drift used to provide

Flutter's drift gave live queries, and the inbox and the running balance were going to lean on
them. The replacement is explicit and small: the store carries a change bus. Every write
publishes a revision and the set of tables it touched; the apps subscribe through a UniFFI
callback interface, wrapped as an `AsyncStream` in Swift and a `Flow` in Kotlin, and re-run the
queries that care. Re-querying local SQLite is cheap, the invalidation is visible rather than
inferred, and it behaves identically on both platforms because it is one implementation.

### Threading

Every call that touches the database is exposed as async — `await` in Swift, `suspend` in
Kotlin — so no screen can block the main thread by accident. Writes serialise behind a single
writer; reads run concurrently against WAL. (UniFFI supports this with an async runtime
attribute on the export; the exact arrangement gets pinned when the crate is wired.)

### What the surface looks like

The rule calls already existed, tested, in `crates/bridge`; they moved to `crates/ffi` with
their DTOs, their enums and their error type, and their bodies did not change. The store
calls are new. As built:

```rust
// crates/ffi/src/store.rs — the handle, and what goes through it.

pub async fn open_store(path: String, key: Vec<u8>) -> Result<Arc<Store>, StoreError>;

impl Store {
    pub fn device_id(&self) -> String;                 // sync: read once, at open
    pub fn revision(&self) -> u64;                     // sync: a counter in memory
    pub fn subscribe(&self, observer: Arc<dyn ChangeObserver>) -> Result<u64, StoreError>;
    pub fn unsubscribe(&self, subscription: u64) -> Result<(), StoreError>;

    pub async fn create_account(&self, account: NewAccountDto) -> Result<AccountDto, StoreError>;
    pub async fn accounts(&self) -> Result<Vec<AccountDto>, StoreError>;
    pub async fn record_transaction(&self, transaction: NewTransactionDto)
        -> Result<TransactionDto, StoreError>;
    pub async fn transactions_for_account(&self, account_id: String)
        -> Result<Vec<TransactionDto>, StoreError>;
    pub async fn account_balance(&self, account_id: String, basis: BalanceBasis)
        -> Result<MoneyDto, StoreError>;
    pub async fn account_balances(&self, basis: BalanceBasis)
        -> Result<Vec<AccountBalanceDto>, StoreError>;
}

// crates/ffi/src/api.rs — the rules, over rows the caller is holding. Unchanged.
pub fn walk_balance(..) -> Result<WalkDto, BridgeError>;
pub fn evaluate_budgets(..) -> Result<Vec<BudgetStatusDto>, BridgeError>;
pub fn expand_recurrence(..) -> Result<GenerationDto, BridgeError>;
pub fn categorise_candidate(..) -> Result<CategorisationDto, BridgeError>;
pub fn score_duplicates(..) -> Result<Vec<DuplicateReportDto>, BridgeError>;
pub fn instances_in_scope(..) -> Result<Vec<String>, BridgeError>;
pub fn compile_rules(..) -> Result<(), BridgeError>;      // refuse a bad pattern at save time
pub fn validate_transfer(..) -> Result<(), BridgeError>;  // refuse legs that disagree
pub fn parse_amount(input: String, currency: String) -> Result<MoneyDto, BridgeError>;
pub fn format_amount(amount: MoneyDto) -> Result<String, BridgeError>;
```

Ingestion and sync are not here, because phases 2 and 5 are where they get written. The
count to watch is the one §10 names: twenty-four published today — fourteen functions and
ten methods on the store.

Every call that touches the database runs on a blocking pool and reaches the apps as `await`
in Swift and `suspend` in Kotlin. The three that only read something already in memory stay
synchronous, because making them async would be noise.

The conventions the bridge settled stayed settled, because they were right for reasons that
had nothing to do with Dart. Every function returns a `Result`, and the error names the field
that caused it rather than computing a plausible wrong number. Ids, instants and dates cross
as text in the spellings the sync wire already commits to. Money crosses as minor units plus
a code — never a decimal string, never a double. The time zone crosses as a table of offsets
the platform supplies, with the window it was built for, and a question reaching past that
window is refused rather than answered from the base offset and quietly put in the wrong
month.

UniFFI maps these the way the two languages expect: records become Swift structs and Kotlin
data classes, enums become Swift enums and Kotlin sealed classes — so a new variant is a
compile error on both sides rather than a silent `else` branch — and `Result::Err` becomes a
Swift `throws` and a Kotlin exception.

One thing the move cost, and it is worth writing down. `StoreError` is a mirror of
`mt_store::StoreError`, variant for variant, except for the one that carries a conversion
failure: that crosses as the message rather than as a `BridgeError` nested inside it. An app
has nothing to do with the difference between a malformed id and a malformed date beyond
showing it and logging it, and the field name — which is the part a developer needs — is in
the message either way.

### The unsafe policy under UniFFI

Unchanged in substance. FFI scaffolding is unsafe by construction, so the workspace denies
rather than forbids `unsafe_code`, and `forbid` is applied one level in, at every crate root
and at every hand-written module of the FFI crate. The single `allow` moves from
`frb_generated` to the module holding UniFFI's generated scaffolding. The policy is written out
in full in the workspace `Cargo.toml`, beside the lint it governs, and the counts there need
updating with the crates.

---

## 4. The core crate

Unchanged by this revision, and the reason the revision is cheap. Phase 0 is built and tested.

```
crates/core/src/
├── money.rs        Money(i64 minor units, Currency). No floats, ever.
├── model.rs        Tx, Account, Category, Budget, RecurringRule, Candidate.
├── ledger.rs       the balance walk — the heart of the product.
├── categorise.rs   the categorisation ladder.
├── recurrence.rs   cadence expansion + RecurrenceScope edit semantics.
├── dedup.rs        duplicate scoring across sources.
├── budget.rs       cap evaluation, thresholds.
└── parse/          sms.rs, csv.rs, ofx.rs — one trait, many handlers.   (phase 2+)
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
Sort by `booked_at`, accumulate, emit a point per row. One function serves the history screen,
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

## 4a. The store and sync crates

New, and the substance of this revision. `mt-store` is built — the schema runs, the
repositories and the balance queries are there, and the change bus is wired through to both
apps. `mt-sync` is phase 5; the layout below is the plan for it.

```
crates/store/src/
├── open.rs         opening, the key, the pragmas, and the handle itself
├── migrate.rs      the migration sequence; db/sqlite/0001_init.sql is what it runs
├── sql.rs          the only place that knows how a column is encoded
├── repo/           one module per table: accounts, transactions
├── query/          the reads behind the screens
└── change.rs       the revision counter and the change bus
```

Repositories hand out `mt-core` values and never leak a `rusqlite` type at all: rows come out
as core values, go through the rules, and leave the FFI as DTOs. Encryption is SQLCipher
through `rusqlite`, built from source with its own crypto rather than linked against whatever
the platform ships — an Android handset has no SQLCipher at all, and iOS ships a plain SQLite
that would open the file unencrypted without saying so. The key is 32 raw bytes, generated on
first run and kept in the Keychain or wrapped under an Android Keystore key, and handed to
`open_store` as bytes. Rust never touches the secure enclave.

Two decisions worth naming, because they are the ones a profile will eventually argue with.
One connection behind a mutex, which is honest for a database with one writer and queries
that run in microseconds, and visibly the first thing to change if a screen ever waits on it.
And instants are rounded to the millisecond the column holds *before* the row is built, so
the row a caller is handed back after a write is the row on disk — without that, `updated_at`
means one thing in memory and another in the file, and `updated_at` is what last-write-wins
resolves conflicts on.

```
crates/sync/src/
├── wire.rs         the push/pull envelopes, shared with the server
├── watermark.rs    what has been seen, per device
├── merge.rs        last-write-wins per row, soft deletes, clock skew handling
└── transport.rs    the trait; a real HTTP client in the apps, a fake in the tests
```

Keeping the merge in Rust is what makes the LWW story testable: the awkward cases — two devices
editing the same row, a delete racing an edit, a device replaying an old watermark — are
exercised in `cargo test` with two in-memory stores, on every commit, with no handset.

---

## 5. Data flow

### SMS capture (Android only)

```
BroadcastReceiver (Kotlin)
  └─ sender allow-list check            ← before the body is read into memory
     └─ UniFFI: offer_message(sender, body)
        └─ mt-core::parse_message       ← on-device, always
           └─ mt-core::categorise
              └─ inbox_items (SQLite, in Rust)
                 └─ change bus → the Review screen's badge
                    └─ user confirms
                       └─ transactions  ← only this ever syncs
```

One hop shorter than the Flutter path, which had to cross a MethodChannel into Dart before it
could reach the parser.

The allow-list check happening *first* is a policy requirement, not an optimisation — the Play
exception for "SMS-based money management" forbids a budgeting app from exfiltrating
non-financial or personal SMS history. Raw message text never leaves the device, never reaches
the server, and is not retained after parsing. An unmatched message is counted, not stored, and
can only be shared by an explicit user action that sends a redacted sample (§7).

### Manual entry

The target is under five seconds: amount, category, done, with account and date defaulted. One
`record_transaction` call writes straight to `transactions` as `Confirmed`, bypassing the
inbox. This path must work perfectly on iOS, where SMS does not exist and never will.

### Import

File → resolver → handler → `Vec<Candidate>` → `score_duplicates` against existing rows →
suspected pairs surfaced for merge → inbox. Matching is amount + account + a date window of a
day or two + a counterparty fingerprint. Not amount-and-same-day alone, which would quietly
merge two separate GHS 20 trotro fares.

### Sync

Local SQLite is the working copy; the server is the merge point. Rows carry `updated_at` and a
device id. Push what changed since the last watermark, pull the rest, last-write-wins per row.
Deletes are soft (`deleted_at`) so a delete on one device is not resurrected by another. The
whole cycle runs in `mt-sync`; the apps supply an HTTP client and an OS scheduler to wake it.

Not a CRDT. For one person's handful of devices, row-level LWW is honest and debuggable, and
the conflict story is one a support email can explain.

---

## 6. Server

Axum, Postgres, `mt-core` and `mt-sync`'s wire types linked in as libraries.

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

- Local database encrypted at rest with SQLCipher; the key lives in the Keychain or the
  Android Keystore, gated behind device authentication, and reaches Rust as bytes at open.
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

| Target | Triple | Produces |
|---|---|---|
| Android arm64 | `aarch64-linux-android` | `.so` via cargo-ndk, in a Gradle module |
| Android x86_64 | `x86_64-linux-android` | emulator |
| iOS device | `aarch64-apple-ios` | staticlib, into an XCFramework |
| iOS simulator | `aarch64-apple-ios-sim` | staticlib, into the same XCFramework |
| Server / dev | host | native, same crates linked directly |

No WASM and no desktop triples, which is most of what dropping web bought. Bindings are
generated in the same step as the libraries, checked for drift in CI, and never hand-edited.

Testing follows the boundary, and the boundary has moved down, which means more of the product
is testable without a device than before:

- **`mt-core`** carries the rule tests — the balance walk, transfer netting, the categorisation
  ladder, recurrence scopes, duplicate scoring, budget thresholds, and a corpus of real
  anonymised SMS bodies per supported format. Seconds, no device, no database, no screen.
- **`mt-store`** carries migration tests against a temporary file and query tests against
  seeded fixtures.
- **`mt-sync`** carries the merge tests: two in-memory stores and a fake transport, driven
  through the conflict cases.
- **`mt-ffi`** carries a round-trip test per published function, so the surface is exercised in
  Rust before either app sees it.
- **The apps** carry screen tests, and one round trip each that opens a real encrypted
  database, writes a transaction and reads the balance back out of Rust. Both run on the
  laptop rather than on a device: the XCFramework carries a macOS slice so `swift test` takes
  a second, and the Kotlin tests run on the JVM against the same library through JNA. That is
  what the Flutter `round_trip_test.dart` was for, and the only thing worth keeping from it.

The three commands, in the order a clean checkout needs them:

```
tools/bindings.sh         # writes the Swift and the Kotlin from the library itself
tools/ffi-build.sh        # the three Apple slices, assembled into an XCFramework
tools/android-libs.sh     # a .so per ABI, into android/core/src/main/jniLibs
```

Then `cargo test --workspace`, `swift test` in `ios/MoneytrackKit`, and `./gradlew test` in
`android`.

Parser tests are fixtures, not mocks: one file per bank format, with the expected `Candidate`
beside it. When a bank changes its wording, the fix is a fixture and a pattern, and the
regression is caught permanently.

### Keeping two apps honest

At the end of every phase, a parity pass: each screen in that phase exists on both platforms,
the numbers on it come from the same FFI call on both, and the only intentional divergence is
the SMS path and the platform idioms `design/ADAPTIVE.md` prescribes. Divergence found later
than that gets expensive.

---

## 10. Open decisions

1. **How wide the store surface gets.** Every screen-shaped query in Rust is one
   implementation and one more published function; a generic row reader is fewer functions and
   tempts the apps into writing logic. Twenty-four published today, against a line in the
   sand at about thirty. Phase 1b is where it gets tested, because that is where the screens are.
2. ~~**UniFFI async arrangement.**~~ Settled: `async_runtime = "tokio"`, which wraps the
   future the foreign side polls in a Tokio context, and the database work runs on
   `spawn_blocking` inside it.
3. **Self-hosted option alongside the published product?** Costs little given the server is one
   binary, and it is a credible answer to "where is my financial data".
4. **Rule pack signing** — simple detached signature, or full transparency log? Start simple.
5. **Does the sit-down case ever come back,** as a web client against the server or as a
   SwiftUI Mac app? Deferred, not refused; nothing below the FFI would have to change.
6. **Working name.** `moneytrack` is a placeholder.
