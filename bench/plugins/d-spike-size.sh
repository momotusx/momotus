#!/bin/bash
# Phase D size probe (plans/plugins-and-marketplace.md 6.1 and 9): builds the probe and its baseline
# (crates/mq-plugins/examples) for one target, strips both, and prints one JSON line with their sizes,
# the difference (what the engine and the parser add), the QuickJS marker strings found in each, and the
# QuickJS symbols in the unstripped probe. Deletes the target's build output afterwards.
#
# Usage: d-spike-size.sh <android-arm64|android-armv7|ios|macos|tvos>
# Run it through the build lock, with CARGO_TARGET_DIR set to this checkout's own target dir.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
T=${CARGO_TARGET_DIR:?set CARGO_TARGET_DIR to this checkout\'s target dir}
NDK=${ANDROID_NDK_HOME:-/opt/homebrew/share/android-commandlinetools/ndk/27.1.12297006}
LLVM=$NDK/toolchains/llvm/prebuilt/darwin-x86_64/bin
# The hardening 6.1 asks of the C engine.
HARDEN="-fstack-protector-strong -D_FORTIFY_SOURCE=2"
# QuickJS's own error messages (6.1 item 2), from code every call reaches.
MARKERS=("invalid opcode: pc=%u opcode=0x%02x" "bytecode function expected" "out of memory in regexp execution")
SYMBOLS=(JS_NewRuntime JS_Eval)
export CARGO_INCREMENTAL=0 CARGO_PROFILE_RELEASE_STRIP=none
cd "$ROOT"

case "${1:-}" in
  android-arm64 | android-armv7)
    if [ "$1" = android-arm64 ]; then abi=arm64-v8a triple=aarch64-linux-android; else abi=armeabi-v7a triple=armv7-linux-androideabi; fi
    # The core's own link flag (apps/android/scripts/build-core.sh).
    packed="-C link-arg=-Wl,--pack-dyn-relocs=android"
    env "CFLAGS_${triple//-/_}=$HARDEN" \
      CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="$packed" CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_RUSTFLAGS="$packed" \
      ANDROID_NDK_HOME=$NDK cargo ndk -t $abi --platform 24 build --release -p mq-plugins -j 4 --locked \
      --example probe --example probe_base >&2
    ext=so strip=("$LLVM/llvm-strip" --strip-all) nm=("$LLVM/llvm-nm")
    ;;
  ios | macos | tvos)
    case "$1" in
      ios) triple=aarch64-apple-ios sdk=iphoneos ;;
      macos) triple=aarch64-apple-darwin sdk=macosx ;;
      tvos) triple=aarch64-apple-tvos sdk=appletvos ;;
    esac
    # The app floors (apps/apple/scripts/build-core.sh); bindgen reads the SDK's headers.
    env MACOSX_DEPLOYMENT_TARGET=15.0 IPHONEOS_DEPLOYMENT_TARGET=18.0 TVOS_DEPLOYMENT_TARGET=18.0 \
      "CFLAGS_${triple//-/_}=$HARDEN" \
      "BINDGEN_EXTRA_CLANG_ARGS_${triple//-/_}=-isysroot $(xcrun --sdk $sdk --show-sdk-path)" \
      cargo build --release --target $triple -p mq-plugins -j 4 --locked --example probe --example probe_base >&2
    ext=dylib strip=(xcrun strip -x) nm=(xcrun nm)
    ;;
  *)
    echo "usage: d-spike-size.sh <android-arm64|android-armv7|ios|macos|tvos>" >&2
    exit 2
    ;;
esac

out=$T/$triple/release/examples
tmp=$(mktemp -d)
for lib in probe probe_base; do
  if [ "$ext" = so ]; then
    "${strip[@]}" -o "$tmp/$lib.$ext" "$out/lib$lib.$ext"
  else
    cp "$out/lib$lib.$ext" "$tmp/$lib.$ext"
    "${strip[@]}" "$tmp/$lib.$ext"
  fi
done
size() { stat -f %z "$1"; }
found() {
  local m list=()
  for m in "${MARKERS[@]}"; do
    if grep -aqF "$m" "$1"; then list+=("\"$m\""); fi
  done
  (IFS=,; echo "[${list[*]}]")
}
syms() {
  local s list=()
  for s in "${SYMBOLS[@]}"; do
    if "${nm[@]}" "$1" 2>/dev/null | grep -qE "[ _]$s\$"; then list+=("\"$s\""); fi
  done
  (IFS=,; echo "[${list[*]}]")
}
probe=$(size "$tmp/probe.$ext")
base=$(size "$tmp/probe_base.$ext")
printf '{"date":"%s","target":"%s","triple":"%s","trunk":"%s","change":"feat(plugins): the engine spike, unmerged","probe_stripped_bytes":%s,"base_stripped_bytes":%s,"added_bytes":%s,"added_mib":%s,"markers_in_stripped_probe":%s,"markers_in_stripped_base":%s,"symbols_in_unstripped_probe":%s,"symbols_in_unstripped_base":%s}\n' \
  "$(date +%F)" "$1" "$triple" "$(git merge-base HEAD feat/protocol-wire | cut -c1-9)" "$probe" "$base" $((probe - base)) \
  "$(echo "scale=3; ($probe - $base) / 1048576" | bc)" \
  "$(found "$tmp/probe.$ext")" "$(found "$tmp/probe_base.$ext")" \
  "$(syms "$out/libprobe.$ext")" "$(syms "$out/libprobe_base.$ext")"
rm -rf "${tmp:?}" "${T:?}/${triple:?}"
