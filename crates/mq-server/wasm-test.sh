#!/usr/bin/env bash
# Runs mq-server's wasm detect checks under the TV's Nodes (plans/webos.md
# 3.4, 6): built by the section 2 recipe (pure MVP through build-std, the
# flags in .cargo/config.toml), glued by wasm-bindgen 0.2.129, lowered to
# Node 8 by esbuild with the stale-view rewrite, then run by tests/wasm/run.js
# over the service's host.js against a local server. TOOLS holds the
# prebuilt wasm-bindgen; Nodes come from fnm.
set -euo pipefail
cd "$(dirname "$0")/../.."
: "${TOOLS:?set TOOLS to the dir with wasm-bindgen 0.2.129}"
: "${CARGO_TARGET_DIR:?set CARGO_TARGET_DIR}"
export CC_wasm32_unknown_unknown="${CC_wasm32_unknown_unknown:-/opt/homebrew/opt/llvm@20/bin/clang}"
export AR_wasm32_unknown_unknown="${AR_wasm32_unknown_unknown:-/opt/homebrew/opt/llvm@20/bin/llvm-ar}"
export CFLAGS_wasm32_unknown_unknown="-mcpu=mvp"
NODES="${NODES:-8.12.0 12.21.0 16.19.1 20.12.2}"
out="$CARGO_TARGET_DIR/mq-server-selftest"
RUSTC_BOOTSTRAP=1 CARGO_INCREMENTAL=0 \
  CARGO_PROFILE_RELEASE_STRIP=false \
  cargo rustc -j 4 -Zbuild-std=std,panic_abort --target wasm32-unknown-unknown \
  -p mq-server --features selftest --release --crate-type cdylib
rm -rf "${out:?}"
mkdir -p "$out"
"$TOOLS/wasm-bindgen" --target nodejs --out-dir "$out" \
  "$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/mq_server.wasm"
${ESBUILD:-bunx esbuild@0.24.0} "$out/mq_server.js" --target=node8 --format=cjs --platform=node \
  --log-level=warning --outfile="$out/mq_server.lowered.js"
# V8 6.2 does not detach the old buffer when wasm memory grows.
sed -E "s/(cached[A-Za-z0-9]+Memory0)\.byteLength === 0/\1.buffer !== wasm.memory.buffer/g" \
  "$out/mq_server.lowered.js" >"$out/mq_server.node8.js"
if grep -q "Memory0.byteLength === 0" "$out/mq_server.node8.js"; then
  echo "node8 glue rewrite missed"
  exit 1
fi
for v in $NODES; do
  fnm exec --using "$v" node crates/mq-server/tests/wasm/run.js "$out/mq_server.node8.js"
done
