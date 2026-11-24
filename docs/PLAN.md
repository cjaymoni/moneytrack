# Personal income & spending tracker — plan

Drafted 2026-11-10, revised the same day after reading `algirdasc/expensave`. Revised twice on
2026-11-15: first when the UI decision was reversed — two native apps instead of one Flutter
tree, with the local database moved into Rust behind them — and then again when the web client
became the thing being built, because the app stores are a gate that cannot be opened yet.
Both reversals are argued on their own terms in
[decisions/0001-native-uis.md](decisions/0001-native-uis.md) and
[decisions/0002-web-first.md](decisions/0002-web-first.md).

Status: phase 0 is built and tested in `crates/core` — money, ids, enums, the row types, and
the five rules that turn rows into answers. Phase 1a is built too: `crates/store` runs the
SQLite schema and carries the change bus, `crates/ffi` publishes it over UniFFI, and both apps
open a real encrypted database and read a balance back out of Rust.

**The mobile apps are paused there.** The active line is the web client and the server under
it. Phase 2a is built: `crates/server` runs the Postgres schema, registers and signs people
in with Argon2id and opaque session cookies, and answers the accounts, transactions and
balance endpoints — every query scoped to a user, with tests that try to read and write
across the boundary and must fail. Phase 2b is built too: `web/` signs a person up, opens
accounts, records rows and shows the ledger through today into the forecast, with the
running balance walked by `mt-core` on the server. Phase 3 has begun with categories: a
starting set of eighteen at registration, one level of nesting, a chip picker in the entry
form, and refiling a row from the ledger. The breakdown is built too: `mt-core` groups a
month's rows by category on each side, transfers out, children rolled into their parent,
and the browser draws it from the shares the core works out. So is the outlook: each
account's projected walk read forward, its low, and the first row that runs it short, never
pooled across accounts. And budgets: a cap per category and currency, evaluated by
`mt-core` on the reader's today and again counting this period's expected rows, drawn as the
design's graded tiers, and changed from the current period on so the past keeps its caps.
And reconciliation: a person enters what an account actually holds, and `mt-core` works out
the one row that makes the ledger agree, recorded from the `adjustment` source and marked
"Counted". Phase 3 is complete.

Repeating rows are built too, ahead of phase 4 because the outlook was only as good as what
someone had typed: a series writes real expected rows a year ahead through `mt-core`'s
`recurrence::generate`, topped up lazily on every ledger read, and a due expected row is
confirmed with one tap. Editing a series in place — "change the rent from March" — is not
built yet; the core's scope rules for it are.

Phase 4 is built. `mt-core`'s `ingest` reads a CSV statement through a column mapping the
person sets once per account, and an OFX statement with no mapping at all; a line it cannot
read is listed back with its line number rather than guessed at. Each line is fingerprinted
so uploading the same statement again — or next month's overlapping one — offers only what is
new. Lines wait in an inbox on the server (`csv` and `ofx` only; see
[decisions/0003-server-inbox.md](decisions/0003-server-inbox.md)) with the category the
ladder proposed, and the duplicate check runs on every read of it, against what is in the
ledger by then. Accepting writes a stamped row; filing it somewhere new leaves a rule behind
for that payee; merging settles it as a row already there, confirming an expected one;
rejecting keeps it rejected.

## What it has to do

Run on Android and iPhone. Take transactions four ways: typed by hand, parsed from MoMo/bank
SMS, imported from CSV/OFX statements, and scanned off receipts. Answer four questions: where
did the money go, am I over budget, what does next month look like, and how much is in each
account and currency.

Desktop and web are out of scope for now. They were phase 5 "off the same Flutter tree", and
there is no such tree any more; nothing below the FFI boundary would have to change if they
come back, so the decision is deferred rather than refused.

## Prior art

