# moneytrack (working name)

Personal income and spending tracker: a web app first, with Android and iPhone apps that carry
their own copy of the ledger offline and capture MoMo and bank SMS on Android. Intended for
publication, not just personal use.

The browser comes first because it is the only channel that is open to us right now — a web
app is a URL, where a store listing is a review. The mobile apps are paused at phase 1a, which
they reached complete; they resume against the same server.

The thinking so far:

- [docs/PLAN.md](docs/PLAN.md) — what it does, in what order, and what will bite.
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — how it is put together and why.
- [docs/decisions/](docs/decisions/) — the decisions that reversed something, argued on their
  own terms.
- [docs/SCHEMA.md](docs/SCHEMA.md) — the data model, and how the Rust types and the two
  database schemas are kept in agreement.
- [docs/SCHEMA-DIAGRAM.md](docs/SCHEMA-DIAGRAM.md) — the same thing as three diagrams.

Built so far:

- `crates/core` — phase 0 complete: money, ids, enums and the row types, plus the five rules
  that turn rows into answers — the balance walk, the categorisation ladder, recurrence
  expansion, duplicate scoring and budget evaluation.
- `crates/store` — the device database: the SQLite schema in `db/` is what it runs, encrypted
  with SQLCipher, with the repositories, the balance queries and the change bus the screens
  re-query on.
- `crates/ffi` — the published surface, over UniFFI. One handle crosses, the store; everything
  else is values.
- `ios/` and `android/` — an app on each platform that opens a real encrypted ledger and reads
  a balance back out of Rust. Paused here while the web client is built.

- `crates/server` — the API: Axum over Postgres, with `mt-core` linked in. Sign-up, sign-in,
  sessions in `HttpOnly` cookies, CSRF, and the accounts, transactions and balance endpoints.
  Every query is scoped to a user, and the tests include one person trying to read and write
  another's rows.

- `web/` — the browser client in TypeScript and React. Sign-up and sign-in, opening an
  account, recording a row, and the ledger: oldest first, through a ruled TODAY line into the
  rows still expected, with what the account held after each. It computes no number — the
  running balance is `mt-core`'s walk and a typed amount is read by `mt-core` on the server.

Categories are in: a new ledger starts with eighteen, rows are filed under one from the
entry form or refiled from the ledger, and a category can hold others one level down.

"Where it went" shows a month's spending and income by category, one currency at a time,
with transfers left out and each share worked out in `mt-core`.

Being built next: the rest of phase 3 — the outlook and budgets.

`cargo test --workspace` covers the Rust, `db/tests/` covers the database constraints, and
each app has a round trip that runs on the laptop rather than on a device.

## Building it

Nothing generated or built is in source control, so a clean checkout runs three scripts:

```
tools/bindings.sh         # writes the Swift and the Kotlin from the library itself
tools/ffi-build.sh        # the Apple slices, assembled into an XCFramework
tools/android-libs.sh     # a .so per ABI
```

Then `cargo test --workspace`; `swift test` in `ios/MoneytrackKit`, or open
`ios/Moneytrack.xcodeproj`; `./gradlew test` in `android`, or open that folder in Android
Studio. Android needs `android/local.properties` pointing at the SDK.

The server and its tests need a Postgres:

```
tools/pg.sh                     # a container on 55432; prints the connection string
cp .env.example .env            # and fill it in
cargo run -p mt-server          # migrates on start, then listens
```

The tests create a database of their own per test and drop it afterwards, so `cargo test`
needs the container running and nothing else.

The browser client, with the server above running on 8080:

```
cd web
npm ci
npm run dev        # Vite on 5173, forwarding /api to the server
npm test           # component tests; no server needed
npm run build      # static files in web/dist
```

In production the static files and `/api` sit behind one host, as Vite arranges in
development. The session cookie is `SameSite=Lax` and scoped to `/api`, and the client is
built to be same-origin with it: there is no CORS configuration, and there should never need
to be. `MT_API` points the dev server at an API somewhere other than 127.0.0.1:8080.

Decided: a Rust core, store and sync layer; native UIs, SwiftUI and Jetpack Compose; integer
minor units for money; on-device SMS parsing; and pending transactions as the forecast.
