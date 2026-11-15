# 0002 — The web client first, and a server under it

**Date:** 2026-11-15
**Status:** accepted
**Supersedes:** the "web and desktop are dropped" decision in
[0001-native-uis.md](0001-native-uis.md) and the phasing in PLAN.md that put the server at
phase 5

## The decision

Build the product as a web application: a Rust server — Axum, Postgres, `mt-core` linked in —
with a TypeScript and React client in the browser. The browser holds no ledger data of its own
and runs no rules; it asks the server, which runs the same `mt-core` the apps run.

The two mobile apps are paused where phase 1a left them: they open an encrypted local database
and read a balance back out of Rust, and nothing about that work is thrown away.

## Why

**The stores are a gate we cannot open yet.** An Android build needs a Play listing, a review,
and — for the feature that makes it feel magic — a restricted-permissions declaration that is
granted case by case. iOS needs a developer account and a review of its own. None of that is
available right now, which means the mobile apps, however finished, reach nobody. A web app is
a URL. It ships today and it ships to everyone, including on a phone.

**The product does not need SMS to be worth using.** PLAN.md already said this, and said it as
a requirement rather than a hope: iOS can never have SMS capture and an Android grant can be
withdrawn, so manual entry, import and the analysis have to carry the product. A web client is
that claim taken seriously. If the thing is not useful without SMS, better to find out now.

**The server was always going to exist.** It was phase 5 — sync, rule packs, deletion and
export — so this is a reordering, not an addition. Everything built for the web client is
load-bearing for mobile sync later: the schema, the tenancy, the auth, the deployment.

## Why the browser is thin

The obvious alternative, and the one considered first, was local-first: compile `mt-core` and
`mt-store` to WASM, keep SQLite in the browser through OPFS, and have no server at all. It is
genuinely available — `rusqlite` targets `wasm32-unknown-unknown` through `sqlite-wasm-rs`,
SQLCipher included — and it would have been cheaper.

Rejected because of where the data would live. A browser-local ledger is one browser profile
on one machine: no second device, no phone and laptop together, and "clear site data" is
indistinguishable from "delete everything I have ever recorded". A finance app whose records a
stray browser setting can erase is not a finance app.

Thin, then: the browser renders what the server computes. Which also removes the question the
old architecture had to keep open — whether WASM could be trusted for the web target — by not
asking it.

## What it costs, and this one is not technical

The privacy story changes shape, and the change is real rather than cosmetic.

The old promise was that the numbers never leave the handset. A hosted service means a
database holding other people's financial records, which makes us a data controller in fact
and not just on a form. Registration under Ghana's Data Protection Act, encryption at rest and
in transit, backups that have actually been restored from, deletion and export as working
features rather than scripts, and a privacy policy that describes what is true — all of that
moves from "phase 7, launch readiness" to "before the second person signs up".

The SMS commitment survives intact and gets easier to keep, because it has not changed: parsing
stays on the device, raw message text never reaches the server, and only a confirmed
transaction is ever sent. There is nowhere in `db/postgres/0001_init.sql` to put a message
body, deliberately.

## What this is built on

Nothing new has to be designed to start. `db/postgres/0001_init.sql` is written: every
per-person table is keyed on `(user_id, id)`, every foreign key carries `user_id` through so a
transaction physically cannot reference another user's account, money is `BIGINT` minor units,
and no balance is stored anywhere. `users`, `devices` — whose platform list already includes
`web` — and rotating refresh tokens with reuse detection are all there.

The rules are written and tested in `mt-core`, and the server links them directly. So what is
actually new is the Axum layer, the Postgres repositories, the auth, and the client.

## What would make this wrong

If the web client ends up re-implementing rules in TypeScript — formatting money by dividing
by a hundred, deciding what counts as spending, filtering transfers — the thin-client
discipline has failed and the product has two implementations of its own arithmetic again. The
test is whether a number on a screen can be traced to one `mt-core` call.

And if the server's per-screen endpoints drift from what `crates/store` answers on the device,
mobile sync will land on a server that disagrees with the handsets. The two will have separate
SQL, which is unavoidable and was always the plan; what they may not have is separate rules.
