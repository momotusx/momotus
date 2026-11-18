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

# bindgen (rquickjs-sys on 32-bit ARM Linux) needs zig's C headers. cargo-zigbuild hands it the ones `zig cc -v` lists,
# but zig 0.15 lists them relative to the working directory when zig sits inside it (.tools on CI), and the build
# script runs elsewhere: "stdbool.h not found". So zig is asked from / and its dirs go first, as absolute paths.
probe="$(mktemp -d)"
: >"$probe/empty.c"
inc="$(cd / && zig cc -target arm-linux-gnueabi -E "$probe/empty.c" -v 2>&1 |
  sed -n '/^#include <...> search starts here:$/,/^End of search list\.$/p' | sed -n 's|^ \(/.*\)$|\1|p')"
rm -rf "${probe:?}"
[ -n "$inc" ] || { echo "build-native.sh: zig cc listed no include dirs for arm-linux-gnueabi" >&2; exit 1; }
bindgen_args=""
while IFS= read -r dir; do bindgen_args="$bindgen_args -isystem '$dir'"; done <<<"$inc"
bindgen_args="${bindgen_args# }"

cd "$root"
# cargo-zigbuild adds its own to both spellings of the variable; bindgen reads the dashed one first.
env "BINDGEN_EXTRA_CLANG_ARGS_${target}=$bindgen_args" "BINDGEN_EXTRA_CLANG_ARGS_${target//-/_}=$bindgen_args" \
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
