# C27 cost of uTP, UPnP and LSD off (phase 5, Mac half)

Date: 2026-10-19. Machine: this Mac (macOS arm64), home broadband. Plan: `plans/c27-torrent-policy.md` section 5, "Cost of UPnP and uTP off".

## Setup

- Builds: `mq` release binaries, kept outside the repo; their target dirs were deleted after the copies.
  - pre-C27: main at `5cac76c0` (before C27 phase 2: uTP and UPnP on, no address policy) plus the `--no-upload` flag of `4f30496d`.
  - C27: `feat/c27` at `350199ec` (phases 1 to 4, the review fixes, `--no-upload`).
- Runner: `bench/c27/run-cost.sh`, the two builds alternately in each round, DWH, headless mpv (`--vo=null --ao=null`, no audio device), upload off (`--no-upload`), 5 s of playback, an empty data folder per run, deleted after it.
- Titles: Blender open movies only (CC BY), each with the same tracker list (UDP and HTTP opentrackr, UDP explodie, HTTPS nyacat) and the DHT.
- Rows: `bench/torrent/real-c27-sintel.jsonl`, `bench/torrent/real-c27-big-buck-bunny.jsonl` (`build` is `pre` or `c27`).

## Medians (5 runs per build and title)

| Title | Build | metadata_ms | ready_ms | peers_at_ready | first_frame_ms | failures, timeouts |
|---|---|---|---|---|---|---|
| Sintel | pre | 840 | 11,852 | 13 | 763 | 0, 0 |
| Sintel | C27 | 668 | 9,526 | 11 | 719 | 0, 0 |
| Big Buck Bunny | pre | 1,689 | 14,887 | 19 | 290 | 0, 0 |
| Big Buck Bunny | C27 | 2,086 | 9,783 | 8 | 327 | 0, 0 |

| Title | metadata | ready | peers at ready |
|---|---|---|---|
| Sintel | -20% | -20% | -15% |
| Big Buck Bunny | +24% | -34% | -58% |

## Reading

- No failure or timeout in either build.
- Time to Ready, the number a viewer waits for, is lower with C27 on both titles. Metadata is within 20% on Sintel and 24% (0.4 s) slower on Big Buck Bunny.
- Peers at Ready drop on both, sharply on Big Buck Bunny (19 to 8). Expected: with uTP off the engine reaches only TCP peers, and with UPnP off no peer behind NAT can dial in. Ready did not suffer in these runs.
- Spread is wide in both builds (single runs of 74.7 s pre and 18.6 s C27 to metadata on Sintel, 114 s C27 and 35.5 s pre to Ready), so 5 runs per build show direction, not a precise ratio.
- Against the section 5 rule (C27 medians within 20% and no more failures or timeouts), the Mac half passes for failures, timeouts, Ready and Sintel metadata, and misses on Big Buck Bunny's metadata (+24%) and both titles' peers at Ready. The Android half (emulator-5584, the app's engine, and one constrained swarm) is still to run; the owner decides on UPnP and the uTP dispatcher patch once both halves are in.
