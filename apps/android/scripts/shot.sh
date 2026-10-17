#!/bin/bash
# Saves a screenshot of the pinned emulator into bench/m6/screens.
# Usage: shot.sh <name>   (writes bench/m6/screens/<name>.png)
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$HERE/../../../bench/m6/screens
mkdir -p "$OUT"
"$HERE/sdk.sh" adb exec-out screencap -p > "$OUT/${1:?name}.png"
echo "$OUT/$1.png"
