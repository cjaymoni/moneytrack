# Architecture

Companion to [PLAN.md](PLAN.md), which covers *what* is being built and in what order. This
document covers *how* it is put together and why. Written 2026-11-10 and revised twice on
2026-11-15: once when the UI decision was reversed, and again when the web client became the
thing being built. Both reversals are argued in
[decisions/0001-native-uis.md](decisions/0001-native-uis.md) and
[decisions/0002-web-first.md](decisions/0002-web-first.md).

Phases 0 and 1a are built, so §4, §4a and §3 describe code that exists. §6 and §7 — the server
and the web client — are the active work and are a sketch that first contact with a compiler
will change. The mobile apps are paused where 1a left them.

`moneytrack` is a working name.

---

## 1. The decision

**A Rust core and server. A thin browser client first; two native apps, paused, that carry
their own copy of the database.**

Everything that decides what a number is — money arithmetic, the balance walk, the
categorisation ladder, recurrence expansion, duplicate scoring, budget evaluation, SMS parsing
— is one Rust crate, `mt-core`. The Axum server links it directly. The apps compile it in
through UniFFI along with `mt-store`, which keeps their own SQLite copy so a handset works
with no signal.

The browser keeps nothing and decides nothing. It renders what the server computes, which is
the same `mt-core` answer a handset computes locally — so the thin client and the offline one
cannot disagree about a balance, because only one of them is doing arithmetic.

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

### Why the browser comes first

Because the stores are shut to us. A Play listing needs a review and, for SMS capture, a
restricted-permissions declaration granted case by case; the App Store needs an account and a
review of its own. A web app is a URL. Everything built for it — the schema, the tenancy, the
auth, the deployment — is what mobile sync needs later anyway, so this is a reordering rather
than a detour. The full argument, including why the browser is thin rather than local-first
WASM, is in [decisions/0002-web-first.md](decisions/0002-web-first.md).

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
   │  paused at phase 1a    │          │  paused at phase 1a    │
   └───────────┬────────────┘          └───────────┬────────────┘
               │   UniFFI (Swift)                  │  UniFFI (Kotlin)
               └─────────────────┬─────────────────┘
                 ┌───────────────▼──────────────────────────┐
                 │        mt-ffi  ·  mt-store               │
                 │  the handle · SQLite · change bus        │
                 └───────────────┬──────────────────────────┘
                                 │
   ┌────────────────────────┐    │
   │   Browser (React/TS)   │    │      renders what it is told;
   │  screens · routing     │    │      holds no ledger, runs no rules
   └───────────┬────────────┘    │
               │ HTTPS, JSON     │
               │ session cookie  │
   ┌───────────▼─────────────────▼──────────────────────────┐
   │              mt-server (Rust / Axum)                   │
   │  auth · tenant-scoped repositories · screen queries    │
   │  later: sync, rule packs            Postgres via sqlx  │
   └───────────────┬────────────────────────────────────────┘
   ┌───────────────▼────────────────────────────────────────┐
   │                   mt-core  (Rust)                      │
   │  money · ledger · rules · recurrence · dedup · budget  │
   │             no I/O, no clock, no DB                    │
   └────────────────────────────────────────────────────────┘
```

`mt-core` is at the bottom of both paths and is the reason they agree. The server reaches it
directly; a handset reaches it through `mt-store`. Neither route has a second implementation
of a rule in it, and the browser has no route to one at all.

### Repository layout

```
moneytrack/
├── docs/                  PLAN.md, ARCHITECTURE.md, SCHEMA.md, decisions/
├── crates/
│   ├── core/              mt-core — pure, no I/O. The only place rules live.
│   ├── store/             SQLite, migrations, repositories, the change bus.
│   ├── sync/              wire protocol, watermarks, merge, transport trait.
│   ├── server/            mt-server — Axum, Postgres, auth, the API.
│   └── ffi/               UniFFI surface: the published API, nothing else.
├── web/                   the browser client: TypeScript, React, Vite.
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

Axum, Postgres through `tokio-postgres`, `mt-core` linked in as a library. The spine is
built: auth, sessions, the tenant-scoped repositories and the first endpoints.

Not `sqlx`, and the reason is worth recording because it looks like a preference and is not.
Cargo refuses a dependency graph where two packages claim the same `links` value, and it
decides that during version selection, before features are considered. `sqlx` names
`sqlx-sqlite` as an optional dependency; `sqlx-sqlite` links the native `sqlite3`; so does
the `rusqlite` the device store is built on. Turning the SQLite driver off does not help. The
choice was to hold the device store hostage to the server's SQL library for ever, or to use
a Postgres client that has no opinion about SQLite. The queries here are written by hand
either way.

