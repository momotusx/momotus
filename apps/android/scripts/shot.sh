#!/bin/bash
# Saves a screenshot of the pinned emulator into bench/m6/screens as a small JPG.
# Usage: shot.sh <name>   (writes bench/m6/screens/<name>.jpg)
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$HERE/../../../bench/m6/screens
mkdir -p "$OUT"
TMP=$(mktemp -t marquee-shot).png
"$HERE/sdk.sh" adb exec-out screencap -p > "$TMP"
"$HERE/shot-jpg.sh" "$TMP" "$OUT/${1:?name}.jpg"
rm -f "$TMP"
echo "$OUT/$1.jpg"
