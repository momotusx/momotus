#!/bin/bash
# Turns a screenshot PNG into a JPG of at most about 240 KB (the repo keeps screenshots as small
# JPGs, never PNGs). Portrait shots are scaled to 1600 px tall, landscape ones to 1600 px wide,
# then the quality drops until the file fits.
# Usage: shot-jpg.sh in.png out.jpg
set -euo pipefail

IN=${1:?input png}
OUT=${2:?output jpg}
LIMIT=240000

read -r WIDTH HEIGHT < <(sips -g pixelWidth -g pixelHeight "$IN" | awk '/pixel(Width|Height)/ {print $2}' | paste -sd' ' -)
if [ "$HEIGHT" -gt "$WIDTH" ]; then SCALE=(--resampleHeight 1600); else SCALE=(--resampleWidth 1600); fi

QUALITY=72
while :; do
  sips -s format jpeg -s formatOptions "$QUALITY" "${SCALE[@]}" "$IN" --out "$OUT" > /dev/null
  SIZE=$(wc -c < "$OUT")
  if [ "$SIZE" -le "$LIMIT" ] || [ "$QUALITY" -le 30 ]; then break; fi
  QUALITY=$((QUALITY - 8))
done