`algirdasc/expensave` (GPL-3.0, Symfony + Angular, actively maintained) solves a large part
of this and several of its design decisions are better than my first draft. Everything
marked **[expensave]** below is an idea taken from it and reimplemented in Rust — the code is
GPL-3.0, so nothing is copied verbatim, and the stacks differ anyway. This is firm now that the
plan is to publish: GPL code in a closed-source store app is both a licence breach and a known
conflict with Apple's terms. Ideas are free; files are not.

What it does *not* solve, and why we're still building: no SMS capture (it's a browser app,
a PWA cannot read SMS on Android), no budgets at all, and no currency field anywhere —
amounts are plain floats in one implicit currency.

## Shape

A Rust core, a Rust server and three clients: a browser, an iPhone and an Android handset.

Every rule and calculation lives in one Rust crate, `mt-core`. The server links it directly.
The apps compile it in through UniFFI, along with `mt-store` — the SQLite database, its
migrations and the queries behind their screens — so a handset works offline. The browser
holds nothing and runs nothing: it renders what the server computes, which is the same
`mt-core` answer a handset would have given.

Native apps rather than Flutter, and a thin browser rather than a local-first one. The
reasoning, the rejected alternatives and the costs are in [ARCHITECTURE.md](ARCHITECTURE.md)
and the two decision records. The short version: the design was already two designs, so one
widget tree bought nothing; and a browser-local ledger is one browser profile on one machine,
where "clear site data" means "delete everything I have ever recorded".

### Layout

- `crates/core/` — pure Rust, no I/O, no clock, no DB. Money, categorisation, balance walk,
  recurrence, dedup, budgets, parsers. Where every rule lives.
- `crates/store/` — SQLite, migrations, repositories, the screen queries, the change bus.
- `crates/sync/` — wire protocol, watermarks, merge, transport behind a trait.
- `crates/server/` — Axum, Postgres, auth, the API the browser calls, later sync and rule
  packs. Links `core` directly.
- `crates/ffi/` — the UniFFI surface, and nothing else.
- `web/` — the browser client: TypeScript, React, Vite. Static files over an HTTP API.
- `ios/` — SwiftUI app.
- `android/` — Compose app, plus the Kotlin SMS broadcast receiver.
- `packs/` — versioned rule packs: SMS patterns and default categories, shipped as data.

## Data model

The central idea, **[expensave]**: a transaction carries a `status` of `pending` or
`confirmed`, and future-dated pending rows *are* the forecast. The balance walk doesn't care
whether a row is in the past or the future — it sorts by date and accumulates. There is no
separate projection engine and no separate forecast screen; the same list and the same
running balance show history and outlook continuously.

- `accounts` — id, name, kind (cash | momo | bank | card), currency, opening_balance_minor
- `transactions` — id, account_id, direction (in | out | transfer), amount_minor, currency,
  booked_at (UTC), category_id, label, counterparty, note, status (pending | confirmed),
  source (manual | sms | csv | receipt | recurring | adjustment), source_ref, external_id,
  recurring_id, created_at, updated_at, deleted_at
- `transfers` — pairs two transaction rows so a move between accounts isn't counted as both
  income and spending
- `categories` — id, name, parent_id, kind (income | expense | uncategorised), icon
- `category_rules` — pattern, matched_field (label | counterparty | raw_text), category_id,
  priority. Data, not code. **[expensave, extended]** — theirs has no field selector and no
  priority; it iterates and takes the first hit.
- `recurring` — template plus cadence; **generates concrete pending transaction rows**
  forward rather than being evaluated on the fly **[expensave]**
- `budgets` — category_id, period, cap_minor, currency, starts_on
- `fx_rates` — pair, rate, as_of. Reporting only.
- `inbox_items` — parsed-but-unconfirmed candidates from SMS, CSV and receipts
- `attachments` — receipt images

Money is integer minor units plus an ISO currency code. Never a float — expensave's
`float $amount` with `round($x, 2)` patching the arithmetic is the thing not to copy. A
transaction keeps the currency it happened in forever; conversion happens at display time at
the rate for that date, so last year's numbers don't move when the cedi does. Timestamps UTC,
shown local.

