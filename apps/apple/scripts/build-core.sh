#!/usr/bin/env bash
# Builds the Rust core (mq-ffi) as static libraries for Apple targets, generates
# the UniFFI Swift bindings, and packages both into apps/apple/Core/:
#   Core/MarqueeCore.xcframework   static library slices plus the C header
#   Core/marquee_core.swift        generated Swift bindings
# Both are build outputs (not committed); run this before the first Xcode build.
#
# Usage: apps/apple/scripts/build-core.sh [--tvos]
#   --tvos   also build the tvOS device slice (needed to build Marquee-tvOS)
set -euo pipefail

apple="$(cd "$(dirname "$0")/.." && pwd)"
root="$(cd "$apple/../.." && pwd)"
target_dir="${CARGO_TARGET_DIR:-$HOME/.cargo-targets/marquee}"
out="$apple/Core"

targets=(aarch64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim)
if [[ "${1:-}" == "--tvos" ]]; then
  targets+=(aarch64-apple-tvos)
fi

# C code built by build scripts (SQLite, ring) must match the app floors.
export MACOSX_DEPLOYMENT_TARGET=15.0 IPHONEOS_DEPLOYMENT_TARGET=18.0 TVOS_DEPLOYMENT_TARGET=18.0

cd "$root"
for target in "${targets[@]}"; do
  echo "building mq-ffi for $target"
  CARGO_TARGET_DIR="$target_dir" CARGO_INCREMENTAL=0 \
    cargo build -p mq-ffi --lib --release --target "$target" -j 6
done

gen="$(mktemp -d)"
trap 'rm -rf "$gen"' EXIT
CARGO_TARGET_DIR="$target_dir" CARGO_INCREMENTAL=0 \
  cargo run -q -p mq-ffi --features bindgen --bin uniffi-bindgen -- \
  generate --library "$target_dir/aarch64-apple-darwin/release/libmarquee_core.dylib" \
  --language swift --out-dir "$gen"

mkdir -p "$gen/include"
cp "$gen/marquee_coreFFI.h" "$gen/include/"
cp "$gen/marquee_coreFFI.modulemap" "$gen/include/module.modulemap"

args=()
for target in "${targets[@]}"; do
  args+=(-library "$target_dir/$target/release/libmarquee_core.a" -headers "$gen/include")
done
rm -rf "$out/MarqueeCore.xcframework"
mkdir -p "$out"
xcodebuild -create-xcframework "${args[@]}" -output "$out/MarqueeCore.xcframework" >/dev/null
cp "$gen/marquee_core.swift" "$out/marquee_core.swift"
echo "wrote $out/MarqueeCore.xcframework and $out/marquee_core.swift"
