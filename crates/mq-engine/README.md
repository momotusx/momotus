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

## The cold first scrape (2026-10-20)

**Problem.** A fresh engine's first `Engine::scrape` sometimes answers nothing.
- In walk EW-1 and race 2, two of four reads kept Torrentio's numbers.
- The counts came about 3 s after the list, through the runtime's second ask (79b5ff5a).
- The runtime starts the engine inside that first scrape, and every app session starts cold. So the first title a user opens ranks on stale numbers.

**The wire on this Mac is not it.** A Python probe ran the scraper's stages: every tracker in parallel from one clock, `getaddrinfo` (what the policy resolver calls), the first address, one BEP 15 datagram per step, the BEP 48 HTTP scrape. Four free Blender hashes, cold then warm in one fresh process (`bench/m13/cold-scrape.jsonl`, `"source": "python probe"`).
- DNS: 4 to 24 ms. The macOS cache may already have been warm; it cannot be flushed without sudo.
- UDP connect: 70 to 340 ms. Scrape: another 70 to 340 ms.
- On the cold pass 6 of 8 default trackers answered within 1 s, opentrackr in 153 ms. exodus and explodie were silent for 3 s on the cold pass; on the warm pass exodus and demonii were silent and explodie answered.
- dler over HTTP took 0.6 s cold and 1.6 s warm.

So a cold engine on this Mac should answer within its budget. Where it does not, the loss is in our own path.

**Suspects in our path** (the measurement keeps or drops each):
1. One datagram per BEP 15 step, never sent again. One lost connect or scrape loses that tracker for the whole call.
2. The connect round trip is paid on every call, because connection ids are not kept. BEP 15: a client may reuse one for a minute.
3. One wall clock covers resolve, connect, scrape and HTTP, so a slow resolve eats the scrape's time.
4. Only the first resolved address is tried (`address`: `found.next()`). An AAAA answer first, on a network with no IPv6 route, is silence.
5. Engine start work at the same moment: session start and DHT bootstrap lookups and datagrams.

**Measurement** (cold engine, the CLI, free hashes only):
- `mq scrape <hash>...` with a fresh `--data` dir per run. It starts the engine with the production policy and discovery on, as the app does, and scrapes at once.
  - Per hash: the count and when it came.
  - Per tracker, from the scraper's own report of that call: resolve, connect and scrape round trips, HTTP time, and the stage the deadline cut.
- 5 cold runs.
- 5 runs that scrape 3 s after engine start (separates suspect 5).
- 5 runs on a resolver delayed by 300 ms in `mq-http` `test_dns` (a cold device resolver; test builds only).
- Rows go to `bench/m13/cold-scrape.jsonl`.

**Fix, chosen by those numbers** (candidates, smallest first):
- Send each BEP 15 datagram again at 250 and 500 ms inside the budget (suspect 1).
- Try every resolved address, IPv4 first (suspect 4).
- Keep each tracker's address for 5 minutes, re-checked by the policy on use, and its connection id for 60 s. A later scrape then costs one round trip (suspect 2).
- Warm the default trackers when a title's detail opens: resolve plus BEP 15 connect, through the same kept state. The runtime starts the engine inside the first scrape, so a warm-up at engine start alone runs at the same moment and buys nothing.
- Budgets per stage instead of one wall clock (suspect 3), only if resolves are slow in the numbers.

**Tests on loopback:**
- A tracker behind a resolver delayed 700 ms still counts within the budget once warmed.
- A kept connection id means the fake tracker sees one datagram per scrape, not two; an expired one connects again.
- A dropped first datagram is sent again and answered.
- An AAAA-only answer with no route falls back to an IPv4 address.
- Each has mutation rows in `bench/m13/mutations.jsonl`.

**Before and after:** the same cold CLI runs.

**Result (2026-10-20, `bench/m13/cold-scrape.jsonl`):** 10 rounds of `mq scrape` on four free hashes, the build order swapped after round 5. Every one of the 50 runs counted all four hashes.

| Build | Case | Median | Max | Runs over 300 ms |
|---|---|---|---|---|
| before | cold | 159 ms | 218 ms | 0 |
| before | late | 142 ms | 1001 ms | 1 |
| after | cold | 140 ms | 394 ms | 3 |
| after | late | 138 ms | 181 ms | 0 |
| after | warm | 65 ms | 73 ms | 0 |

"Cold" scrapes as the engine starts, "late" 3 s after it, "warm" 2 s after a warm-up from the start.

- **Suspect 1 is real.** In 4 runs a burst lost the first datagram to several trackers at once.
  - The "before" run lost opentrackr, torrent.eu.org and stealth for the whole call, ran its full second and kept only explodie's numbers.
  - The three "after" runs sent again at 250 ms and answered by 394 ms.
- **The others did not show on this Mac:**
  - Resolves took 3 to 17 ms (suspect 3), and no tracker returned an IPv6 address (suspect 4).
  - Cold and late scrapes cost the same (suspect 5).
  - A device with a slow resolver or more loss leans harder on the kept state and the warm-up. A cold miss without them looks like the before run above.
- **What was built:**
  - Resends, every address with IPv4 first, kept addresses and connection ids (fix(engine) 089dc8d9).
  - The warm-up when a title's page opens (feat(runtime)).
  - With the warm-up, a scrape costs one round trip: 65 ms.

Mutations SR1 to SR7.

