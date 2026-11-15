#!/usr/bin/env bash
# Builds the Rust library for the Apple targets and assembles the XCFramework the iOS app
# and the Swift package both link.
#
# Three slices, and the third is the reason the package is testable at all: a macOS slice
# lets `swift test` run the round trip on a laptop in a second, rather than only inside a
# simulator.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
profile="${1:-debug}"
flag=""
[ "$profile" = "release" ] && flag="--release"

# Both deployment targets are set explicitly, and the iOS one has to be: Rust defaults the
# target to iOS 10, OpenSSL and SQLCipher compile against the installed SDK, and the link
# then fails on `___chkstk_darwin`, which only exists in a libSystem from this decade. These
# two numbers are the floor the app targets as well; they belong together.
export IPHONEOS_DEPLOYMENT_TARGET=17.0
export MACOSX_DEPLOYMENT_TARGET=14.0

targets=(aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-darwin)
for target in "${targets[@]}"; do
    echo "building $target ($profile)"
    (cd "$root" && cargo build -p mt_ffi --target "$target" $flag)
done

out="$root/ios/MoneytrackKit/Frameworks"
framework="$out/MoneytrackFFI.xcframework"
rm -rf "$framework"
mkdir -p "$out"

# One headers directory per slice: the C header UniFFI emits, plus a module map naming it
# `mt_ffiFFI`, which is the module the generated Swift imports.
headers="$(mktemp -d)"
"$root/tools/bindings.sh" --headers-only "$headers"

xcodebuild -create-xcframework \
    -library "$root/target/aarch64-apple-ios/$profile/libmt_ffi.a" -headers "$headers" \
    -library "$root/target/aarch64-apple-ios-sim/$profile/libmt_ffi.a" -headers "$headers" \
    -library "$root/target/aarch64-apple-darwin/$profile/libmt_ffi.a" -headers "$headers" \
    -output "$framework"

rm -rf "$headers"
echo "wrote $framework"
