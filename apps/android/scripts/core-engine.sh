#!/bin/bash
# Which core build-core.sh makes, shared with its test (scripts/core-engine.test.ts). Every own build carries
# the torrent engine (plans/play-every-version.md, Phase 0); STORE=1 builds the store core without it, on
# purpose (plans/architecture.md, App Store note).

# The cargo features: MQ_FEATURES when set (even empty), else torrent; with STORE=1 none unless named.
core_features() {
  if [ "${STORE:-}" = 1 ]; then
    echo "${MQ_FEATURES:-}"
  else
    echo "${MQ_FEATURES-torrent}"
  fi
}

# Whether an unstripped core library carries the engine: its symbols name the mq-engine crate, mangled as
# `9mq_engine` (a store core has `15mq_engine_sched`, the scheduler, and no `9mq_engine`).
core_has_engine() {
  grep -aqF '9mq_engine' "$1"
}

# Fails when a library is not the core it should be: the engine in an own build, none in a store build.
core_check_engine() {
  if [ "${STORE:-}" = 1 ]; then
    if core_has_engine "$1"; then
      echo "build-core.sh: $1 carries the torrent engine, but STORE=1 builds the store core" >&2
      return 1
    fi
  elif ! core_has_engine "$1"; then
    echo "build-core.sh: $1 has no torrent engine; own builds carry it (STORE=1 builds without it on purpose)" >&2
    return 1
  fi
}
