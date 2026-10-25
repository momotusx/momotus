#!/usr/bin/env bash
# The iPhone walk of plans/player-gestures.md 7.2 on a connected device: builds the app as app.marquee.walk (never the
# owner's app) and runs MarqueeWalkUITests against it. The core must be this checkout's own (build-core.sh).
#
# Usage: apps/apple/scripts/walk-ui.sh <device UDID> [xcodebuild settings, such as the signing team]
#   (xcrun devicectl list devices for the UDID)
# Shots land in the result bundle under the scratch dir; uninstall the walk app and the runner after:
#   xcrun devicectl device uninstall app --device <UDID> app.marquee.walk
#   xcrun devicectl device uninstall app --device <UDID> app.marquee.walk.uitests.xctrunner
set -euo pipefail

udid=${1:?usage: walk-ui.sh <device UDID>}
app_id=app.marquee.walk
apple="$(cd "$(dirname "$0")/.." && pwd)"
out=${WALK_OUT:-$HOME/.local/share/marquee-walks/player-gestures-ios}
mkdir -p "$out"
cd "$apple"
xcodegen generate -q
# The id goes to the build (MARQUEE_APP_ID) and to the test (MQ_WALK_APP, through xcodebuild's TEST_RUNNER_ prefix);
# both refuse anything else.
# MQ_WALK_ARGS adds launch arguments for the walk install, such as -MQInstallAddons <manifest URLs>.
# -allowProvisioningUpdates lets xcodebuild register app.marquee.walk (and the runner, app.marquee.walk.uitests) and
# their development profiles in the owner's Apple developer team, from the signing settings passed after the UDID.
TEST_RUNNER_MQ_WALK_APP=$app_id TEST_RUNNER_MQ_WALK_ARGS=${MQ_WALK_ARGS:-} xcodebuild test -project Marquee.xcodeproj -scheme MarqueeWalkUITests \
  -destination "id=$udid" -derivedDataPath "$out/dd" -resultBundlePath "$out/walk-$(date +%Y%m%d-%H%M%S).xcresult" \
  -allowProvisioningUpdates "${@:2}" MARQUEE_APP_ID=$app_id
