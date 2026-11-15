# 0001 — Native UIs instead of Flutter, and a Rust store beneath them

**Date:** 2026-11-15
**Status:** accepted
**Supersedes:** the UI half of ARCHITECTURE.md §1 as written 2026-11-10

## The decision

Build the iOS app in SwiftUI and the Android app in Jetpack Compose, and move the local
database and the sync client into Rust alongside the rules. Delete the Flutter tree and the
`flutter_rust_bridge` crate; replace the bridge with a UniFFI surface.

Three sub-decisions were taken at the same time: the Rust side owns storage *and* sync rather
than staying pure; desktop and web are dropped from the plan rather than re-platformed; and the
two apps are built in parallel rather than one after the other.

## What changed since 2026-11-10

Nothing about the Rust core, which is why this is affordable. What changed is the case for one
widget tree.

**The design stopped being one design.** `design/ADAPTIVE.md` and `DESIGN.md` specify Material
3 on Android and HIG on iOS down to navigation, sheets, back gesture, icon set, type scale and
motion — with the shared layer reduced to type, palette, density and the stamp. The review
screenshots are already `phone-ios-*` and `phone-android-*` pairs. Flutter's economy is one
tree for both platforms, and we had spent it on paper before writing a screen.

**Web went away.** It was the target Flutter uniquely covered and the one §1 flagged as the
architecture's soft spot — `flutter_rust_bridge`'s WASM support being young. Dropping web and
desktop removes the risk, and removes most of what the single tree was buying.

**The cost of the switch is at its floor.** `app/` is `main.dart`, a wiring check, generated
bindings and one round-trip test. In a month it would be the ledger, the entry sheet, the
inbox and the outlook.

## Why storage moves into Rust

This is the part that is not a UI preference.

Under Flutter, drift gave one implementation of the schema, the migrations, the queries and the
sync client for both platforms, for free. Two native apps give none. Leaving SQLite on the
platform side means writing all of that once in Room and once in GRDB and keeping the two in
agreement by discipline — which is precisely the failure mode the Rust core exists to prevent,
reintroduced one layer down and with someone's balance at the end of it.

So the boundary rule is rewritten. It was *values cross, handles do not*; it is now **one handle
crosses, and it is the store**. The core itself is untouched: still pure, still no I/O, still no
clock, still unaware that a database exists.

The visible loss is drift's reactive queries, which the inbox and the running balance were
going to lean on. The replacement is a change bus in the store: a revision and the set of
tables each write touched, delivered over a UniFFI callback interface, wrapped as an
`AsyncStream` in Swift and a `Flow` in Kotlin. Explicit invalidation rather than inferred, and
one implementation of it rather than two.

## Why UniFFI

It generates the Swift and the Kotlin layer from the same annotated Rust signatures, maps
`Result::Err` onto Swift `throws` and Kotlin exceptions, and turns Rust enums into Swift enums
and Kotlin sealed classes — so an added variant is a compile error on both sides, which is the
property the `flutter_rust_bridge` surface was valued for.

A hand-rolled C ABI with cbindgen and a hand-written JNI layer was the alternative. It buys
control over the boundary and costs a large amount of hand-written `unsafe` on both sides, in
two languages, maintained forever.

Version gets pinned exactly, the way `flutter_rust_bridge = "=2.13.0"` was: the generator and
the runtime must agree about their own wire format.

## What this costs

Every screen is built twice, and two screen-test suites run on two simulators. The containment
is that no screen decides anything: both apps call the same Rust for every number, every list
and every validation, so divergence shows up as a layout difference rather than as two
different answers. A parity pass closes every phase.

The native path also keeps a cost the plan already carried: the SMS receiver is Kotlin either
way — it just loses the MethodChannel hop into Dart.

## What is thrown away

`app/` entirely, and `crates/bridge/` as a crate. Not the work in it: `api.rs`, `dto.rs`,
`enums.rs`, `scalar.rs`, `zone.rs` and `error.rs` — roughly 1,700 hand-written lines, tested —
move to `crates/ffi/` with their conventions intact. Ids, instants and dates still cross as
text in the sync wire's spellings; money still crosses as minor units plus a code; the zone
still crosses as a platform-supplied offset table that refuses questions past its window. What
is deleted is `frb_generated.rs`, which the generator wrote, and the Dart that the other
generator wrote.

## What building it changed

Written the same week it was decided, so the record is of a decision that has been carried
out rather than one still being argued for. Four things the work itself settled.

**The unsafe policy got stricter, not looser.** The expectation was an exception for the FFI
crate, as `flutter_rust_bridge` needed: its glue was a file in the crate, so that crate root
could only `deny` and allow the one module. UniFFI writes its scaffolding from a macro, and
the compiler does not fire lints inside another crate's macro expansion — so the FFI crate
forbids unsafe like every other crate, and the workspace lint is now `forbid` with no
exception anywhere.

**The store's own clock had to be rounded.** The schema keeps instants as whole
milliseconds and a clock hands out more precision than that, so the row returned from a
write carried microseconds the database did not have. It compared unequal against itself a
moment later, and `updated_at` — which is what last-write-wins will resolve conflicts on —
meant one thing in memory and another on disk. Found by a test that asserted the obvious
thing: that a row reads back as it was written.

**Async settled on `spawn_blocking` under a Tokio context.** `async_runtime = "tokio"` wraps
the future the foreign side polls, and the database work runs on the blocking pool inside
it. Swift gets `await`, Kotlin gets `suspend`, and no screen can block its main thread by
forgetting to dispatch.

**Two platform details that only show up when you run it.** The iOS link needs a modern
deployment target or it fails on `___chkstk_darwin`, a symbol that only exists in a libSystem
from this decade. And the app needs a `keychain-access-groups` entitlement before it can
read its own key: without it the first run dies on `errSecMissingEntitlement` long before it
reaches the database. Neither is in any architecture document; both are now in the build
scripts and the project file.

## What would make this wrong

If the two apps drift in behaviour despite the shared store, the parity pass is not working and
the diagnosis is a screen that decided something it should have asked Rust. If the FFI surface
grows past roughly thirty functions, the store is being asked screen-shaped questions it should
be answering generically — or the apps are too thin and should hold some composition of their
own. Neither failure returns us to Flutter; both are adjustments to where the line sits.
