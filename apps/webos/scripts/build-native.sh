#!/usr/bin/env bash
# Builds the native core for the TV (plans/webos.md v5): mq-bridge with the torrent engine and Nuvio plugins, for the
# 32-bit ARM userland every webOS 6 to 26 TV runs (softfp, glibc), linked against glibc 2.17 symbols at most so any
# TV's own glibc loads it and its resolver answers DNS as the TV's Node does. Built with cargo-zigbuild and zig
# (prebuilt; fetch-tools.sh on Linux, Homebrew on macOS). Writes apps/webos/service/bin/mq-bridge, which package.sh
# ships; prints the commit, size and SHA-256.
#   CARGO_TARGET_DIR  where cargo builds (default ~/.cargo-targets/marquee-<checkout>-armv7); its own, never shared
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
root="$(cd "$here/../.." && pwd)"
target=armv7-unknown-linux-gnueabi
glibc=2.17
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cargo-targets/marquee-$(basename "$root")-armv7}"
out="$here/service/bin"

cd "$root"
CARGO_INCREMENTAL=0 cargo zigbuild --release -j 4 -p mq-bridge --bin mq-bridge --features torrent --target "$target.$glibc"
mkdir -p "$out"
cp "$CARGO_TARGET_DIR/$target/release/mq-bridge" "$out/mq-bridge"
chmod 755 "$out/mq-bridge"

sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1
}
echo "core $(git rev-parse --short HEAD)$(git diff --quiet HEAD -- crates Cargo.lock || echo ' (uncommitted changes)')"
echo "mq-bridge $(wc -c <"$out/mq-bridge" | tr -d ' ') bytes, sha256 $(sha256 "$out/mq-bridge")"
file "$out/mq-bridge"
