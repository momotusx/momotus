#!/bin/bash
# Starts an AVD headless on a pinned port and waits until Android has booted.
# Usage: emu.sh <avd-name> [extra emulator args]   (stop with: sdk.sh adb emu kill)
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
AVD=${1:?avd name}
shift || true
export EMU_PORT=${EMU_PORT:-5584}

nohup "$HERE/sdk.sh" emulator -avd "$AVD" -port "$EMU_PORT" -no-window -no-snapshot -no-audio \
  -no-boot-anim -gpu host -netdelay none -netspeed full "$@" > "${TMPDIR:-/tmp}/emu-$AVD.log" 2>&1 &

"$HERE/sdk.sh" adb wait-for-device
until [ "$("$HERE/sdk.sh" adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = "1" ]; do
  sleep 2
done
"$HERE/sdk.sh" adb shell input keyevent KEYCODE_WAKEUP
echo "booted: $AVD on emulator-$EMU_PORT"
