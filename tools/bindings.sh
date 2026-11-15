#!/usr/bin/env bash
# Writes the Swift and the Kotlin from the library itself.
#
# The generator is this workspace's own binary rather than an installed tool, so the version
# that writes the bindings is the version compiled into the library beside them. Those two
# disagreeing about their own wire format is a bug that surfaces as a crash on a handset,
# long after the build that caused it.
#
# Nothing it writes is ever edited by hand: run this instead.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
swift_out="$root/ios/MoneytrackKit/Sources/MoneytrackKit/Generated"
kotlin_out="$root/android/core/src/main/kotlin"

# The generator reads the metadata out of a built library, so build the host one first.
(cd "$root" && cargo build -p mt_ffi)
library="$root/target/debug/libmt_ffi.dylib"

generate() {
    (cd "$root" && cargo run -q -p mt_ffi --bin uniffi-bindgen -- \
        generate --library "$library" --language "$1" --out-dir "$2" --no-format)
}

if [ "${1:-}" = "--headers-only" ]; then
    # The C header and the module map, for the XCFramework's Headers directory.
    scratch="$(mktemp -d)"
    generate swift "$scratch"
    mkdir -p "$2"
    cp "$scratch"/*.h "$2/"
    cp "$scratch"/*.modulemap "$2/module.modulemap"
    rm -rf "$scratch"
    exit 0
fi

mkdir -p "$swift_out" "$kotlin_out"
scratch="$(mktemp -d)"
generate swift "$scratch"
cp "$scratch"/mt_ffi.swift "$swift_out/"
rm -rf "$scratch"

generate kotlin "$kotlin_out"

echo "wrote $swift_out/mt_ffi.swift"
echo "wrote $kotlin_out/uniffi/mt_ffi/mt_ffi.kt"
