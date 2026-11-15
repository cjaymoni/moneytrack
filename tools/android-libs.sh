#!/usr/bin/env bash
# Builds the Rust library for each Android ABI and drops the result where Gradle packages
# it from. Writes into `android/core/src/main/jniLibs`, which is a build artifact and is
# not in source control.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
profile="${1:-debug}"
flag=""
[ "$profile" = "release" ] && flag="--release"

: "${ANDROID_NDK_HOME:=$HOME/Library/Android/sdk/ndk/$(ls "$HOME/Library/Android/sdk/ndk" | sort -V | tail -1)}"
export ANDROID_NDK_HOME
export ANDROID_NDK_ROOT="$ANDROID_NDK_HOME"

# arm64 is every handset worth shipping to; x86_64 is the emulator, and leaving it out makes
# the app untestable on a laptop. 32-bit ABIs are not built: Play has required a 64-bit
# build since 2019 and nothing in the target market is still 32-bit only.
cd "$root"
cargo ndk \
    --target arm64-v8a \
    --target x86_64 \
    --platform 26 \
    --output-dir "$root/android/core/src/main/jniLibs" \
    -- build -p mt_ffi $flag

# And the desktop library the JVM unit tests load through JNA. It has to match the
# architecture of the JDK Gradle runs on, which on a Mac is not always the architecture of
# the Mac: an x86_64 JDK under Rosetta needs an x86_64 library, and dlopen says so in a
# sentence nobody reads twice.
arch="$(java -XshowSettings:properties -version 2>&1 | sed -n 's/.*os\.arch = //p')"
case "$(uname -s)-$arch" in
    Darwin-aarch64) host=aarch64-apple-darwin ;;
    Darwin-x86_64)  host=x86_64-apple-darwin ;;
    Linux-aarch64)  host=aarch64-unknown-linux-gnu ;;
    Linux-x86_64|Linux-amd64) host=x86_64-unknown-linux-gnu ;;
    *) host="" ;;
esac
if [ -n "$host" ]; then
    echo "building $host for the JVM tests"
    cargo build -p mt_ffi --target "$host" $flag
fi

echo "wrote $root/android/core/src/main/jniLibs"
