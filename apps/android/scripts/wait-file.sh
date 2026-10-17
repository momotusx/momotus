#!/bin/bash
# Waits until a file inside the app's private storage contains a marker, then prints it.
# Usage: wait-file.sh <path-under-files/> <marker> [max-seconds]
HERE=$(cd "$(dirname "$0")" && pwd)
file=$1
marker=$2
max=${3:-60}
start=$(date +%s)
while true; do
  out=$("$HERE/sdk.sh" adb shell run-as app.marquee.android cat "files/$file" 2>/dev/null || true)
  if echo "$out" | grep -q "$marker"; then echo "$out"; exit 0; fi
  if [ $(( $(date +%s) - start )) -ge "$max" ]; then echo "$out"; echo "timeout waiting for '$marker'"; exit 1; fi
  sleep 2
done