### Tenancy, which is the part to get right first

Multi-tenancy is enforced in the repository layer, not the handlers: every query takes a
`UserId` and there is no accessor that does not. `db/postgres/0001_init.sql` already does its
half — every per-person table is keyed on `(user_id, id)` and every foreign key carries
`user_id` through, so a transaction physically cannot reference another user's account — and
the repository layer has to do the other half from the first commit. Retrofitting an owner
check onto each query later is how IDOR bugs ship. A test asserts that no query helper exists
without a `UserId` parameter, and the API tests include an explicit cross-tenant read that
must fail.

### Auth

Argon2id for passwords, with the parameters in one place and no second call site.

Sessions are opaque tokens, not JWTs. The token is random bytes from the OS, stored only as a
SHA-256 hash in `refresh_tokens`, and delivered to the browser as a cookie that is `HttpOnly`,
`Secure`, `SameSite=Lax` and scoped to the API path. A browser never sees a token in
JavaScript, which removes the whole class of bug where an XSS becomes a stolen session that
outlives the fix. Sessions rotate on use, and presenting a rotated token revokes the chain —
the schema has `rotated_from` for exactly that, because reuse of a rotated token means theft.

State-changing requests carry a CSRF token checked against a second cookie. `SameSite=Lax`
alone is a browser default, not a guarantee, and this is someone's money.

Auth routes are rate limited per address and per account, and a failed sign-in says the same
thing whether the address exists or not.

### What the browser asks for

Screen-shaped endpoints, mirroring what `mt-store` answers on a handset:

```
GET    /csrf                     the CSRF token, for a page that cannot read its cookie
POST   /auth/register · /auth/login · /auth/logout       (/auth/password: later)
GET    /me

GET    /currencies               the registry, for a picker
GET    /accounts                 every account with both balances
POST   /accounts
POST   /accounts/{id}/reconcile  a count: the one row that makes the ledger agree, from mt-core
GET    /ledger                   one account's walk: rows in booked order, balance after each
GET    /categories               every category, in display order; a new ledger has eighteen
POST   /categories               one level of nesting, judged by mt-core
GET    /transactions             the rows, optionally for one account
POST   /transactions             optionally under a category
POST   /transactions/{id}/category   file a row under a category, or under none
PATCH  /transactions/{id}        change a row, sent whole; not its account, not a transfer leg
DELETE /transactions/{id}        a tombstone, never a DELETE: sync must say the row is gone
POST   /transactions/{id}/restore    undo a deletion, exactly
GET    /totals                   per currency, on a basis — never converted into one number
GET    /breakdown                a month by category, one per currency, shares from mt-core

GET    /outlook                  per account: the walk ahead, its low, the row that runs it short
GET    /budgets                  every cap, where it stands now and once expected rows land
POST   /budgets                  starts with the current period, so this month's spending counts
PATCH  /budgets/{id}             a new cap from this period on; past periods keep theirs
DELETE /budgets/{id}
```

The CSRF cookie is scoped to `/api` like the session, and `document.cookie` on a page at `/`
cannot see a cookie scoped to a path the page is not under — so `/csrf` hands the same value
back in a body. That is no weaker: a cross-site page can make the request, but the
same-origin policy stops it reading the answer, which is what the cookie relied on too.

Money crosses as minor units plus an ISO code — never a double — and beside it, a `display`
string the server rendered through `mt-core`. Where the decimal point goes is a property of
the currency, and a client that divides by a hundred is right until the first CFA franc row
arrives. The same rule read backwards decides what a typed `12.50` is in minor units, so an
amount may also arrive as the decimal text a person typed (`amount`, `opening_balance`) and
be read by `Money::parse` in the account's currency. Finer than the currency is refused, not
rounded. Ids, instants and dates use the same wire spellings as the sync
protocol and the FFI, which `crates/core/tests/wire.rs` already fixes.

Errors come back with a stable machine-readable code and a message. The code is what the
client switches on; the message is for a person.

### Later, on the same server

Sync (`/sync/push`, `/sync/pull`), rule packs, and the redacted-sample endpoint. The schema
already carries the per-user `sync_seq` counter and the device watermarks they need. Deletion
and export are phase 5 and are obligations, not features.

