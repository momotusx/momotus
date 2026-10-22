#!/usr/bin/env bash
# check-engine.sh <lib>...: fails when a core library has no torrent engine. Every own build carries
# it (build-core.sh, and the Xcode build over the linked xcframework); STORE=1 is a store core,
# engine-free on purpose, and passes with a note.
# The mark is mq-ffi's `MQ_ENGINE_TORRENT`, defined only with `--features torrent`.
set -euo pipefail
if [[ "${STORE:-}" == 1 ]]; then
  echo "check-engine.sh: store core (STORE=1): no torrent engine, on purpose"
  exit 0
fi
for lib in "$@"; do
  # A defined symbol (not U); grep -c reads all of nm's output, where grep -q would close the pipe
  # early and fail it under pipefail.
  count=$(nm -g "$lib" 2>/dev/null | grep -cE ' [A-TV-Z] _MQ_ENGINE_TORRENT$' || true)
  if [[ "$count" == 0 ]]; then
    echo "check-engine.sh: $lib has no torrent engine; build the core with apps/apple/scripts/build-core.sh, or set STORE=1 for a store core on purpose" >&2
    exit 1
  fi
done
