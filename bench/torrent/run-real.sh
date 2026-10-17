#!/bin/sh
# Real-swarm runs of `mq play-magnet`, DWH and stock librqbit interleaved
# (ABAB), one JSONL record per run. `peak_rss_bytes` comes from the CLI itself
# (engine, loopback server, probe); `tree_max_rss_bytes` is what `time -l`
# reports for the process tree, which includes mpv and its demuxer cache.
#
# Usage: bench/torrent/run-real.sh <name> <runs> <magnet> [play-magnet args]
# Example: bench/torrent/run-real.sh sintel 5 "$SINTEL" --headless --seek
#
# Every run starts from an empty data folder, deleted after the run (also
# when the 300 s timeout kills it).
set -eu

MQ=${MQ:-$HOME/.cargo-targets/marquee/release/mq}
name=$1
runs=$2
magnet=$3
shift 3
out=$(dirname "$0")/real-$name.jsonl
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

for i in $(seq 1 "$runs"); do
  for mode in dwh stock; do
    flag=""
    [ "$mode" = stock ] && flag=--stock
    # shellcheck disable=SC2086
    if /usr/bin/time -l timeout 300 "$MQ" play-magnet "$magnet" $flag \
      --data-dir "$tmp/data" "$@" >"$tmp/out" 2>"$tmp/err"; then
      status=ok
    else
      status=failed
    fi
    rm -rf "$tmp/data"
    rss=$(awk '/maximum resident set size/ {print $1}' "$tmp/err")
    last=$(tail -1 "$tmp/out")
    [ -n "$last" ] || last='{}'
    echo "$last" | jq -c \
      --arg t "$name" --arg mode "$mode" --arg status "$status" \
      --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
      --argjson run "$i" --argjson rss "${rss:-null}" \
      '. + {torrent: $t, mode: $mode, status: $status, date: $date, run: $run, tree_max_rss_bytes: $rss}' \
      >>"$out"
    tail -1 "$out" | jq -c '{torrent, mode, run, status, metadata_ms, ttfb_ms, ready_ms, first_frame_ms, seeks, peak_rss_bytes}'
  done
done
