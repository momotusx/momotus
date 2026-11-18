#!/usr/bin/env bash
# Runs mq-rt's wasm selftest under the TV's Nodes (plans/webos.md 3.1, 6):
# built by the section 2 recipe (pure MVP through build-std), glued by
# wasm-bindgen 0.2.129, lowered to Node 8 by esbuild with the stale-view
# rewrite, then run by tests/wasm/run.js with normal and early timers.
# TOOLS holds the prebuilt wasm-bindgen; Nodes come from fnm. Run it through
# build-lock.sh like any cargo build.
set -euo pipefail
cd "$(dirname "$0")/../.."
: "${TOOLS:?set TOOLS to the dir with wasm-bindgen 0.2.129}"
: "${CARGO_TARGET_DIR:?set CARGO_TARGET_DIR}"
NODES="${NODES:-8.12.0 12.21.0 16.19.1 20.12.2}"
out="$CARGO_TARGET_DIR/mq-rt-selftest"
RUSTC_BOOTSTRAP=1 CARGO_INCREMENTAL=0 \
  CARGO_PROFILE_RELEASE_STRIP=false \
  cargo rustc -j 4 -Zbuild-std=std,panic_abort --target wasm32-unknown-unknown \
  -p mq-rt --features selftest --release --crate-type cdylib
rm -rf "${out:?}"
mkdir -p "$out"
"$TOOLS/wasm-bindgen" --target nodejs --out-dir "$out" \
  "$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/mq_rt.wasm"
# ESBUILD may name a local esbuild binary (offline runs).
${ESBUILD:-bunx esbuild@0.24.0} "$out/mq_rt.js" --target=node8 --format=cjs --platform=node \
  --log-level=warning --outfile="$out/mq_rt.lowered.js"
# V8 6.2 does not detach the old buffer when wasm memory grows.
sed -E "s/(cached[A-Za-z0-9]+Memory0)\.byteLength === 0/\1.buffer !== wasm.memory.buffer/g" \
  "$out/mq_rt.lowered.js" >"$out/mq_rt.node8.js"
if grep -q "Memory0.byteLength === 0" "$out/mq_rt.node8.js"; then
  echo "node8 glue rewrite missed"
  exit 1
fi
for v in $NODES; do
  for early in 0 1; do
    MQ_EARLY=$early fnm exec --using "$v" node crates/mq-rt/tests/wasm/run.js "$out/mq_rt.node8.js"
  done
done
