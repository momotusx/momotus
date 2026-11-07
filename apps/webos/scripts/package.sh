#!/usr/bin/env bash
# Packages the webOS app (plans/webos.md section 4): the stub, the service,
# the built UI and the built core into one IPK with the webOS CLI
# (@webos-tools/cli 3.2.6; its fstream needs mkdirp 0.5 nested under it).
#   ARES      dir holding ares-package (required)
#   CORE_DIR  dir holding mq_wasm.js and mq_wasm_bg.wasm (required)
#   UI_DIR    the UI built in webos mode (default apps/web/dist-webos)
#   APP_ID    optional id rewrite, for walk builds and the Simulators (which
#             cut a Luna service name from its first "com": com.momotus.webos.sim).
#             It also moves the service's port to WALK_PORT (default 47811), so
#             a walk build never shares 127.0.0.1:47801 with the owner's app.
#             The UI takes its origin from the page, so it needs no rewrite.
#   CA_BUNDLE optional Mozilla cacert.pem, shipped beside host.js
#   OUT       output dir (default apps/webos/out, gitignored)
set -euo pipefail
cd "$(dirname "$0")/.."
OUT="${OUT:-out}"
UI_DIR="${UI_DIR:-../web/dist-webos}"
: "${ARES:?set ARES to the webOS CLI bin dir}"
: "${CORE_DIR:?set CORE_DIR to the dir with the built mq_wasm.js and mq_wasm_bg.wasm}"
[ -f "$UI_DIR/index.html" ] || { echo "no built UI at $UI_DIR: build apps/web in webos mode first" >&2; exit 1; }
for f in mq_wasm.js mq_wasm_bg.wasm; do
  [ -f "$CORE_DIR/$f" ] || { echo "$CORE_DIR/$f is missing: build the core first" >&2; exit 1; }
done
stage="$OUT/stage"
rm -rf "${stage:?}"; mkdir -p "$stage/app" "$stage/service/core"
cp app/appinfo.json app/index.html "$stage/app/"
sips -z 80 80 ../web/public/icon-192.png --out "$stage/app/icon.png" >/dev/null
sips -z 130 130 ../web/public/icon-192.png --out "$stage/app/largeIcon.png" >/dev/null
cp service/index.js service/core-worker.js service/host.js service/keystore.js service/services.json service/package.json "$stage/service/"
cp -R "$UI_DIR" "$stage/service/ui"
cp "$CORE_DIR"/* "$stage/service/core/"
[ -z "${CA_BUNDLE:-}" ] || cp "$CA_BUNDLE" "$stage/service/cacert.pem"
if [ -n "${APP_ID:-}" ]; then
  walk_port="${WALK_PORT:-47811}"
  # Only the named text files: the core and the CA bundle stay byte for byte.
  for f in app/index.html app/appinfo.json service/index.js service/services.json service/package.json; do
    sed -i "" -e "s/app\.momotus\.webos/$APP_ID/g" -e "s/47801/$walk_port/g" "$stage/$f"
  done
fi
(cd "$stage/service" && bun install --production --no-save >/dev/null && rm -f bun.lock bun.lockb)
"$ARES/ares-package" "$stage/app" "$stage/service" -o "$OUT" --no-minify
# Old tools wrote zero timestamps into the archives, which TVs reject.
ipk=$(ls -t "$OUT"/*.ipk | head -1)
check="$OUT/ipk-check"
rm -rf "${check:?}"; mkdir -p "$check"
(cd "$check" && ar x "../$(basename "$ipk")")
if tar -tvzf "$check/data.tar.gz" | grep -qE ' 1970 |1970-01-01'; then
  echo "$ipk holds zero timestamps" >&2
  exit 1
fi
rm -rf "${check:?}"
ls -la "$ipk"
