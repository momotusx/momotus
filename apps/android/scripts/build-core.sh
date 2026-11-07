#!/bin/bash
# Builds the Rust core for Android (arm64-v8a and armeabi-v7a) and generates the Kotlin bindings.
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
# Usage: build-core.sh [arm64|armv7|dev [armv7]]
#   Both ABIs by default: phones are arm64, and TV sticks such as the Fire TV Stick 4K run a 32-bit
#   userland (plans/m6-android.md D10). arm64 or armv7 builds one, for a walk on one device; a release
#   or bundle build then refuses until both are there (core/build.gradle.kts).
#   dev turns debug assertions on, which compiles the bridge's dev hooks in (Trakt from a local fake on
#   this machine, `MQ_DEV_TRAKT`, for walks), in its own target dir, into src/debug/jniLibs: only debug
#   builds of the app package it. It builds arm64, or armv7 when named. The plain build has no debug
#   assertions and so no hook.
# The torrent engine is in every own build's core, arm64-v8a and armeabi-v7a (plans/play-every-version.md, Steps 0
# and 0b). Its size is measured against its own
# budget (plans/core-backlog.md C11). STORE=1 builds the store core without it, on purpose; MQ_FEATURES names
# the features outright. The build fails when a library is not the core it should be, and it tells the
# app which it is in core-build.properties (BuildConfig.CORE_BUILD, and CORE_ENGINE_ABIS for the ABIs whose core
# carries the engine). Rules: scripts/core-engine.sh.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
# shellcheck source=apps/android/scripts/core-engine.sh
source "$ROOT/apps/android/scripts/core-engine.sh"
if [ "$(core_kind)" = store ]; then echo "build-core.sh: store core (STORE=1): no torrent engine"; fi
# The Trakt app (client id and redirect URI, read by `option_env!`) when this machine has it:
# .env.trakt.local at the repository root, gitignored. Without the file the core has no Trakt app
# (notConfigured), as in CI or a fresh clone. The TMDB read token (MARQUEE_TMDB_READ_TOKEN) the same
# way, from .env.tmdb.local: the title page's TMDB score.
for env in "$ROOT/.env.trakt.local" "$ROOT/.env.tmdb.local"; do
  if [ -f "$env" ]; then
    set -a
    source "$env"
    set +a
  fi
done
TARGET=${CARGO_TARGET_DIR:-$HOME/.cargo-targets/marquee-android}
NDK=${ANDROID_NDK_HOME:-/opt/homebrew/share/android-commandlinetools/ndk/27.1.12297006}
STRIP=$NDK/toolchains/llvm/prebuilt/darwin-x86_64/bin/llvm-strip
# The engine check reads the libraries' dynamic symbols (scripts/core-engine.sh).
NM=$NDK/toolchains/llvm/prebuilt/darwin-x86_64/bin/llvm-nm
OUT=$ROOT/apps/android/core/src/main
TRIPLES=(aarch64-linux-android:arm64-v8a armv7-linux-androideabi:armeabi-v7a)
DEV_ASSERTIONS=false
LIBS=$OUT/jniLibs
usage() {
  echo "usage: build-core.sh [arm64|armv7|dev [armv7]]" >&2
  exit 2
}
# One ABI only, for a walk on one device.
one() {
  case "$1" in
    arm64)
      TRIPLES=(aarch64-linux-android:arm64-v8a)
      ;;
    armv7)
      TRIPLES=(armv7-linux-androideabi:armeabi-v7a)
      ;;
    *) usage ;;
  esac
}
case "${1:-}" in
  "") ;;
  arm64 | armv7) one "$1" ;;
  dev)
    DEV_ASSERTIONS=true
    TARGET=$TARGET-dev
    LIBS=$ROOT/apps/android/core/src/debug/jniLibs
    one "${2:-arm64}"
    ;;
  *) usage ;;
esac
# A release core carries TMDB (plans/title-page-ratings-genres-similar.md), the store core (STORE=1)
# too: it never ships without the token by accident. Only a dev core may lack it.
if [ "$DEV_ASSERTIONS" = false ] && [ -z "${MARQUEE_TMDB_READ_TOKEN:-}" ]; then
  echo "build-core.sh: no TMDB token: copy .env.tmdb.local to the repository root" >&2
  exit 1
fi

cd "$ROOT"

