#!/usr/bin/env bash
# Runs mq-library's wasm selftest under the TV's Nodes (plans/webos.md 3.3,
# 6): built by the section 2 recipe (pure MVP through build-std, SQLite's C
# half by clang for wasm32), glued by wasm-bindgen 0.2.129, lowered to Node 8
# by esbuild with the stale-view rewrite, then run by tests/wasm/run.js over
# the service's host.js: write, a write cut halfway, and a read in a new
# process. TOOLS holds the prebuilt wasm-bindgen; Nodes come from fnm. Run it
# through build-lock.sh like any cargo build.
set -euo pipefail
cd "$(dirname "$0")/../.."
: "${TOOLS:?set TOOLS to the dir with wasm-bindgen 0.2.129}"
: "${CARGO_TARGET_DIR:?set CARGO_TARGET_DIR}"
export CC_wasm32_unknown_unknown="${CC_wasm32_unknown_unknown:-/opt/homebrew/opt/llvm@20/bin/clang}"
export AR_wasm32_unknown_unknown="${AR_wasm32_unknown_unknown:-/opt/homebrew/opt/llvm@20/bin/llvm-ar}"
export CFLAGS_wasm32_unknown_unknown="-mcpu=mvp"
NODES="${NODES:-8.12.0 12.21.0 16.19.1 20.12.2}"
out="$CARGO_TARGET_DIR/mq-library-selftest"
RUSTC_BOOTSTRAP=1 CARGO_INCREMENTAL=0 \
  CARGO_PROFILE_RELEASE_STRIP=false \
  cargo rustc -j 4 -Zbuild-std=std,panic_abort --target wasm32-unknown-unknown \
  -p mq-library --features selftest --release --crate-type cdylib
rm -rf "${out:?}"
mkdir -p "$out"
"$TOOLS/wasm-bindgen" --target nodejs --out-dir "$out" \
  "$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/mq_library.wasm"
# ESBUILD may name a local esbuild binary (offline runs).
${ESBUILD:-bunx esbuild@0.24.0} "$out/mq_library.js" --target=node8 --format=cjs --platform=node \
  --log-level=warning --outfile="$out/mq_library.lowered.js"
# V8 6.2 does not detach the old buffer when wasm memory grows.
sed -E "s/(cached[A-Za-z0-9]+Memory0)\.byteLength === 0/\1.buffer !== wasm.memory.buffer/g" \
  "$out/mq_library.lowered.js" >"$out/mq_library.node8.js"
if grep -q "Memory0.byteLength === 0" "$out/mq_library.node8.js"; then
  echo "node8 glue rewrite missed"
  exit 1
fi
glue="$out/mq_library.node8.js"
for v in $NODES; do
  data="$out/data-$v"
  rm -rf "${data:?}"
  mkdir -p "$data"
  run() { fnm exec --using "$v" node crates/mq-library/tests/wasm/run.js "$glue" "$data" "$1"; }
  run write
  # Only the database and its (truncated) journal reach the disk; temp
  # files and the memory library stay off it.
  files=$(cd "$data" && ls -A | tr '\n' ' ')
  if [ "$files" != "library.sqlite library.sqlite-journal " ]; then
    echo "FAIL $v: files on disk: $files"
    exit 1
  fi
  rc=0
  run cut || rc=$?
  if [ "$rc" != 3 ]; then
    echo "FAIL $v: the cut phase ended with $rc, want 3"
    exit 1
  fi
  if [ ! -s "$data/library.sqlite-journal" ]; then
    echo "FAIL $v: no hot journal after the cut"
    exit 1
  fi
  run read
done
