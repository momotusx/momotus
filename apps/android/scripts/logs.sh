#!/bin/bash
# Prints the app's recent log lines (tags marquee*, crashes, linker errors).
# Usage: logs.sh [filter-regex]
HERE=$(cd "$(dirname "$0")" && pwd)
"$HERE/sdk.sh" adb logcat -d -v time | grep -E "${1:-marquee|AndroidRuntime|FATAL EXCEPTION|UnsatisfiedLink}" | tail -${LINES_MAX:-60}
