#!/bin/bash
# Runs the Gradle wrapper with the JDK and SDK this project expects.
# JAVA_HOME and ANDROID_HOME can be overridden from the environment.
set -euo pipefail

HERE=$(cd "$(dirname "$0")/.." && pwd)
export JAVA_HOME=${JAVA_HOME:-$HOME/.local/jdks/zulu17.68.203-ca-jdk17.0.20.1-macosx_aarch64/Contents/Home}
export ANDROID_HOME=${ANDROID_HOME:-/opt/homebrew/share/android-commandlinetools}
export ANDROID_SDK_ROOT=$ANDROID_HOME
# Gradle's install tasks go to every connected device unless one is named. The adb server is
# shared with a real device on the network that is not ours, so always name this project's emulator.
export EMU_PORT=${EMU_PORT:-5584}
export ANDROID_SERIAL=${ANDROID_SERIAL:-emulator-$EMU_PORT}

cd "$HERE"
exec ./gradlew "$@"
