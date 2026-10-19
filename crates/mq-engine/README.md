# mq-engine

Embedded torrent streaming engine for Marquee `full` builds: a thin fork of librqbit 9.0.1 (`vendor/librqbit`, Apache-2.0) for the protocol, with the DWH scheduler (`mq-engine-sched`) choosing every block of a streamed torrent. Plans: `plans/architecture.md` section 6, `research/10-sota-torrent-engines.md` sections 3, 6 and appendix A.

```rust
let engine = Engine::start(EngineConfig::new(data_dir, http.torrent_scope(&[]))).await?;
let t = engine.add_magnet(info_hash, &trackers, &[]).await?;        // metadata only
let file = engine.select_file(t, &FileHint::Episode { season: 1, episode: 2 }).await?;
let reader = engine.open_reader(t, file).await?;
let mut range = reader.range(start, Some(end)).await?;              // RangeOpened
range.ready().await?;                                               // DWH Ready
range.read(&mut buf).await?;                                        // verified bytes only
drop(range);                                                        // RangeClosed
engine.stats(t).await?; engine.remove(t, false).await?;
```

## What it does

- One librqbit session per process: shared DHT persisted in `data_dir/dht.json`, trackers, inbound TCP and uTP listener (`listen_port` or a free port), outgoing TCP with a uTP fallback, PEX, IPv6 dual stack; LSD and UPnP are off. Every peer, dialed or incoming over TCP or uTP, must pass `EngineConfig::policy` (mq-http's playback address rule, `plans/c27-torrent-policy.md`); refusals are counted per kind (`Engine::refused`), never logged with an address. Honest peer id `-MQ0100-` and client name `Marquee 0.1.0 (librqbit 9.0.1)`. `upload: false` stops serving pieces.
- `add_magnet` resolves metadata only (librqbit `list_only`, peers it met are kept); nothing downloads until `select_file`, which picks by index, filename, season and episode (through `mq-relparse`) or the largest video, and starts the torrent with only that file wanted.
- With DWH (the default) librqbit's piece picker is off for the torrent: the adapter (`src/adapter.rs`) turns librqbit's events into DWH `Event`s (`PeerConnected` with bitfield and BEP 10 `reqq`, `PeerHave`, choke, unchoke, disconnect, `BlockReceived`, `PieceVerified` / `PieceFailed`, a `PeerRate` sample per peer every second with an RTT estimate, `Tick` every 100 ms) and executes `Request`, `Cancel` and `Ready`. Ranges feed `RangeOpened` before the first body read, `RangeAdvanced` and `RangeClosed`.
- Readers wrap librqbit's `FileStream`, so they only ever return verified bytes. `RangeReader::ready()` resolves on DWH `Ready` (the range's first 1 MiB plus the file's last 2 MiB verified); stock mode uses the same rule.
- Disk: whole-torrent LRU over allocated bytes (`st_blocks`), torrents with open readers never evicted; DWH readahead is capped at a quarter of the budget. A torrent stays live 60 s after its last reader closes, then pauses (peers dropped) until the next reader.
- `EngineConfig::dwh = false` keeps librqbit's own picker and 32 MiB stream lookahead, for A/B runs through the same harness.

In the app, `mq-server` serves an engine file as `Upstream::Torrent` (206, HEAD, Content-Length, Content-Type from the name), and `PlaybackView` plays `Source::Torrent` streams through it (debrid first when an account is set, peer to peer when the store cannot give the torrent or no account exists), after metadata (30 s budget) and `Ready` (60 s). All of it sits behind the `torrent` feature of `mq-server`, `mq-runtime` and `mq-ffi`; store builds leave it off.

## Fork hook surface

All fork changes are listed in `vendor/librqbit/NOTICE`; the hook itself is `src/torrent_state/live/ext.rs` (`ExtSched { tx, drive }`, `ExtEvent`, `ext_request`, `ext_cancel`, `ext_have_pieces`) plus one-line calls at the event sites. Rebasing onto a new librqbit means re-applying about 120 changed lines in seven files.

## Deviations from the plan

| Plan | Here | Why |
|---|---|---|
| `add_magnet(info_hash, trackers)` | adds `peers: &[SocketAddr]`, and `add_torrent_bytes` | tests point a leecher at a loopback seeder, and the seeder needs a torrent file; trackers accept Stremio `sources` (`tracker:` stripped, `dht:` skipped) |
| hook DWH into librqbit's `piece_tracker` for stream-owned pieces only, the rest to the stock picker | in DWH mode DWH owns every request of the torrent; outside its windows nothing downloads | two pickers on one peer would each believe they own its request queue (DWH sizes and preempts it); with `only_files` set to the played file, DWH's playback and readahead windows are all the non-stream work there is. Matches "not downloaded when no stream is active" |
| `BlockRejected` / `BlockTimedOut` | not emitted | librqbit 9 has no BEP 6 reject, and drops a connection after 10 s without any message, which releases every request through `PeerDisconnected`. A REQUEST that cannot be sent (peer already gone) drops the peer from DWH at once |
| `reqq` arrives with `PeerConnected` | a `reqq` that comes after the bitfield re-announces the peer when nothing is queued yet (the usual case: before the first unchoke), else 250 stays | DWH has no reqq update event |
| RTT samples | smallest request-to-arrival time per peer (RTT plus one block) | librqbit measures no RTT |
| connect burst of 40 half-open (16 on 1 GB) | `peer_limit` 40 / 80 / 128 live-or-connecting peers by device class | librqbit has one limit for both |
| curated tracker list, persisted peers per info hash | not yet | magnets in the test set carry trackers; listed under open issues |
| `FileStream` as upstream | no blocking-spawner permit per stream (fork change) | upstream held one of 8 permits per open stream for its lifetime; 8 open ranges starved disk writes and every stream hung |

## Tests

`CARGO_TARGET_DIR=... cargo test -p mq-engine` runs the adapter unit tests (translation of every event kind, BEP 3 bit order, `reqq` before and after the bitfield, choke forgetting requests, RTT and rate samples, a failed send dropping the peer, stock-mode readiness, the fallback for unaligned pieces), the file picker, and `tests/loopback.rs`: a seeder engine and a leecher engine on 127.0.0.1 with discovery off, a two-file torrent whose pieces straddle the files, metadata over BEP 9, a seek then the whole file read back and compared byte for byte, once with DWH and once with the stock picker. `cargo test -p mq-server --features torrent` adds the same swarm through the loopback server (HEAD, 206 with `Content-Range`, the tail, the whole file, byte for byte). `tests/live.rs` holds ignored tests on the webtorrent.io swarms (`-- --ignored`).

## Measurements (real swarms, 2026-10-17)

Method: `mq play-magnet` (release build) adds the magnet, selects the largest video, opens a head range and serves the file through the loopback server; a GET at offset 0 runs beside the readiness wait; mpv 0.41 (`--vo=null --ao=null`) starts on Ready and seeks to 25, 50 and 75% over IPC, each timed from the command to the next `playback-restart`. `bench/torrent/run-real.sh` interleaves DWH and stock librqbit (ABAB, 5 pairs per torrent) with a fresh data folder per run; `bench/torrent/summarize.py` makes the table; raw records in `bench/torrent/real-*.jsonl`. One Mac (arm64, 12 cores, other builds running) on one residential link (5 to 9 MB/s observed aggregate download), legal torrents only. Five runs per cell on live swarms whose seeders change from run to run: the p90 column is the second worst run, so read it as a range, not a gate.

Seconds. Metadata from add; first byte and Ready from file selection; first frame from mpv's start; "add to frame" sums metadata, Ready, the probe and mpv's first frame. RSS is the engine process (engine, loopback server, probe), not mpv.

| torrent (pieces) | mode | metadata p50 / p90 | first byte p50 / p90 | Ready p50 / p90 | first frame p50 / p90 | add to frame p50 / p90 | seek p50 / p90 (15 seeks) | peak RSS MB p50 / p90 |
|---|---|---|---|---|---|---|---|---|
| Sintel, 129 MB MP4 (128 KiB) | DWH | 0.74 / 72.17 | 1.88 / 3.25 | **5.45 / 6.72** | 0.19 / 0.46 | **7.10** / 79.09 | 1.96 / 4.37 | 30.7 / 32.4 |
| | stock | 0.63 / 2.88 | 1.00 / 43.45 | 21.37 / 154.56 | 0.18 / 0.26 | 22.13 / 157.62 | 0.05 / 0.05 | 32.1 / 33.9 |
| Big Buck Bunny, 276 MB MP4 (256 KiB) | DWH | 3.49 / 6.96 | 4.69 / 19.19 | **6.49 / 25.97** | 0.16 / 0.26 | **12.59 / 29.60** | 1.85 / 3.03 | 28.5 / 30.1 |
| | stock | 3.03 / 23.23 | 1.93 / 2.94 | 49.26 / 67.29 | 0.21 / 0.27 | 59.54 / 72.76 | 0.04 / 0.17 | 31.6 / 34.6 |
| Tears of Steel, 571 MB WebM (512 KiB) | DWH | 0.50 / 0.59 | 3.21 / 3.84 | **4.46 / 7.71** | 0.14 / 0.16 | **5.08 / 7.14** | 4.26 / 7.82 | 28.9 / 30.2 |
| | stock | 0.50 / 70.64 | 0.86 / 9.09 | 101.89 / 117.22 | 0.20 / 0.40 | 102.58 / 188.03 | 0.05 / 0.05 | 28.4 / 30.3 |

Reading it honestly:

- **Start.** DWH reaches Ready (first 1 MiB and the last 2 MiB verified) 3.9x (Sintel), 7.6x (Big Buck Bunny) and 23x (Tears of Steel) sooner at p50. Stock librqbit fetches the file in order and the tail last, so its Ready waited for nearly the whole file (it downloaded 128, 275 and 570 MB per run against DWH's 37, 31 and 114 MB).
- **Seeks.** Stock's near-zero seeks are cache hits: by the time it was Ready it had the whole file. DWH started playing minutes earlier and its seeks land in data it has not fetched yet: 1.9 to 4.3 s p50 to the first frame after the seek, above the research/10 target (p50 < 1.5 s). The open issues below explain where the time goes; the Ubuntu runs below compare cold seeks in both modes.
- **Outliers are the swarm, not the scheduler.** The 72 s and 70 s metadata p90s are single runs where no peer answered ut_metadata for a minute (one DWH Sintel run, one stock Tears of Steel run); Big Buck Bunny's DWH p90 Ready of 26 s is a run with 3 connected peers at Ready.
- **First frame** after Ready is 0.1 to 0.5 s in both modes: mpv reads only verified, local bytes by then.
- **Waste.** DWH's duplicate payload was 9.9% (Big Buck Bunny), 10.8% (Sintel) and 5.5% (Tears of Steel) of the bytes it kept. Hedges are capped at 5%; the rest is blocks that peers sent after our CANCEL (see open issues). Stock was not instrumented for duplicates.
- **Memory.** About 30 MB peak RSS in both modes (research/10 measured 34 MB for stock rqbit).

Cold seeks, no player: Ubuntu 26.10 desktop ISO (6.6 GB) with `--bytes`, 5 pairs. Bytes through the loopback at the head and at 25, 50 and 75% (15 seeks per mode); neither mode can have the seek points before they are asked for. Stock has no Ready here (it fetches the 2 MiB tail last), so the byte probe does not wait for it.

| Ubuntu 26.10 ISO | metadata p50 / p90 | first byte p50 / p90 | head 4 MiB p50 | seek first byte p50 / p90 | seek 4 MiB p50 / p90 | Ready p50 / p90 | peak RSS MB p50 |
|---|---|---|---|---|---|---|---|
| DWH | 5.17 / 28.63 | 3.06 / 6.77 | 11.08 | 2.28 / 5.88 | 6.71 / 9.95 | 6.39 / 9.20 | 30.6 |
| stock | 14.21 / 67.80 | 0.76 / 6.05 | 6.83 | 1.21 / 13.06 | 10.37 / 19.89 | none in 180 s | 30.4 |

This one is mixed, and the numbers say why:

- Stock gets the **first byte** sooner (0.76 against 3.06 s at the head, 1.21 against 2.28 s after a seek). Stock gives a whole piece to the first peer that asks; DWH spreads the blocks of the first pieces over every peer, and before any rate sample it cannot tell a fast peer from a 10 KB/s one, so the first piece waits for its slowest block. DWH also spends its first seconds on the 2 MiB tail, which an ISO does not need (the head 4 MiB p50 is 11.1 against 6.8 s).
- DWH is better on **volume and the tail**: 4 MiB after a seek p50 6.7 against 10.4 s and p90 9.9 against 19.9 s, and seek first byte p90 5.9 against 13.1 s.
- Neither mode meets the research/10 seek gate (p50 < 1.5 s, p90 < 3 s) on this link; with 256 KiB pieces and 5 to 9 MB/s the physics allow well under a second, so this is scheduler work, not the link (open issues).

**The app path**, once: a local addon (a manifest plus `stream/movie/tt1727587.json` returning Sintel's info hash, trackers and `behaviorHints.filename`) installed into `mq --data <dir>`, then `mq play movie tt1727587 --headless` with no debrid key. VersionsView picked the torrent row, PlaybackView resolved it through the engine (metadata plus Ready) in 5.4 s, the probe read 726 KB through the loopback, and mpv showed the first frame 0.47 s after it started.

## Disk used while measuring

The engine target directory (`$HOME/.cargo-targets/marquee-engine`, release profile only during the runs) stayed at 1.3 GB; torrent data lived in a per-run temporary folder deleted after each run (largest: 571 MB for a stock Tears of Steel run).

## Open issues

- **Peers that ignore CANCEL.** Traces on the Big Buck Bunny swarm showed slow peers delivering every readahead block we cancelled at a seek, seconds later. The scheduler now ignores those stale blocks as proof of life (`mq-engine-sched` deviation table), which in back-to-back single runs took the first seek's first byte from 7.8 s to 1.4 s, but right after a seek DWH still sees those peers' queues as empty and gives them urgent blocks that sit behind the stale ones until the 1 s stall rule moves them. A per-peer "ignores CANCEL" flag that leaves such queues alone on a seek was tried and left out: a simulator run showed worse seeks (healthy 256 KiB seek p50 1.79 s against 0.69 s; that run used a target directory shared with other worktrees, so the number is indicative only), because the simulated peers honor CANCEL and the one or two in-flight blocks that still arrive were enough to flag them; it was never measured on a real swarm. Next step: decide it on the share of cancelled blocks that still arrive, not on a count.
- **Cold-start fan-out.** Before the first rate samples every peer has the same estimate, so the first urgent blocks spread round-robin over fast and very slow peers alike; Ready then waits on hedges and the stall release.
- **Size.** The engine adds 4.84 MB to the stripped arm64 `libmarquee_core.so` (10.40 to 15.23 MB, budget 12 MB; `bench/torrent/size.jsonl`). About 0.7 MB is aws-lc, a second crypto library that librqbit's `rust-tls` feature brings for reqwest TLS and SHA-1 next to the ring provider `mq-http` already ships; moving both to ring is the first cut. UPnP's XML and charset stack is about 0.3 MB.
- **Not yet built:** curated tracker list, persisted peers per info hash, next-episode preload, NAT-PMP, BEP 6 fast extension (reject messages), a PEX-based connect burst, configurable device class and budget from the apps (`Core` uses `Standard` and 4 GiB today).
- **Keep-warm pause and LRU eviction** are exercised only by hand; a test needs a clock seam in `maintenance`.
- **Hash failure** in drive mode disconnects the peer that delivered the last block (upstream behavior), although other peers may have sent the bad block.
- **Advisory ignore.** `deny.toml` ignores RUSTSEC-2026-0293 (ringbuf 0.4.8 through librqbit-utp 0.7.0): the bug needs an element whose `Drop` panics, and librqbit-utp's only ring buffer holds `u8`. Remove it when librqbit-utp moves to ringbuf 0.5.2.
