#!/usr/bin/env bash
# Builds Momotus's Windows installer on a Windows machine with MSVC, bun and NSIS (GitHub's windows runner:
# ci.yml and release-please.yml `windows`, plans/windows-app.md 2.5): the core (mq-bridge.exe, torrent engine in),
# the launcher (Momotus.exe), the web app, then makensis. Release builds pass the MARQUEE_* keys the core
# compiles in, as the webOS build does.
#   apps/windows/build.sh <out dir>
# Writes momotus-windows-x64-<version>-setup.exe and its .sha256 into <out dir>.
set -euo pipefail

out=${1:?usage: apps/windows/build.sh <out dir>}
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
version=$(sed -n 's/^  "version": "\(.*\)",$/\1/p' "$repo/apps/web/package.json" | head -1)
[[ -n $version ]] || { echo "build.sh: no version in apps/web/package.json" >&2; exit 1; }
target=${CARGO_TARGET_DIR:-$repo/target}/release

cd "$repo"
cargo build --release --locked -p mq-bridge --bin mq-bridge --features torrent
cargo build --release --locked -p momotus-win
(cd apps/web && bun install --frozen-lockfile >/dev/null && bun run build)

mkdir -p "$out"
out=$(cd "$out" && pwd)
stage=$(mktemp -d)
trap 'rm -rf "${stage:?}"' EXIT
cp "$target/Momotus.exe" "$target/mq-bridge.exe" "$stage/"
cp -R apps/web/dist "$stage/web"
cp LICENSE "$stage/LICENSE.txt"

setup=$out/momotus-windows-x64-$version-setup.exe
rm -f "${setup:?}"
# makensis takes Windows paths for the folder it packs.
win() { if command -v cygpath >/dev/null; then cygpath -w "$1"; else echo "$1"; fi; }
(cd "$here" && makensis -V2 -DVERSION="$version" -DSTAGE="$(win "$stage")" -DOUTFILE="$(win "$setup")" installer.nsi)
(cd "$out" && sha256sum "$(basename "$setup")" >"$(basename "$setup").sha256")
ls -l "$setup"
