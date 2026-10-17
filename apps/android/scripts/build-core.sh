#!/bin/bash
# Builds the Rust core for Android arm64 and generates the Kotlin bindings.
#
# The bindings come from a host debug build: UniFFI library mode reads the
# static symbol table, which the release profile strips (strip = true). The
# metadata is identical on every target, so the Android .so matches.
#
# Usage: build-core.sh [all|so|bindings] [armv7]
#   so        only the Android .so
#   bindings  only the Kotlin bindings (host debug build plus bindgen)
#   armv7     adds the 32-bit ABI (size measurement only, no emulator run)
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
TARGET=${CARGO_TARGET_DIR:-$HOME/.cargo-targets/marquee}
NDK=${ANDROID_NDK_HOME:-/opt/homebrew/share/android-commandlinetools/ndk/27.1.12297006}
OUT=$ROOT/apps/android/core/src/main
STEP=${1:-all}
ABIS=(-t arm64-v8a)
if [ "${2:-}" = "armv7" ]; then ABIS+=(-t armeabi-v7a); fi

cd "$ROOT"

if [ "$STEP" = "all" ] || [ "$STEP" = "so" ]; then
  CARGO_TARGET_DIR=$TARGET CARGO_INCREMENTAL=0 ANDROID_NDK_HOME=$NDK \
    cargo ndk "${ABIS[@]}" --platform 24 -o "$OUT/jniLibs" build --release -p mq-ffi -j 4
  ls -l "$OUT"/jniLibs/*/libmarquee_core.so
fi

if [ "$STEP" = "all" ] || [ "$STEP" = "bindings" ]; then
  CARGO_TARGET_DIR=$TARGET CARGO_INCREMENTAL=0 cargo build -p mq-ffi -j 4
  rm -rf "$OUT/generated"
  CARGO_TARGET_DIR=$TARGET CARGO_INCREMENTAL=0 \
    cargo run -p mq-ffi --features bindgen --bin uniffi-bindgen -j 4 -- generate \
    --library "$TARGET/debug/libmarquee_core.dylib" --language kotlin \
    --config "$ROOT/apps/android/uniffi.toml" --out-dir "$OUT/generated"
fi