## Review answers: engine round 1 (2026-10-20, plan)

`plans/reviews/engine-2026-10-20-r1.md`, 6/10. The order of work is the coordinator's: P1s, then P2, then the P3s marked "now".

**P1-1, nesting depth: fixed in the parser, once.**
- **Where.** Every bencode parse in the engine goes through `librqbit-bencode`'s `BencodeDeserializer`:
  - Scrape answers (`dyn_from_bytes`).
  - Announce answers (tracker-comms).
  - DHT messages, extended-protocol messages from peers, and `ut_metadata` info dicts (peer-protocol, core).
  - Its only recursion is `deserialize_seq` and `deserialize_map` (`deserialize_any`, `ignored_any` and the raw-value path all end there).
- **Fix.**
  - Vendor `librqbit-bencode` 9.0.1 like the other librqbit crates (`[patch.crates-io]`, NOTICE, provenance row).
  - Count depth in those two functions, and refuse past `MAX_DEPTH = 32` with a new `Error::TooDeep` before the visitor recurses.
- **Why 32 rather than 16.** The same parser reads torrent metadata. A BEP 52 file tree nests one dict per directory, depth 5 plus the directory depth, so 16 would refuse a v2 torrent 12 directories deep.
  - Trackers, scrapes, the DHT and peer messages need 3.
  - 32 frames stay far below the stack the review measured to overflow (10,000 levels on 2 MiB).
- **Tests** (10,000 and 100,000 levels, each must return an error and the test process must live):
  - The parser itself: `dyn_from_bytes` and a typed `from_bytes`, unit tests in mq-engine.
  - The scrape: an HTTP tracker on loopback answering the nested body.
  - The announce: the same tracker answering an announce; the engine survives and still adds the torrent.
  - The DHT: a datagram to a loopback DHT session; the session still answers a ping after it.
- **Mutation rows:** the depth check removed, and the bound raised past 100,000.

**P1-2, early settle on a zero: changed to the review's rule.**
- **Rule.** A hash settles early only on an answer with 2 or more seeders, or once every tracker asked for it has answered or failed. Otherwise the scrape waits out the 1 s budget and keeps the maximum, as `Engine::scrape`'s doc says.
- **Tracking.** A count of trackers still out per hash: a tracker's task ending, with counts or without, takes its hashes' counts down by one.
- **Test.** `a_scrape_ends_once_each_hash_has_its_own_answer` (it asserts the old rule) becomes a test of the new one, case by case:
  - An own 5 with a silent default ends early.
  - An own 0 with a silent default waits the budget and gives 0.
  - An own 0 with a default 7 ends early with 7.
  - An own 0 and a default 0 both answered ends early with 0.
  - An own 1 with a default that fails at once ends early with 1.
- **Mutation rows:** settle on any answer again, and never count a failure as an answer.

**P2, the fan-out: capped.**
- **Per hash:** at most 16 own trackers after dedupe (`Engine::scrape`).
- **Per call:** at most 24 trackers. The defaults always stay; the rest are the own trackers most hashes share, ties by first appearance. A hash whose own trackers all fall outside still goes to the defaults.
- **Failed resolves** are kept for 60 s, so a dead name is not resolved again on every call. That is a shorter keep than the 5 minutes a good answer gets, because a cold resolver can fail once.
- **`Reach::trackers`** is pruned of stale entries on every call.
- **Test:** 40 unique trackers named in the test resolver; the resolver sees at most 24 lookups, and a second call within 60 s sees none for the failed ones.
- **Mutation row:** the cap removed.

**P3, now:**
- **Kept tracker state after a network change.**
  - `Engine::network_changed` clears the scraper's kept addresses and ids, and `Core::on_network_changed` calls it when the engine runs.
  - A scrape whose kept addresses all fail resolves once more within the same call.
  - Test: a kept address that stops answering, and a fresh resolve that points elsewhere, still counts.
- **A refused SYN answered later.** `match_syn_with_accept` (vendored uTP) checks the source again, so a SYN cached while the streams were full is dropped if the policy refuses it by then. No deterministic test: it needs a full stream table and a policy refresh in between. The check sits next to the one in `on_recv` and goes through the same closure.
- **A claim on a removed torrent.** `claim` counts under the torrents lock, and `enforce_budget` checks claims and readers again under that lock before it removes. That is the order `remove` already uses. No deterministic test (a race window).
- **`container_index` overflow and stretched index.**
  - `checked_add` throughout.
  - An index past `mdat` larger than 64 MiB falls back to the fixed tail. That bounds the review's second `mdat` case, which the 4 KiB head cannot see.
  - Unit rows for a largesize near `u64::MAX` and a 1 GiB index.
- **clippy on mq-runtime alone, with and without the torrent feature**, after every change that touches it.

**P3, not in this turn (owner or other agent):**
- **`read_head` inline for up to 2 s.** The head is verified bytes on local disk, so a stall needs a stuck file read. It moves to a spawned task with the next adapter change.
- **Ready's fixed 60 s (or 30 s) deadline against a large `moov`.** That deadline is the runtime's (playback.rs), so the stream agent owns it. The engine can report index bytes arriving if the runtime wants to extend the deadline on progress.
- **The relabel scan for every client.** That is in mq-server, whose request does not say what the client is. Capping the prefetch breaks Apple's large `moov` relabel. The owner of the server path decides between a client hint and a cap.

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
