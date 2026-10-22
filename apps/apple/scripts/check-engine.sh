#!/usr/bin/env bash
# check-engine.sh <lib>...: fails when a core library has no torrent engine, which an own build's
# iOS and tvOS slices must carry (build-core.sh; STORE=1 builds the store core without it on purpose).
# The engine is the mq-engine crate: its symbols (`9mq_engine` in a mangled name, never
# mq_engine_sched's `15mq_engine_sched`) are in the library only with `--features torrent`.
set -euo pipefail
for lib in "$@"; do
  # grep -c reads all of nm's output: grep -q would close the pipe early and fail it under pipefail.
  count=$(nm "$lib" 2>/dev/null | grep -cE '(^|[^0-9])9mq_engine' || true)
  if [[ "$count" == 0 ]]; then
    echo "check-engine.sh: $lib has no torrent engine; build without STORE=1, or set STORE=1 for a store core on purpose" >&2
    exit 1
  fi
done
