#!/usr/bin/env bash
# The Windows installer's smoke test on a Windows machine (ci.yml `windows`, plans/windows-app.md 2.5): installs
# the setup silently, checks the core serves the app and its socket's origin, opens Momotus and screenshots its
# window, closes it and checks the core ended with it, then uninstalls.
#   apps/windows/smoke.sh <setup.exe> <shot.png>
set -euo pipefail

setup=$(cygpath -w "${1:?usage: smoke.sh <setup.exe> <shot.png>}")
shot=$(cygpath -w "${2:?usage: smoke.sh <setup.exe> <shot.png>}")
dir=$(cygpath -u "$LOCALAPPDATA")/Programs/Momotus
ps() { powershell -NoProfile -NonInteractive -Command "$1"; }
running() { tasklist //FI "IMAGENAME eq $1" //NH | grep -qi "$1"; }

echo "install"
ps "Start-Process -Wait -FilePath '$setup' -ArgumentList '/S'"
for f in Momotus.exe mq-bridge.exe web/index.html Uninstall.exe; do test -e "$dir/$f" || { echo "missing $f"; exit 1; }; done
reg query 'HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Momotus' //v DisplayVersion

echo "core alone: ready line, the app, the socket's origin"
data=$(mktemp -d)
MQ_DATA=$(cygpath -w "$data") MQ_STATIC=$(cygpath -w "$dir/web") MQ_BRIDGE_PORT=0 "$dir/mq-bridge.exe" >"$data/out" 2>"$data/err" &
core=$!
for _ in $(seq 1 120); do grep -q bridge_ready "$data/out" && break; sleep 0.5; done
port=$(sed -n 's/.*"url":"ws:\/\/127.0.0.1:\([0-9]*\)\/ws".*/\1/p' "$data/out")
[[ -n $port ]] || { echo "no ready line"; cat "$data/err"; exit 1; }
curl -fsS "http://127.0.0.1:$port/" | grep -qi '<html' || { echo "the app is not served"; exit 1; }
# The socket upgrades for the page's own origin only.
code() { curl -s -o /dev/null -w '%{http_code}' --http1.1 -H 'Connection: Upgrade' -H 'Upgrade: websocket' \
  -H 'Sec-WebSocket-Version: 13' -H 'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==' -H "Origin: $1" \
  --max-time 3 "http://127.0.0.1:$port/ws" || true; }
[[ $(code "http://127.0.0.1:$port") == 101 ]] || { echo "the app's own origin is refused"; exit 1; }
[[ $(code "http://evil.example") != 101 ]] || { echo "another origin is let in"; exit 1; }
kill "$core"
for _ in $(seq 1 20); do running mq-bridge.exe || break; sleep 1; done

echo "the app: window, core beside it, both end on close"
ps "Start-Process -FilePath '$(cygpath -w "$dir/Momotus.exe")'"
for _ in $(seq 1 60); do running mq-bridge.exe && break; sleep 1; done
running mq-bridge.exe || { echo "the core did not start"; cat "$(cygpath -u "$LOCALAPPDATA")/Momotus/logs/core.log" || true; exit 1; }
sleep 20
ps "Add-Type -AssemblyName System.Windows.Forms,System.Drawing; \$b=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds; \$i=New-Object System.Drawing.Bitmap \$b.Width,\$b.Height; [System.Drawing.Graphics]::FromImage(\$i).CopyFromScreen(\$b.Location,[System.Drawing.Point]::Empty,\$b.Size); \$i.Save('$shot')"
running Momotus.exe || { echo "Momotus ended by itself"; exit 1; }
taskkill //IM Momotus.exe >/dev/null
for _ in $(seq 1 20); do running mq-bridge.exe || break; sleep 1; done
running mq-bridge.exe && { echo "the core outlived the window"; exit 1; }
running Momotus.exe && { echo "Momotus did not close"; exit 1; }

echo "uninstall"
# The uninstaller runs a copy of itself from the temp folder, so -Wait returns early: wait for the files to go.
ps "Start-Process -Wait -FilePath '$(cygpath -w "$dir/Uninstall.exe")' -ArgumentList '/S'"
for _ in $(seq 1 30); do test -e "$dir/Momotus.exe" || break; sleep 1; done
test ! -e "$dir/Momotus.exe" || { echo "uninstall left Momotus.exe"; exit 1; }
echo "smoke ok"
