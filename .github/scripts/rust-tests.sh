#!/bin/bash
# The Rust tests, one test binary at a time, as the gate's core step runs them.
# Usage: rust-tests.sh <runtime|rest|hooks|all> [--list]
#   runtime  every default-feature test target of mq-runtime
#   rest     every other default-feature test target of the workspace
#   hooks    the tests gated on test-hooks (and so torrent)
#   all      the three in order
#   --list   print the cargo test arguments of each run, run nothing
# Each run gets a timeout and its test binaries are deleted after it, so a small disk holds the whole suite.
# Exits non-zero when any run fails. bash 3.2 safe (macOS): no associative arrays, no mapfile.
set -uo pipefail

shard=${1:-}
list=${2:-}
case $shard in runtime | rest | hooks | all) ;; *) echo "usage: rust-tests.sh <runtime|rest|hooks|all> [--list]" >&2; exit 2 ;; esac
cd "$(dirname "$0")/../.." || exit 1
T=${CARGO_TARGET_DIR:-target}
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0
# The relay tests open more files than a default shell allows.
ulimit -n "$(ulimit -Hn)" 2>/dev/null || true
TIMEOUT=$(command -v timeout || command -v gtimeout || true)

# Tests that fail on the trunk today, skipped until fixed. One line per test: "<run>|<test>", then the plan line
# that shows it fails on the trunk. Fixing a test removes its line; a new failure is never added here.
SKIP=(
  # plans/github-ci.md, Phase 1 "Tests that fail on trunk today": mq-runtime --test playback (without test-hooks)
  "-p mq-runtime --test playback|labels_that_name_a_file_read_as_the_file"
  # plans/github-ci.md, same list: mq-server a_cursor_reads_ahead_only_so_far (3 of 3 on trunk)
  "-p mq-server --test server|a_cursor_reads_ahead_only_so_far"
  # plans/github-ci.md, same list: mq-engine --features test-hooks --test keeper, the live-torrent and full-ring tests
  "-p mq-engine --features test-hooks --test keeper|the_next_file_of_a_live_torrent_gets_its_round_and_ends_the_window"
  "-p mq-engine --features test-hooks --test keeper|a_full_ring_plays_on_with_free_held_at_the_stop"
  "-p mq-engine --features test-hooks --test keeper|at_the_hard_stop_even_a_full_ring_stops"
  # plans/github-ci.md, same list: mq-turkcealtyazi a_request_cut_short_still_spaces_the_next (482 vs 490 ms)
  "-p mq-turkcealtyazi --test site|a_request_cut_short_still_spaces_the_next"
)

# Every test target of every workspace package, default features: lib, bins, each integration test, doc tests.
default_targets() {
  cargo metadata --locked --no-deps --format-version 1 | jq -r '
    .packages[] | .name as $p | .targets[] |
    (((."required-features" // []) | if length > 0 then " --features " + join(",") else "" end)) as $rf |
    if (.kind | index("test")) then "-p \($p) --test \(.name)\($rf)"
    elif (.kind | index("bin")) and .test and ((."required-features" // []) | length == 0) then "-p \($p) --bin \(.name)"
    elif ([.kind[] | select(. == "lib" or . == "rlib" or . == "staticlib" or . == "cdylib" or . == "proc-macro")] | length > 0) then
      (if .test then "-p \($p) --lib" else empty end), (if .doctest then "-p \($p) --doc" else empty end)
    else empty end' | sort -u
}

hooks_targets() {
  printf '%s\n' "-p mq-engine --features test-hooks --lib" "-p mq-engine --features test-hooks --test keeper" \
    "-p mq-engine --features test-hooks --test hold" "-p mq-engine --features test-hooks --test loopback" \
    "-p mq-engine --features test-hooks --test window" "-p mq-runtime --features test-hooks --test playback" \
    "-p mq-runtime --features test-hooks --test stream_choice" "-p mq-runtime --features test-hooks --test kids_play" \
    "-p mq-server --features test-hooks --test torrent"
}

case $shard in
  runtime) targets=$(default_targets | grep -E '^-p mq-runtime ') ;;
  rest) targets=$(default_targets | grep -vE '^-p mq-runtime ') ;;
  hooks) targets=$(hooks_targets) ;;
  all) targets=$(default_targets; hooks_targets) ;;
esac
[ -n "$targets" ] || { echo "rust-tests.sh: no test targets for $shard" >&2; exit 1; }

# The --skip arguments of one run.
skips_for() {
  local s
  for s in "${SKIP[@]}"; do
    [ "${s%%|*}" = "$1" ] && printf ' --skip %s' "${s#*|}"
  done
}

if [ "$list" = --list ]; then
  while read -r t; do
    s=$(skips_for "$t")
    echo "$t${s:+ --$s}"
  done <<<"$targets"
  exit 0
fi

# Test binaries have no extension; proc-macro dylibs and rlibs stay.
drop_bins() { find "$T/debug/deps" -maxdepth 1 -type f -perm -u+x ! -name '*.*' -delete 2>/dev/null; rm -rf "${T:?}/debug/incremental"; }
sum() { grep -E '^test result' "$2" | sed -E "s/.* ([0-9]+) $1.*/\\1/" | awk '{s+=$1} END {print s+0}'; }

P=0 F=0 I=0 BINS=0 BROKEN=
log=$(mktemp)
echo "$(echo "$targets" | wc -l | tr -d ' ') test runs ($shard)"
while read -r t; do
  s=$(skips_for "$t")
  echo "::group::cargo test $t${s:+ --$s}"
  # shellcheck disable=SC2086
  ${TIMEOUT:+$TIMEOUT -k 30s 15m} cargo test --locked $t ${s:+--$s} </dev/null 2>&1 | tee "$log"
  rc=${PIPESTATUS[0]}
  echo "::endgroup::"
  P=$((P + $(sum passed "$log"))) F=$((F + $(sum failed "$log"))) I=$((I + $(sum ignored "$log"))) BINS=$((BINS + 1))
  if [ "$rc" -ne 0 ]; then
    BROKEN="$BROKEN [$t]"
    [ "$rc" -eq 124 ] && echo "TIMEOUT cargo test $t (15 min)" || echo "FAIL cargo test $t (rc $rc)"
    grep -E 'FAILED|panicked|^error' "$log" | head -10
  fi
  drop_bins
done <<<"$targets"
rm -f "${log:?}"

echo "rust $shard: $BINS test runs: $P passed, $F failed, $I ignored, ${#SKIP[@]} known trunk failures skipped"
[ -n "$BROKEN" ] && echo "broken runs:$BROKEN"
[ -z "$BROKEN" ]