## 6a. The browser client

TypeScript, React, Vite. Static files: the build output is HTML, JS and CSS that any host can
serve, and the API is a separate origin or a path behind the same one.

**One rule, and it is the whole design.** No rule lives here. No money arithmetic, no decision
about what counts as spending, no transfer filtering, no category ladder, no formatting of an
amount by dividing by a hundred. If a number appears on a screen, the server computed it and
the client printed it. The test, when something looks wrong: can the number be traced to one
`mt-core` call? The moment the answer is no, the product has two implementations of its own
arithmetic again, which is the thing this architecture exists to prevent.

What the client *is* responsible for: routing, forms and their validation-for-politeness
(the server validates for real), loading and error states, keyboard and screen-reader
behaviour, and remembering what the person was looking at.

**Design.** The tokens in `design/tokens/` and the direction in `DESIGN.md` carry over. The
HTML references in `design/mobile/` do not: they are phone-shaped, and a browser on a laptop
is a different composition with different densities. New layouts are design work, not a
port — the type, palette, density and the stamp are what the world lends.

**State.** Server state is fetched and cached per query, invalidated on mutation; there is no
client-side model of the ledger to keep in step. That is the thin-client discipline paying
for itself: no cache coherence problem, because there is no second copy. TanStack Query does
the caching; React Router the routes.

**What the rule allowed, and where the line fell, in 2b.** The client groups thousands into a
`display` string, because `mt-core` leaves grouping to the screen on purpose. It prefixes `+`
or `−` from a row's `direction`, a word on the wire, not a negation. It reads whether a
balance is below zero from the sign of `minor_units`. It splits history from forecast by
comparing `booked_at` with the reader's clock, which is a question about time, not money. It
does not draw the magnitude bar under each ledger row the phone references show: a bar's
width is an amount divided by the largest one, which is arithmetic on money. The breakdown
shows the way back: `mt-core` works out each share in basis points and the screen turns that
into a CSS width — a percentage of a bar, not of anyone's money.

**Layout.** A laptop gets a top bar, a rail of accounts with what each holds and the totals
per currency, and the account's screen: the hero balance, one ruled line to record a row, and
the ledger. Under 820px the rail stacks above the screen. Fonts are served from the same
origin, not a font CDN: a finance app's pages should not tell a third party who opened them.

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
| Server | host, and the host of wherever it is deployed | the Axum binary |
| Browser | — | static files from Vite; no Rust in the browser at all |
| Android arm64 | `aarch64-linux-android` | `.so` via cargo-ndk, in a Gradle module |
| Android x86_64 | `x86_64-linux-android` | emulator |
| iOS device | `aarch64-apple-ios` | staticlib, into an XCFramework |
| iOS simulator | `aarch64-apple-ios-sim` | staticlib, into the same XCFramework |

Still no WASM: the browser runs no Rust, which is what being a thin client means and what
removes the one risk the original architecture had to keep open. Bindings are generated in the
same step as the libraries, checked for drift in CI, and never hand-edited.

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
- **`mt-server`** carries API tests against a real Postgres in a container — not a mock, and
  not SQLite standing in for it, because the constraints doing the tenancy work are Postgres
  constraints. Every endpoint gets its happy path, its unauthenticated path and its
  wrong-tenant path, and the wrong-tenant one is the test that matters most.
- **The browser client** carries component tests over its own behaviour and, where a flow is
  worth the cost, an end-to-end run against a server with a seeded database. It carries no
  test of a number, because it computes none.
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
5. ~~**Does the sit-down case come back?**~~ Settled by
   [decisions/0002-web-first.md](decisions/0002-web-first.md): a browser is the first client,
   and it covers the sit-down case. A native desktop app is still refused.
6. **Where it is hosted, and what that costs per month.** The deployment has to carry a
   managed Postgres with backups, TLS, and somewhere to put the static files. Two things in
   the code are waiting on this answer and are wrong until it arrives: the database
   connection is plain TCP, which is right for a loopback container and wrong for a managed
   Postgres across a network, and the rate limiter reads the caller's address from the socket
   rather than a forwarded header, which behind a proxy is the proxy. Both are marked where
   they sit.
7. **Email.** Verification and password reset need a provider. Until there is one, sign-up
   works without verification and a forgotten password is a support request — acceptable for
   one user, not for a second.
8. **Working name.** `moneytrack` is a placeholder.