Account balances are **derived from the transaction walk**, not stored. Expensave keeps a
denormalised balance column and ships a cron command to fix it when it drifts; that's a
warning, not a pattern. If performance ever demands a cached balance, update it inside the
same database transaction as the row that changed.

The schema is written down three times — the Rust types, `db/postgres/0001_init.sql` and
`db/sqlite/0001_init.sql` — and kept in agreement by a test rather than by care. See
[SCHEMA.md](SCHEMA.md). The SQLite file stops being a statement of intent that a Dart schema
mirrors, and becomes the thing `crates/store` actually runs.

### Recurrence editing

When a recurring template changes, the edit asks which instances it applies to:
`this | future | past | all` **[expensave]**. This is the detail that's usually got wrong,
and having it as an explicit prompt at edit time is the right answer.

### Sync

Local SQLite is the working copy, the server is the merge point. Rows carry `updated_at` and
a device id; sync pushes what changed since the last watermark and pulls the rest, last-write-
wins per row. Deletes are soft so a delete on one device isn't resurrected by another. Not a
CRDT — for one person with a few devices, row-level LWW is honest and debuggable.

The whole cycle lives in `crates/sync`, which means the awkward cases — two devices editing one
row, a delete racing an edit, a replayed watermark — are exercised by `cargo test` against two
in-memory stores, on every commit, with no handset involved. The apps supply an HTTP client and
an OS scheduler to wake it: `WorkManager` on Android, `BGTaskScheduler` on iOS.

Publishing changes the ceiling here. Row-level LWW is fine for one person's three devices; for
other people's money it needs per-user isolation enforced server-side on every query, a
conflict story you can explain in a support email, and backups you have actually restored from.
Build the sync layer tenant-aware from the first commit — retrofitting an owner check onto every
query is how IDOR bugs ship.

## Ingestion

### Categorisation fallback chain **[expensave]**

Every ingested row runs the same ladder, and it stops at the first hit:

1. a category named explicitly in the source file
2. a matching `category_rules` pattern, by priority
3. **the user's own most recent transaction with the same label or counterparty** — learning
   from history, no machine learning, a handful of lines
4. Uncategorised

Step 3 is the one that makes this feel smart, and it's where repeat MoMo merchants get
classified for free. Corrections made in the inbox write back as new rules.

### Handler plug-in shape **[expensave]**

Each source implements one interface: `supports(file_or_message)` plus `parse(...)` returning
candidate rows. A resolver walks registered handlers until one claims the input. Adding GCB
or Ecobank CSV later is a new handler and nothing else. Planned handlers: MoMo SMS, bank alert
SMS, generic CSV with a stored column mapping, OFX (several banks export it and the fields
are defined, unlike CSV), Dollarbird-style export for migration.

### Duplicate detection

One purchase can arrive as an SMS, a CSV row and a receipt. Matching is on amount + account +
a date window of a day or two + a counterparty fingerprint, surfacing suspected pairs for the
user to merge rather than merging silently. Expensave matches on amount + same calendar date
only, which would quietly merge two separate GHS 20 trotro fares — the idea is right, the
threshold isn't.

### SMS, specifically

Highest value of the four and the reason we're building rather than deploying expensave. A
background receiver catches MoMo and bank alerts, calls straight into Rust, runs the pattern
table, and drops candidates into the inbox. Never auto-commit a parsed message straight to the
ledger; formats change without notice and a silent mis-parse poisons the numbers quietly.

Android only — iOS gives apps no access to SMS at all, and never will. On Android this is
publishable: Google's restricted-permissions policy carries an exception row for "SMS-based
money management", example given being budgeting apps, covering READ_SMS, RECEIVE_SMS,
RECEIVE_MMS and RECEIVE_WAP_PUSH. It is granted by review, not by right — see Publishing below
for what that forces on the design.

### Receipts

