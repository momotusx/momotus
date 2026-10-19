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
- Against the section 5 rule (C27 medians within 20% and no more failures or timeouts), the Mac half passes for failures, timeouts, Ready and Sintel metadata, and misses on Big Buck Bunny's metadata (+24%) and both titles' peers at Ready.

## Android half (emulator-5584, the app's engine)

- Builds: debug APKs installed side by side (`app.marquee.android.c27pre` from main `5cac76c0`, `app.marquee.android.c27` from `feat/c27` `71b2fa4c`), the core built with `mq-ffi --features torrent` and, for these runs only, the engine's upload off (an uncommitted patch: the app core uploads by default). Uninstalled after the runs.
- Runner: scratchpad `live.ts cost`: per title and round the two apps alternately, the title page opened by its debug extra, Play pressed, the player's own marks read from logcat (`url ready` is when the core handed over the loopback URL, metadata and head verified; `first frame` when Media3 drew it), the torrent folder deleted after every run, media volume read as 0 before every playback. Same local addon for both apps, public trackers only. Rows: `bench/c27/android-cost.jsonl`.
- The constrained swarm: no free Blender title had under 10 seeders on the day (scrape on opentrackr: Sintel 206, Big Buck Bunny 334, Tears of Steel 58; explodie: 39, 81, 21), so Tears of Steel ran through explodie.org alone (21 seeders there) with the DHT.
- The host was loaded by other work (load average about 9) and the emulator shares it, so spreads are wide.

| Title | Build | runs ok | url ready (median) | first frame (median) | failures |
|---|---|---|---|---|---|
| Sintel | pre | 5 of 5 | 9.7 s | 10.1 s | 0 |
| Sintel | C27 | 5 of 5 | 14.1 s | 14.5 s | 0 |
| Big Buck Bunny | pre | 5 of 5 | 18.3 s | 18.9 s | 0 |
| Big Buck Bunny | C27 | 4 of 5 | 17.4 s | 17.7 s | 1 ("torrent did not start", trackers 0, peers 0 refused) |
| Tears of Steel, one tracker | pre | 2 of 5 | 26.6 s | 26.9 s | 3 ("None of the sources could be played") |
| Tears of Steel, one tracker | C27 | 4 of 5 | 23.5 s | 23.8 s | 1 ("torrent did not start", trackers 0, peers 0 refused) |

- Failures and timeouts: 3 for pre-C27, 2 for C27 over 15 runs each, so C27 has no more.
- Medians: Big Buck Bunny -6% and the constrained Tears of Steel -11% with C27; Sintel +44% (spread 6.5 to 20.8 s with C27, 7.3 to 18.0 s pre).
- C27's two failures refused nothing (both counts 0): the swarm did not answer in time, as pre-C27's three did.

## Decision input

Mac and Android together: C27 has no more failures or timeouts on either, Ready is faster on the Mac for both titles and on Android for two of three, and peers at Ready are lower with uTP and UPnP off. The 20% rule is missed on Mac Big Buck Bunny metadata (+24%), Mac peers at Ready, and Android Sintel (+44%) on a loaded host. Per plan section 5 that sends the call to the owner: keep both off, or bring uTP back through the dispatcher patch (3.4) as a follow-up, and decide on UPnP.
