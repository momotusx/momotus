#!/bin/bash
# Runs an Android SDK command-line tool with the JDK 17 it requires.
# Usage: sdk.sh avdmanager list avd | sdk.sh sdkmanager --list | sdk.sh adb devices
#
# The adb server is shared with other work on this machine (including a real device on
# the network), so adb always targets this project's emulator, pinned to EMU_PORT.
set -euo pipefail

export JAVA_HOME=${JAVA_HOME:-$HOME/.local/jdks/zulu17.68.203-ca-jdk17.0.20.1-macosx_aarch64/Contents/Home}
export ANDROID_HOME=${ANDROID_HOME:-/opt/homebrew/share/android-commandlinetools}
export ANDROID_SDK_ROOT=$ANDROID_HOME
export EMU_PORT=${EMU_PORT:-5584}
export ANDROID_SERIAL=${ANDROID_SERIAL:-emulator-$EMU_PORT}

tool=$1
shift
case "$tool" in
  adb) exec "$ANDROID_HOME/platform-tools/adb" "$@" ;;
  emulator) exec "$ANDROID_HOME/emulator/emulator" "$@" ;;
  *) exec "$ANDROID_HOME/cmdline-tools/latest/bin/$tool" "$@" ;;
esac
