# Personal income & spending tracker — plan

Drafted 2026-11-10, revised the same day after reading `algirdasc/expensave`.
Status: for discussion, nothing built yet.

## What it has to do

Run on phone, desktop and web. Take transactions four ways: typed by hand, parsed from
MoMo/bank SMS, imported from CSV/OFX statements, and scanned off receipts. Answer four
questions: where did the money go, am I over budget, what does next month look like, and
how much is in each account and currency.

## Prior art

`algirdasc/expensave` (GPL-3.0, Symfony + Angular, actively maintained) solves a large part
of this and several of its design decisions are better than my first draft. Everything
marked **[expensave]** below is an idea taken from it and reimplemented in Dart — the code
is GPL-3.0, so nothing is copied verbatim, and the stacks differ anyway. This is firm now that
the plan is to publish: GPL code in a closed-source store app is both a licence breach and a
known conflict with Apple's terms. Ideas are free; files are not.

What it does *not* solve, and why we're still building: no SMS capture (it's a browser app,
a PWA cannot read SMS on Android), no budgets at all, and no currency field anywhere —
amounts are plain floats in one implicit currency.

## Shape

A Rust core and server, a Flutter UI. Every rule and calculation lives in one Rust crate,
compiled into the apps via `flutter_rust_bridge`, into the web client as WASM, and linked
directly into the server — so client and server cannot disagree about a balance. SQLite on each
device as the working store, Postgres behind the server.

Flutter rather than Tauri mobile or React Native for the UI, and Rust rather than Dart for the
rules. The reasoning, the rejected alternatives and the costs are in
[ARCHITECTURE.md](ARCHITECTURE.md); the short version is that the sync product needs one
implementation of the ledger, and the hardest Android work is better travelled in Flutter.

### Layout

- `crates/core/` — pure Rust, no I/O, no clock, no DB. Money, categorisation, balance walk,
  recurrence, dedup, budgets, parsers. Where every rule lives.
- `crates/server/` — Axum, Postgres, auth, sync, rule packs. Links `core` directly.
- `crates/bridge/` — the `flutter_rust_bridge` surface, and nothing else.
- `app/` — Flutter UI for all platforms, drift/SQLite storage, sync client, Kotlin SMS plugin.
- `packs/` — versioned rule packs: SMS patterns and default categories, shipped as data.
- `site/` — static marketing site, not Flutter.

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

### Recurrence editing

When a recurring template changes, the edit asks which instances it applies to:
`this | future | past | all` **[expensave]**. This is the detail that's usually got wrong,
and having it as an explicit prompt at edit time is the right answer.

### Sync

Local SQLite is the working copy, the server is the merge point. Rows carry `updated_at` and
a device id; sync pushes what changed since the last watermark and pulls the rest, last-write-
wins per row. Deletes are soft so a delete on one device isn't resurrected by another. Not a
CRDT — for one person with a few devices, row-level LWW is honest and debuggable.

The web client can't use local SQLite the same way; it talks to the server directly with a
small cache. Accepted asymmetry.

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
or Ecobank CSV later is a new class and nothing else. Planned handlers: MoMo SMS, bank alert
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
background receiver catches MoMo and bank alerts, runs the pattern table, and drops candidates
into the inbox. Never auto-commit a parsed message straight to the ledger; formats change
without notice and a silent mis-parse poisons the numbers quietly.

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
and by-category. Write it once, subclass it three times.

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

**Rule distribution becomes a support problem.** Every bank and wallet another user has is a
format you don't have. Ship `category_rules` and SMS patterns as a versioned pack the app
fetches, so adding a bank is a server-side release rather than a store review. Pair it with an
explicit, user-initiated "this message didn't parse — send a redacted sample" action. Never an
automatic upload of unmatched text; that is exactly the thing the policy forbids.

**Compliance furniture, none of it optional.** Privacy policy URL, Play Data safety form and
App Store privacy labels that match what the app actually does, in-app account deletion and
data export, Ghana's Data Protection Act registration as a data controller, and GDPR duties
the moment a European user signs up. Budget real time for this; it is a phase, not a checkbox.

**The public web surface is a separate site.** Flutter web is fine for the signed-in app and
poor as a landing page — heavy first load, canvas-rendered text, weak SEO. Marketing site in
plain static HTML, app at `app.` on the same domain.

## Phasing

| Phase | Deliverable | Rough size |
|---|---|---|
| 0 | `core`: money, balance walk, categorisation ladder, recurrence, budgets, dedup — tested | small |
| 1 | Android app: accounts, manual entry, pending/confirmed, recurring, outlook, breakdown | medium |
| 2 | SMS ingestion, review inbox, rules learning from corrections | medium |
| 3 | Budgets and warnings; reconciliation entries | small |
| 4 | CSV + OFX import, handler resolver, duplicate merging | medium |
| 5 | Sync server, then desktop and web builds off the same tree | large |
| 6 | Receipt scanning | medium |
| 7 | Launch readiness: multi-tenant hardening, rule packs, deletion/export, store declarations, marketing site | large |

Forecasting no longer has a phase of its own — adopting the pending-transaction model folds it
into phase 1 for roughly nothing. Phases 0–3 give a genuinely useful app; everything after is
widening, not deepening. If this stalls it stalls at phase 5, which is why that sits after the
app already earns its keep standalone.

## Things that will bite

1. **SMS formats drift.** Patterns live in the database, versioned, and every unmatched
   message is logged so a new rule is a row, not a release.
2. **Duplicates across sources.** First-class feature, not a cleanup script. See above.
3. **Transfers.** Bank → MoMo is neither income nor spending. Get the pairing right early or
   every report is wrong.
4. **Multi-currency reporting.** Decide once: report in GHS by default at transaction-date
   rates, and label any total that crosses currencies.
5. **Sensitive data.** Encrypt the local database, device auth to open the app, and if a
   server stores anything it's your own instance. No analytics, no third-party crash reporter
   carrying transaction text.
6. **Other people's banks.** Unbounded format work once published. Rule packs and a redacted
   user-submitted sample flow are the containment; there is no version where you parse every
   Ghanaian bank correctly on day one.
7. **Discipline, not software, is the usual failure.** Manual entry has to be under five
   seconds: amount, category, done, with account and date defaulted.

## Not porting from expensave

Float money. The denormalised balance column plus cron repair. Regex rules compiled with
errors suppressed (`@preg_match`) — a bad pattern should fail loudly at save time. The
amount-plus-same-day duplicate check. And the conflation of account, container and currency
into one "calendar" object, which is precisely what blocks the multi-currency case.

## Open questions

- Server in Dart (one language everywhere) or Rust/Axum (reuses the credvault muscle)?
  Leaning Dart to keep the context switch low.
- Is the web client needed, or does desktop cover the sit-down case? Dropping it removes the
  storage asymmetry and a chunk of phase 5.
- Self-hosted server, or local-only with sync over the LAN? Local-only is a real option and
  removes most of the security surface.
- iPhone in the picture, or Android only? Changes nothing except the SMS story.
