#!/bin/sh
# C27 phase 5 cost runs (plans/c27-torrent-policy.md 5): the pre-C27 and the
# C27 `mq` binaries alternately, DWH, headless mpv (no audio device), upload
# off, one JSONL record per run in bench/torrent/real-c27-<name>.jsonl.
#
# Usage: bench/c27/run-cost.sh <name> <rounds> <magnet>
# Needs MQ_PRE and MQ_C27 (the two release binaries).
#
# Every run starts from an empty data folder, deleted after the run (also
# when the 300 s timeout kills it).
set -eu

name=$1
rounds=$2
magnet=$3
out=$(dirname "$0")/../torrent/real-c27-$name.jsonl
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

for i in $(seq 1 "$rounds"); do
  for build in pre c27; do
    if [ "$build" = pre ]; then mq=$MQ_PRE; else mq=$MQ_C27; fi
    if /usr/bin/time -l timeout 300 "$mq" play-magnet "$magnet" --headless --no-upload \
      --data-dir "$tmp/data" >"$tmp/out" 2>"$tmp/err"; then
      status=ok
    else
      status=failed
    fi
    rm -rf "$tmp/data"
    rss=$(awk '/maximum resident set size/ {print $1}' "$tmp/err")
    last=$(tail -1 "$tmp/out")
    [ -n "$last" ] || last='{}'
    echo "$last" | jq -c \
      --arg t "$name" --arg build "$build" --arg status "$status" \
      --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
      --argjson run "$i" --argjson rss "${rss:-null}" \
      '. + {torrent: $t, build: $build, mode: "dwh", upload: false, status: $status, date: $date, run: $run, tree_max_rss_bytes: $rss}' \
      >>"$out"
    tail -1 "$out" | jq -c '{torrent, build, run, status, metadata_ms, ready_ms, peers_at_ready, first_frame_ms}'
  done
done
