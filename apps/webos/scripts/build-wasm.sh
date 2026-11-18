#!/usr/bin/env bash
# Builds the webOS core (crates/mq-wasm) by the recipe of plans/webos.md
# section 2 and runs its gate. Pure MVP wasm, since webOS 6 runs Node 8.12
# (V8 6.2): std rebuilt through -Zbuild-std on the stable toolchain
# (RUSTC_BOOTSTRAP, this command only; thiserror and proc-macro2 then take
# their unstable paths too, one more reason the wasm target dir is its own),
# SQLite's C half by clang for wasm32, `-Ctarget-cpu=mvp` from
# .cargo/config.toml. Then wasm-bindgen, the glue lowered to Node 8 with the
# stale-view rewrite, wasm-opt, and the gate: (b) wasm-tools validates it as
# MVP, (c) no i64 crosses the boundary, (d) it loads and answers `ping` under
# the TV's Nodes.
#   --e2e      the web e2e's core: profile wasm-e2e with the dev hooks
#   OUT        output dir (default apps/webos/service/core, where the
#              service's core child and package.sh's CORE_DIR look)
#   TOOLS      prebuilt wasm-bindgen 0.2.129, wasm-opt and wasm-tools
#              (default .tools/bin)
#   NODES      fnm versions for gate (d) (default 8.12.0 12.21.0 16.19.1 20.12.2)
#   ESBUILD    an esbuild command (default bunx esbuild@0.24.0)
#   LLVM       dir with clang and llvm-ar for SQLite's C half (default
#              /opt/homebrew/opt/llvm@20/bin; Linux CI: /usr/lib/llvm-18/bin)
#   MIN_FREE_GB  free disk the build needs (default 5)
#   CARGO_TARGET_DIR  default ~/.cargo-targets/marquee-<checkout>-wasm
set -euo pipefail
cd "$(dirname "$0")/../../.."
root=$(pwd)
profile=wasm
features=()
if [ "${1:-}" = "--e2e" ]; then
  profile="wasm-e2e"
  features=(--features dev-hooks)
fi
OUT="${OUT:-apps/webos/service/core}"
TOOLS="${TOOLS:-$root/.tools/bin}"
NODES="${NODES:-8.12.0 12.21.0 16.19.1 20.12.2}"
ESBUILD="${ESBUILD:-bunx esbuild@0.24.0}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cargo-targets/marquee-$(basename "$root")-wasm}"
for tool in wasm-bindgen wasm-opt wasm-tools; do
  [ -x "$TOOLS/$tool" ] || { echo "no $tool in $TOOLS" >&2; exit 1; }
done
free_gb=$(df -Pk / | awk 'NR==2 {print int($4 / 1048576)}')
if [ "$free_gb" -lt "${MIN_FREE_GB:-5}" ]; then
  echo "free disk ${free_gb} GB, need ${MIN_FREE_GB:-5}" >&2
  exit 1
fi
sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1
}

LLVM="${LLVM:-/opt/homebrew/opt/llvm@20/bin}"
RUSTC_BOOTSTRAP=1 CARGO_INCREMENTAL=0 \
  CC_wasm32_unknown_unknown="$LLVM/clang" AR_wasm32_unknown_unknown="$LLVM/llvm-ar" \
  CFLAGS_wasm32_unknown_unknown="-mcpu=mvp" \
  cargo build -j 4 -Zbuild-std=std,panic_abort --target wasm32-unknown-unknown \
  -p mq-wasm --profile "$profile" "${features[@]}"
raw="$CARGO_TARGET_DIR/wasm32-unknown-unknown/$profile/mq_wasm.wasm"

mkdir -p "$OUT"
rm -f "${OUT:?}"/mq_wasm*
"$TOOLS/wasm-bindgen" --target nodejs --out-dir "$OUT" "$raw"
# Node 8 glue: syntax lowered, and every cached memory view checked against
# the live buffer: V8 6.2 does not detach the old buffer when wasm grows
# memory, so a byteLength check reads a stale view and decodes "".
$ESBUILD "$OUT/mq_wasm.js" --target=node8 --format=cjs --platform=node \
  --log-level=warning --outfile="$OUT/mq_wasm.lowered.js"
sed -E "s/(cached[A-Za-z0-9]+Memory0)\.byteLength === 0/\1.buffer !== wasm.memory.buffer/g" \
  "$OUT/mq_wasm.lowered.js" >"$OUT/mq_wasm.js"
rm -f "$OUT/mq_wasm.lowered.js"
if grep -q "Memory0.byteLength === 0" "$OUT/mq_wasm.js"; then
  echo "node8 glue rewrite missed" >&2
  exit 1
fi
"$TOOLS/wasm-opt" -Oz --mvp-features "$OUT/mq_wasm_bg.wasm" -o "$OUT/mq_wasm_bg.opt.wasm"
mv "$OUT/mq_wasm_bg.opt.wasm" "$OUT/mq_wasm_bg.wasm"

gate=0
"$TOOLS/wasm-tools" validate --features mvp "$OUT/mq_wasm_bg.wasm" \
  || { echo "gate b: not MVP" >&2; gate=1; }
# wasm-bindgen types an i64 export as bigint in both .d.ts files; an i64 host
# import shows on its import line.
if grep -q bigint "$OUT/mq_wasm.d.ts" "$OUT/mq_wasm_bg.wasm.d.ts" \
  || "$TOOLS/wasm-tools" print "$OUT/mq_wasm_bg.wasm" | grep -E '^\s*\(import ' | grep -q i64; then
  echo "gate c: i64 at the boundary" >&2
  gate=1
fi
for v in $NODES; do
  if answer=$(fnm exec --using "$v" node apps/webos/scripts/ping-core.js "$OUT/mq_wasm.js" 2>&1); then
    echo "gate d: node $v $answer"
  else
    echo "gate d: node $v refused: $answer" >&2
    gate=1
  fi
done
wasm="$OUT/mq_wasm_bg.wasm"
echo "core $(git rev-parse --short HEAD)$(git diff --quiet HEAD -- crates Cargo.toml Cargo.lock || echo '+dirty') profile $profile size $(wc -c <"$wasm" | tr -d ' ') sha256 $(sha256 "$wasm")"
exit "$gate"
