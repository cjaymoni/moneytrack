# moneytrack (working name)

Personal income and spending tracker for Android and iPhone, with automatic capture from MoMo
and bank SMS on Android. Intended for publication, not just personal use.

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
  a balance back out of Rust. The screens are next.

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

Decided: a Rust core, store and sync layer; native UIs, SwiftUI and Jetpack Compose; integer
minor units for money; on-device SMS parsing; and pending transactions as the forecast.