# Android's packed relocations (APS2, read by the loader since API 23; minSdk is 24): the relocation
# table drops from 538,776 to 72,626 bytes, which keeps the library inside its 12 MB budget.
PACKED="-C link-arg=-Wl,--pack-dyn-relocs=android"
# Panic locations and debug paths name ~ instead of this Mac's home folder.
PACKED="$PACKED --remap-path-prefix=$HOME=~"
# The same for C file names (assert and __FILE__) in the C parts build scripts compile (rquickjs, SQLite, ring).
export CFLAGS="${CFLAGS:+$CFLAGS }-ffile-prefix-map=$HOME=~"
# `rustc --crate-type cdylib`: with mq-ffi's three crate types (lib, staticlib, cdylib) cargo passes
# no `-C lto`, so the profile's thin LTO never applied; the cdylib alone gets it. At 39e3b7ae the
# store library went from 12,123,488 to 10,989,256 bytes and the full one from 15,362,864 to
# 13,977,216 (bench/torrent/size.jsonl).
# One cargo-ndk call per ABI: each ABI gets its own features (the engine on arm64-v8a only).
for pair in "${TRIPLES[@]}"; do
  features=$(core_features "${pair##*:}")
  CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS=$PACKED CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_RUSTFLAGS=$PACKED \
  CARGO_TARGET_DIR=$TARGET CARGO_INCREMENTAL=0 CARGO_PROFILE_RELEASE_STRIP=none ANDROID_NDK_HOME=$NDK \
    CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=$DEV_ASSERTIONS \
    cargo ndk -t "${pair##*:}" --platform 24 rustc --release -p mq-ffi -j 4 --crate-type cdylib \
    ${features:+--features "$features"}
done

# The bindings come from the first ABI's library. Each further ABI's are generated too and must be the
# same, so a `cfg(target_pointer_width)` that ever reached the FFI surface fails here, not on a device.
bindings() {
  uniffi-bindgen generate --library "$TARGET/$1/release/libmarquee_core.so" --language kotlin \
    --config "$ROOT/apps/android/uniffi.toml" --out-dir "$2"
}
rm -rf "$OUT/generated"
bindings "${TRIPLES[0]%%:*}" "$OUT/generated"
for pair in "${TRIPLES[@]:1}"; do
  other=$(mktemp -d)
  bindings "${pair%%:*}" "$other"
  if ! diff -r -q "$OUT/generated" "$other" >/dev/null; then
    echo "build-core.sh: the ${pair##*:} bindings differ from ${TRIPLES[0]##*:}'s" >&2
    rm -rf "$other"
    exit 1
  fi
  rm -rf "$other"
done

# A shipped core carries no dev hook: the MQ_DEV_ variables and the e2e recorder's set_dev_replay
# are compiled in only with debug assertions (dev). Checked on each library before it is stripped.
if [ "$DEV_ASSERTIONS" = false ]; then
  for pair in "${TRIPLES[@]}"; do
    lib=$TARGET/${pair%%:*}/release/libmarquee_core.so
    if grep -aqE 'MQ_DEV_|set_dev_replay' "$lib"; then
      echo "build-core.sh: $lib carries a dev hook (MQ_DEV_ or set_dev_replay): debug assertions were on" >&2
      exit 1
    fi
  done
fi

# The engine in an own build's core, none in a store build: checked on each library before it is stripped.
for pair in "${TRIPLES[@]}"; do
  core_check_engine "$TARGET/${pair%%:*}/release/libmarquee_core.so" "${pair##*:}"
done
abis=()
for pair in "${TRIPLES[@]}"; do abis+=("${pair##*:}"); done
printf 'kind=%s\nengine_abis=%s\n' "$(core_kind)" "$(core_engine_abis "${abis[@]}")" \
  > "$ROOT/apps/android/core/core-build.properties"

rm -rf "$LIBS"
# A plain build drops a dev library left from before, which debug builds would otherwise package.
if [ "$DEV_ASSERTIONS" = false ]; then rm -rf "$ROOT/apps/android/core/src/debug/jniLibs"; fi
for pair in "${TRIPLES[@]}"; do
  triple=${pair%%:*}
  abi=${pair##*:}
  mkdir -p "$LIBS/$abi"
  "$STRIP" --strip-all -o "$LIBS/$abi/libmarquee_core.so" "$TARGET/$triple/release/libmarquee_core.so"
done
ls -l "$LIBS"/*/libmarquee_core.so
if [ "${#TRIPLES[@]}" -lt 2 ] && [ "$DEV_ASSERTIONS" = false ]; then
  echo "build-core.sh: only ${TRIPLES[0]##*:} built; a release or bundle build needs both ABIs" >&2
fi
