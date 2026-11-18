#!/bin/zsh
# Builds examples/host_e2e.rs as pure wasm MVP by the plans/webos.md section 2
# recipe and runs run.js under the TV's Nodes (8.12.0, 12.21.0, 16.19.1,
# 20.12.2) through fnm. `run.sh default` builds with the stable defaults
# instead (no build-std, so no std sources to fetch), which only Node 20
# loads. WASM_BINDGEN is the 0.2.129 CLI, ESBUILD an esbuild binary. Run it
# through build-lock.sh. The wasm build keeps its own dir under the lane's
# target dir: RUSTC_BOOTSTRAP sends thiserror and proc-macro2 down other
# code paths.
set -euo pipefail
: "${WASM_BINDGEN:?set WASM_BINDGEN to the wasm-bindgen 0.2.129 binary}"
: "${ESBUILD:?set ESBUILD to an esbuild binary}"
mode="${1:-mvp}"
cd "${0:A:h}/../../../.."
TARGET="${CARGO_TARGET_DIR:-$HOME/.cargo-targets/marquee-webos-http}/wasm-$mode"
OUT="$TARGET/host-e2e"
LLVM=/opt/homebrew/opt/llvm@20/bin
if [ "$mode" = mvp ]; then
  RUSTC_BOOTSTRAP=1 \
    CC_wasm32_unknown_unknown="$LLVM/clang" AR_wasm32_unknown_unknown="$LLVM/llvm-ar" \
    CFLAGS_wasm32_unknown_unknown="-mcpu=mvp" \
    CARGO_PROFILE_RELEASE_STRIP=false CARGO_PROFILE_RELEASE_LTO=false \
    CARGO_TARGET_DIR="$TARGET" CARGO_INCREMENTAL=0 \
    cargo build --offline -j 4 -Zbuild-std=std,panic_abort --target wasm32-unknown-unknown \
    -p mq-http --example host_e2e --release
  nodes=(8.12.0 12.21.0 16.19.1 20.12.2)
else
  CARGO_PROFILE_RELEASE_STRIP=false CARGO_PROFILE_RELEASE_LTO=false \
    CARGO_TARGET_DIR="$TARGET" CARGO_INCREMENTAL=0 \
    cargo build --offline -j 4 --target wasm32-unknown-unknown \
    -p mq-http --example host_e2e --release
  nodes=(20.12.2)
fi
rm -rf "${OUT:?}"
mkdir -p "$OUT"
"$WASM_BINDGEN" --target nodejs --out-dir "$OUT" "$TARGET/wasm32-unknown-unknown/release/examples/host_e2e.wasm"
# The service's glue steps (section 2): syntax lowered for Node 8, and each
# cached memory view checked against the live buffer (V8 6.2 keeps the old
# one when wasm grows memory).
"$ESBUILD" "$OUT/host_e2e.js" --target=node8 --format=cjs --platform=node \
  --log-level=warning --outfile="$OUT/host_e2e.lowered.js"
sed -E 's/(cached[A-Za-z0-9]+Memory0)\.byteLength === 0/\1.buffer !== wasm.memory.buffer/g' \
  "$OUT/host_e2e.lowered.js" > "$OUT/host_e2e.node8.js"
if grep -q 'Memory0.byteLength === 0' "$OUT/host_e2e.node8.js"; then echo "glue patch missed"; exit 1; fi
echo "$mode wasm $(stat -f %z "$OUT/host_e2e_bg.wasm") bytes, core $(git rev-parse --short HEAD)"
failed=0
for node in $nodes; do
  fnm exec --using="$node" node crates/mq-http/tests/host-e2e/run.js "$OUT/host_e2e.node8.js" || failed=1
done
exit $failed
