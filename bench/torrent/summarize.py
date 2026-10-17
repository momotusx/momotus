"""p50/p90 per torrent and mode from bench/torrent/real-*.jsonl.

Usage: uv run python -I bench/torrent/summarize.py bench/torrent/real-*.jsonl
Seconds. "first byte" and "ready" count from file selection (after
metadata), "first frame" from mpv's start; seeks pooled over every seek of
every run (mpv: command to the next frame; byte runs: to the first byte); nearest-rank
percentiles; a missing value (timeout, failed run) counts as missing, and the
n column says how many samples there were.
"""

import json
import math
import sys
from collections import defaultdict


def pct(values: list[float], p: float) -> str:
    if not values:
        return "n/a"
    v = sorted(values)
    return f"{v[max(0, math.ceil(p / 100 * len(v)) - 1)]:.2f}"


def seek_values(run: dict) -> list[float]:
    out = []
    for s in run.get("seeks") or []:
        # mpv seeks carry "ms"; byte seeks carry the first byte and 4 MiB.
        ms = s.get("ms", s.get("first_byte_ms"))
        if ms is not None:
            out.append(ms / 1000)
    return out


def main(paths: list[str]) -> None:
    rows: dict[tuple[str, str], list[dict]] = defaultdict(list)
    for path in paths:
        for line in open(path):
            run = json.loads(line)
            rows[(run["torrent"], run["mode"])].append(run)
    cols = ["metadata", "first byte", "ready", "first frame", "add to frame", "seek", "RSS MB"]
    print("| torrent | mode | runs ok | " + " | ".join(f"{c} p50 | p90" for c in cols) + " |")
    print("|---" * (3 + 2 * len(cols)) + "|")
    for (torrent, mode), runs in sorted(rows.items()):
        ok = [r for r in runs if r.get("status") == "ok"]
        get = lambda k: [r[k] / 1000 for r in ok if r.get(k) is not None]  # noqa: E731
        seeks = [s for r in ok for s in seek_values(r)]
        # Add to first frame: metadata, then Ready (from file selection), then
        # the probe and mpv's first frame (both after Ready).
        total = [
            (r["metadata_ms"] + r["ready_ms"] + (r.get("probe_ms") or 0) + r["first_frame_ms"])
            / 1000
            for r in ok
            if r.get("first_frame_ms") is not None
        ]
        cells = [
            get("metadata_ms"),
            get("ttfb_ms"),
            get("ready_ms"),
            get("first_frame_ms"),
            total,
            seeks,
            # Engine process only (MB), not mpv.
            [r["peak_rss_bytes"] / 1e6 for r in ok if r.get("peak_rss_bytes")],
        ]
        text = " | ".join(f"{pct(c, 50)} | {pct(c, 90)}" for c in cells)
        print(f"| {torrent} | {mode} | {len(ok)}/{len(runs)} | {text} |")


if __name__ == "__main__":
    main(sys.argv[1:])
