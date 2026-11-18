#!/usr/bin/env bash
# Packages the webOS app (plans/webos.md section 4) into one IPK: the stub, the service, the UI built in webos mode
# (apps/web, `bun run build:webos`, which also runs the Chromium 79 bundle check) and the wasm core, with the webOS CLI
# (@webos-tools/cli 3.2.6, installed by bun into its own dir). Runs on macOS and on Linux CI.
#   CORE_DIR       dir holding mq_wasm.js and mq_wasm_bg.wasm (default apps/webos/service/core, where the wasm build writes)
#   OUT            output dir (default apps/webos/out, gitignored)
#   NODE           the Node that runs the CLI; else Node 20.12.2 through fnm when fnm is there; else `node` on PATH
#   WEBOS_CLI_DIR  where the CLI is installed (default ~/.cache/momotus/webos-cli-3.2.6)
#   APP_ID         optional id rewrite, for walk builds and the Simulators (which cut a Luna service name from its
#                  first "com": com.momotus.webos.sim). It also moves the service's port to WALK_PORT (default
#                  47811), so a walk build never shares 127.0.0.1:47801 with the owner's app. The UI takes its
#                  origin from the page, so it needs no rewrite.
#   CA_BUNDLE      optional Mozilla cacert.pem, shipped beside host.js
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
web="$here/../web"
OUT="${OUT:-$here/out}"
CORE_DIR="${CORE_DIR:-$here/service/core}"
CLI_VERSION=3.2.6
WEBOS_CLI_DIR="${WEBOS_CLI_DIR:-$HOME/.cache/momotus/webos-cli-$CLI_VERSION}"

# 1. The UI, built and checked for Chromium 79.
(cd "$web" && bun install --frozen-lockfile >/dev/null && bun run build:webos)
ui="$web/dist-webos"

# 2. The core: the wasm build's two files, or a stop that says so.
for f in mq_wasm.js mq_wasm_bg.wasm; do
  if [ ! -f "$CORE_DIR/$f" ]; then
    echo "package.sh: $CORE_DIR/$f is missing. Build the wasm core first (it writes mq_wasm.js and mq_wasm_bg.wasm), or set CORE_DIR to where it wrote them." >&2
    exit 1
  fi
done

# 3. The webOS CLI and the Node that runs it.
if [ ! -x "$WEBOS_CLI_DIR/node_modules/.bin/ares-package" ]; then
  mkdir -p "$WEBOS_CLI_DIR"
  echo '{ "private": true }' >"$WEBOS_CLI_DIR/package.json"
  (cd "$WEBOS_CLI_DIR" && bun add --exact "@webos-tools/cli@$CLI_VERSION" >/dev/null)
fi
# fstream (under ares-package) calls mkdirp's 0.5 callback API, but its ">=0.5" takes the hoisted 3.x ("mkdir is not a
# function"): it gets a 0.5 copy of its own, taken from a package that already nests one.
if [ ! -d "$WEBOS_CLI_DIR/node_modules/fstream/node_modules/mkdirp" ]; then
  old="$(grep -l '"version": "0\.5\.' "$WEBOS_CLI_DIR"/node_modules/*/node_modules/mkdirp/package.json | head -1)"
  [ -n "$old" ] || { echo "package.sh: no mkdirp 0.5 in the webOS CLI's tree to give fstream" >&2; exit 1; }
  mkdir -p "$WEBOS_CLI_DIR/node_modules/fstream/node_modules"
  cp -R "$(dirname "$old")" "$WEBOS_CLI_DIR/node_modules/fstream/node_modules/mkdirp"
fi
if [ -n "${NODE:-}" ]; then
  node_run=("$NODE")
elif command -v fnm >/dev/null; then
  node_run=(fnm exec --using=v20.12.2 node)
else
  node_run=(node)
fi

# 4. The stage: the stub app and the service with the UI and the core inside it.
stage="$OUT/stage"
rm -rf "${stage:?}"
mkdir -p "$stage/app" "$stage/service/core"
cp "$here"/app/{appinfo.json,index.html,icon.png,largeIcon.png,splash.png} "$stage/app/"
cp "$here"/service/{index.js,core-worker.js,host.js,keystore.js,services.json,package.json} "$stage/service/"
cp -R "$ui" "$stage/service/ui"
cp "$CORE_DIR/mq_wasm.js" "$CORE_DIR/mq_wasm_bg.wasm" "$stage/service/core/"
[ -z "${CA_BUNDLE:-}" ] || cp "$CA_BUNDLE" "$stage/service/cacert.pem"
if [ -n "${APP_ID:-}" ]; then
  walk_port="${WALK_PORT:-47811}"
  # Only the named text files: the core and the CA bundle stay byte for byte.
  for f in app/index.html app/appinfo.json service/index.js service/services.json service/package.json; do
    perl -pi -e "s/app\.momotus\.webos/$APP_ID/g; s/47801/$walk_port/g" "$stage/$f"
  done
fi
(cd "$stage/service" && bun install --production --no-save >/dev/null && rm -f bun.lock bun.lockb)

# 5. The IPK.
rm -f "$OUT"/*.ipk
"${node_run[@]}" "$WEBOS_CLI_DIR/node_modules/.bin/ares-package" "$stage/app" "$stage/service" -o "$OUT" --no-minify
ipk="$(ls "$OUT"/*.ipk)"

# 6. Old tools wrote zero timestamps into the archives, which TVs refuse.
check="$OUT/ipk-check"
rm -rf "${check:?}"
mkdir -p "$check"
(cd "$check" && ar x "$ipk")
for part in control data; do
  if tar -tvzf "$check/$part.tar.gz" | grep -qE ' 1970 |1970-01-01'; then
    echo "package.sh: $ipk holds zero timestamps in $part.tar.gz" >&2
    exit 1
  fi
done
rm -rf "${check:?}" "${stage:?}"
ls -la "$ipk"
