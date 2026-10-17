#!/bin/bash
# Runs the Gradle wrapper with the JDK and SDK this project expects.
# JAVA_HOME and ANDROID_HOME can be overridden from the environment.
set -euo pipefail

HERE=$(cd "$(dirname "$0")/.." && pwd)
export JAVA_HOME=${JAVA_HOME:-$HOME/.local/jdks/zulu17.68.203-ca-jdk17.0.20.1-macosx_aarch64/Contents/Home}
export ANDROID_HOME=${ANDROID_HOME:-/opt/homebrew/share/android-commandlinetools}
export ANDROID_SDK_ROOT=$ANDROID_HOME

cd "$HERE"
exec ./gradlew "$@"
