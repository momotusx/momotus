#!/bin/bash
# Builds the Rust core for Android arm64 and generates the Kotlin bindings.
#
# One cargo build, in this app's own target dir (cargo gives workspace crates the same artifact
# names in every checkout, so a shared target dir mixes sources across worktrees):
#   1. cargo-ndk builds libmarquee_core.so with symbols kept (CARGO_PROFILE_RELEASE_STRIP=none).
#   2. uniffi-bindgen reads the UniFFI metadata from those symbols (library mode needs them; the
#      release profile strips them) and writes the Kotlin bindings.
#   3. llvm-strip makes the shipped .so, which is what the size budget counts.
# This needs no host build of the core. The bindgen CLI is a tiny standalone crate pinned to the
# UniFFI version of the core, installed once:
#   cargo install --path apps/android/tools/uniffi-bindgen --locked --target-dir <scratch>
#
# Usage: build-core.sh [armv7]   (armv7 adds the 32-bit ABI, size measurement only, no emulator run)
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
TARGET=${CARGO_TARGET_DIR:-$HOME/.cargo-targets/marquee-android}
NDK=${ANDROID_NDK_HOME:-/opt/homebrew/share/android-commandlinetools/ndk/27.1.12297006}
STRIP=$NDK/toolchains/llvm/prebuilt/darwin-x86_64/bin/llvm-strip
OUT=$ROOT/apps/android/core/src/main
ABIS=(-t arm64-v8a)
TRIPLES=(aarch64-linux-android:arm64-v8a)
if [ "${1:-}" = "armv7" ]; then
  ABIS+=(-t armeabi-v7a)
  TRIPLES+=(armv7-linux-androideabi:armeabi-v7a)
fi

cd "$ROOT"

CARGO_TARGET_DIR=$TARGET CARGO_INCREMENTAL=0 CARGO_PROFILE_RELEASE_STRIP=none ANDROID_NDK_HOME=$NDK \
  cargo ndk "${ABIS[@]}" --platform 24 build --release -p mq-ffi -j 4

UNSTRIPPED=$TARGET/aarch64-linux-android/release/libmarquee_core.so
rm -rf "$OUT/generated"
uniffi-bindgen generate --library "$UNSTRIPPED" --language kotlin \
  --config "$ROOT/apps/android/uniffi.toml" --out-dir "$OUT/generated"

rm -rf "$OUT/jniLibs"
for pair in "${TRIPLES[@]}"; do
  triple=${pair%%:*}
  abi=${pair##*:}
  mkdir -p "$OUT/jniLibs/$abi"
  "$STRIP" --strip-all -o "$OUT/jniLibs/$abi/libmarquee_core.so" "$TARGET/$triple/release/libmarquee_core.so"
done
ls -l "$OUT"/jniLibs/*/libmarquee_core.so
