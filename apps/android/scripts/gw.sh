#!/bin/bash
# Runs the Gradle wrapper with the JDK and SDK this project expects.
# JAVA_HOME and ANDROID_HOME can be overridden from the environment.
set -euo pipefail

HERE=$(cd "$(dirname "$0")/.." && pwd)
# JDK 21: Robolectric needs it for SDK 36 (the player's tests), and one JDK for the whole gate keeps
# locale data, and so formatted dates, the same everywhere.
export JAVA_HOME=${JAVA_HOME:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}
export ANDROID_HOME=${ANDROID_HOME:-/opt/homebrew/share/android-commandlinetools}
export ANDROID_SDK_ROOT=$ANDROID_HOME
# Gradle's install tasks go to every connected device unless one is named. The adb server is
# shared with a real device on the network that is not ours, so always name this project's emulator.
export EMU_PORT=${EMU_PORT:-5584}
export ANDROID_SERIAL=${ANDROID_SERIAL:-emulator-$EMU_PORT}

cd "$HERE"
exec ./gradlew "$@"