Last. Camera, then on-device OCR or a vision model call, pulling total, merchant and date into
the same inbox. Worth it for cash spending that leaves no SMS trail; lowest return per hour of
the four.

## Analysis

**Reporting scaffolding** **[expensave]**: one generic report service parameterised by a date
grouping format, carrying a running balance across the period, specialised into daily, monthly
and by-category. Write it once, in Rust, and both apps show it.

- **Where it went** — month view, category breakdown, per-category trend, search.
- **Budgets** — monthly cap per category, warning at 80% and at breach. Caps are per currency;
  a cedi budget and a dollar budget are separate budgets, not one converted number. Expensave
  has nothing here, so this is ours from scratch.
- **Outlook** — falls out of the pending-row balance walk. Shows the running balance forward
  and the date it goes negative, if it does. This is the feature that changes behaviour; the
  pie charts just confirm what you suspected.
- **Accounts** — balance per account and per currency, plus a converted total clearly labelled
  as an estimate.
- **Reconciliation** **[expensave]** — "I counted my wallet, it's GHS 340 as of today" creates
  the single adjusting transaction that makes the ledger agree. Small feature, big for cash.

## Publishing

The long-term plan is a public app on both stores, not a personal build. That changes four
things.

**The SMS exception is conditional, and the conditions are architectural.** Google grants it
on review via the Call Log & SMS Permissions Declaration Form, and the spyware policy says a
budgeting app "may not exfiltrate or share non-financial or personal SMS history". So: parsing
runs entirely on-device, the receiver filters to known financial senders before anything is
read into memory, and the only thing that may ever reach the server is a parsed transaction —
never raw message text, never an unmatched message, never a sender list. Prominent disclosure
and consent on first run, separate from the system permission dialog. Plan for the exception to
be questioned at some review, because it is described as temporary and granted case by case.

**Therefore the product must be good without SMS.** iOS can never have it, and an Android
grant can be withdrawn. Manual entry, CSV/OFX import and receipt capture carry the product;
SMS is the thing that makes the Android version feel magic. Build and market it in that order.
Building both apps in parallel enforces this: a feature that only works on Android is visible
as a hole in the iOS build the same week it is written.

**Rule distribution becomes a support problem.** Every bank and wallet another user has is a
format you don't have. Ship `category_rules` and SMS patterns as a versioned pack the app
fetches, so adding a bank is a server-side release rather than a store review. Pair it with an
explicit, user-initiated "this message didn't parse — send a redacted sample" action. Never an
automatic upload of unmatched text; that is exactly the thing the policy forbids.

**Compliance furniture, none of it optional.** Privacy policy URL, Play Data safety form and
App Store privacy labels that match what the app actually does, in-app account deletion and
data export, Ghana's Data Protection Act registration as a data controller, and GDPR duties
the moment a European user signs up. Budget real time for this; it is a phase, not a checkbox.

**The public web surface is a static site**, written whenever launch approaches. It was never
going to be the app's framework anyway, and now there is no app framework that could have
tempted it.

## Phasing

Phases 0 through 4 are built. What follows is reordered around the browser, because that is the
channel that is open. Nothing in 1a is wasted: the apps resume against the same server.

| Phase | Deliverable | Rough size |
|---|---|---|
| 0 | `core`: money, balance walk, categorisation ladder, recurrence, budgets, dedup — tested | small ✅ |
| 1a | The turn to native: `crates/ffi` over UniFFI, `crates/store` with the SQLite schema and the change bus, an app on each platform that opens an encrypted ledger and reads a balance back | medium ✅ |
| 2a | The server's spine: Axum, Postgres, the schema migrated, sign-up and sign-in, sessions, and tenant-scoped repositories with no accessor that takes no `UserId` | medium ✅ |
| 2b | The first useful screen, end to end: accounts, manual entry, the ledger list and a running balance, in the browser against the server | medium ✅ |
| 3 | Budgets, the outlook, the category breakdown, reconciliation entries — the analysis the product exists for | medium ✅ |
| 3b | Repeating rows: series, a year of expected rows, confirming one when it happens | small ✅ |
| 4 | CSV + OFX import, the review inbox, duplicate merging | medium ✅ |
| 5 | Deletion, export, and the compliance furniture a hosted service cannot open without | medium |
| 6 | Mobile resumes: `crates/sync` and the push/pull cycle against this server, then the apps' screens | large |
| 7 | SMS ingestion on Android, rules learning from corrections | medium |
| 8 | Receipt scanning | medium |

