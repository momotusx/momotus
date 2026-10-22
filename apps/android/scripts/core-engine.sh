#!/bin/bash
# Which core build-core.sh makes, shared with its test (scripts/core-engine.test.ts). Every own build carries
# the torrent engine on arm64-v8a (plans/play-every-version.md, Step 0); armeabi-v7a stays without it until an
# engine slice is shown to work there (Step 0b). STORE=1 builds the store core without it, on purpose
# (plans/architecture.md, App Store note).

# What the app is told it is (BuildConfig.CORE_BUILD): "own" or "store".
core_kind() {
  if [ "${STORE:-}" = 1 ]; then echo store; else echo own; fi
}

# Whether the core for one ABI must carry the engine: an own build's arm64-v8a slice.
core_wants_engine() {
  [ "$(core_kind)" = own ] && [ "$1" = arm64-v8a ]
}

# The cargo features for one ABI: MQ_FEATURES (default torrent) where the engine belongs, else MQ_FEATURES in a
# store build and none on armeabi-v7a.
core_features() {
  if core_wants_engine "$1"; then
    echo "${MQ_FEATURES-torrent}"
  elif [ "$(core_kind)" = store ]; then
    echo "${MQ_FEATURES:-}"
  fi
}

# Whether an unstripped core library carries the engine: its symbols name the mq-engine crate, mangled as
# `9mq_engine`. A plain `mq_engine` is not enough: a store core has `15mq_engine_sched`, the scheduler.
core_has_engine() {
  grep -aqF '9mq_engine' "$1"
}

# Fails when the library for an ABI is not the core it should be: the engine in an own arm64-v8a build, none in
# a store build or on armeabi-v7a.
core_check_engine() {
  local lib=$1 abi=$2
  if core_wants_engine "$abi"; then
    if ! core_has_engine "$lib"; then
      echo "build-core.sh: $lib has no torrent engine; own builds carry it (STORE=1 builds without it on purpose)" >&2
      return 1
    fi
  elif core_has_engine "$lib"; then
    if [ "$(core_kind)" = store ]; then
      echo "build-core.sh: $lib carries the torrent engine, but STORE=1 builds the store core" >&2
    else
      echo "build-core.sh: $lib carries the torrent engine; $abi stays without it until its slice is shown to work" >&2
    fi
    return 1
  fi
}