Two things moved and both moved for the same reason. Deletion and export are no longer part of
a launch-readiness phase at the end: the moment a second person's records are in a database we
host, those are obligations rather than features. And SMS — the feature that made the product
feel magic — is now last of the ingestion work, because it is Android-only and the apps are
paused.

Phases 0 through 4 give a genuinely useful product to anyone with a browser. Phase 6 is where
the work already done in 1a comes back.

## Things that will bite

1. **SMS formats drift.** Patterns live in the database, versioned, and every unmatched
   message is logged so a new rule is a row, not a release.
2. **Duplicates across sources.** First-class feature, not a cleanup script. See above.
3. **Transfers.** Bank → MoMo is neither income nor spending. Get the pairing right early or
   every report is wrong.
4. **Multi-currency reporting.** Decide once: report in GHS by default at transaction-date
   rates, and label any total that crosses currencies.
5. **Sensitive data, and now other people's.** This changed with the server. A hosted
   database holding other people's financial records makes us a data controller in fact:
   encryption in transit and at rest, backups that have actually been restored from, deletion
   and export that work, and registration under Ghana's Data Protection Act. On the device
   side the old answers still hold — the local database is encrypted and the app is behind
   device authentication. No analytics, and no third-party crash reporter that could carry
   transaction text or labels, anywhere.
6. **Other people's banks.** Unbounded format work once published. Rule packs and a redacted
   user-submitted sample flow are the containment; there is no version where you parse every
   Ghanaian bank correctly on day one.
7. **Discipline, not software, is the usual failure.** Manual entry has to be under five
   seconds: amount, category, done, with account and date defaulted.
8. **Two apps drifting.** New with this plan. The containment is that no screen decides
   anything — every number, list and validation comes from the same Rust call on both — plus a
   parity pass at the end of every phase. Drift found later than that is expensive.
9. **The FFI surface growing without limit.** Each screen-shaped query is one more published
   function. Watch the count; past about thirty, the line is in the wrong place.

## Not porting from expensave

Float money. The denormalised balance column plus cron repair. Regex rules compiled with
errors suppressed (`@preg_match`) — a bad pattern should fail loudly at save time. The
amount-plus-same-day duplicate check. And the conflation of account, container and currency
into one "calendar" object, which is precisely what blocks the multi-currency case.

## Settled

- **The server is Rust/Axum, not Dart.** A syncing product has the client and the server both
  computing balances and budget state, and two implementations of those rules eventually
  disagree about someone's money. One `mt-core`, linked into both, removes the class of bug
  rather than testing for it. Phase 0 settles it in practice too: the rules are written, in
  Rust, and there is nothing to port.
- **The UIs are native and the storage layer is not.** See
  [decisions/0001-native-uis.md](decisions/0001-native-uis.md).
- **The web client is the first client**, as a thin SPA against the server rather than a
  local-first WASM build. See [decisions/0002-web-first.md](decisions/0002-web-first.md). A
  browser-local ledger would have been cheaper and is one browser profile on one machine,
  which is not a place to keep someone's records. Desktop is still dropped; a browser covers
  the sit-down case.
- **iPhone is in the picture, from phase 1, alongside Android.** It changes nothing except that
  the SMS story is Android-only and the product has to be good without it.

## Open questions

- Self-hosted server alongside the published one, or only the hosted product? Costs little
  given the server is one binary, and it is a credible answer to "where is my financial data".
- Rule pack signing: a detached signature, or something closer to a transparency log?
- The working name.
