# mq-engine-sched

DWH streaming scheduler (deadline windows, block-level urgent fan-out, hedging) for the Marquee torrent engine. Specified in `research/10-sota-torrent-engines.md` section 6.2; this crate is the pure core that a torrent engine adapter (the librqbit fork, later) drives.

The scheduler is deterministic and has no async, no I/O and no clock. The adapter feeds `Event`s and executes the `Action`s it gets back. Time arrives only through `Event::Tick`, so the same event sequence always produces the same actions.

```rust
let geo = Geometry::new(piece_len, &file_lens).unwrap();
let mut s = Scheduler::new(geo, Config::standard()); // or low_ram(), high_end()
s.set_bitrate(file, file_len / duration_s);          // optional, else 10 Mbit/s
let mut out = Vec::new();
s.handle(Event::Tick { now_us }, &mut out);
s.handle(Event::RangeOpened { demand, file, start, end_hint: None }, &mut out);
for a in out.drain(..) {
    match a {
        Action::Request { peer, block } => { /* REQUEST geo.wire(block) to peer */ }
        Action::Cancel { peer, block } => { /* CANCEL geo.wire(block) to peer */ }
        Action::Ready { demand } => { /* start writing the HTTP body */ }
    }
}
```

## Adapter contract

- Send `RangeOpened` as soon as the HTTP `Range` header is parsed, before reading the body (range-before-read). `start` and `end_hint` (exclusive) are file-relative.
- Send `RangeAdvanced` as the body writer moves forward and `RangeClosed` when the response ends. `PlayPosition` is optional; when present, bytes the player already buffered stop being critical.
- Peers: `PeerConnected` (bitfield, advertised `reqq` or 250), `PeerHave`, `PeerChoked` / `PeerUnchoked`, `PeerDisconnected`, and a `PeerRate` sample about once a second (payload bytes per second, RTT).
- Block indexes count 16 KiB blocks from each piece's start, as librqbit counts chunks, so any piece length works: a piece that is not a multiple of 16 KiB (YTS uses 136 KiB) ends in a short block. Map a PIECE message to its block with `geo.block_at(piece, begin)`.
- Blocks: `BlockReceived` for every payload block (unrequested and duplicate ones too), `BlockRejected` / `BlockTimedOut` for requests the peer will not answer, `PieceVerified` / `PieceFailed` after hashing (also for pieces found on disk at start).
- `Tick` whenever the clock moves; periodic work (snubbing, hedging, stall release) runs at most every `tick_ms`.
- Peer acquisition (connect burst, DHT, trackers, uTP), disk and the HTTP server are the adapter's job (research/10 6.2 items 8 and 10).

## Algorithm

1. **Demands.** Every open range is a demand. The newest is focused and gets all windows; older readers keep only their blocking window. If the focused read starts in the last `max(tail, size/64)` of the file (an MP4 `moov` or MKV Cues read), the head's blocking and critical windows are merged in, because the player reads the head next. When the last reader closes, its playback and readahead windows stay as non-urgent work (anchor).
2. **Classes**, most urgent first: blocking (`blocking_bytes` from the reader, never less than the piece under it), tail (last 2 MiB of each opened file), critical (`max(4 MiB, 3 s x bitrate)` minus what the player already buffered), playback (30 s x bitrate), readahead (budget per device class). A piece takes its most urgent class.
3. **Deadlines.** Blocking and tail are due now; a critical piece is due at `now + (offset - play_pos) / bitrate - safety` (1 s). Urgent pieces are served in deadline order: the focused blocking window, then the tail, then older readers' blocking windows, then critical by offset.
4. **Block-level fan-out** for urgent pieces (libtorrent's time-critical rule): each free block goes to the eligible peer with the lowest ETA, `max(rtt, queued / rate) + block / rate`, skipping choked and snubbed peers and the slowest 10%, until the best peer's queue exceeds 2 s. Blocks of one piece spread over many peers. When the chosen peer is full, the urgent block takes the slot of its newest non-urgent request ("urgent jumps the queue"). The slowest peers still take urgent pieces that nobody else has, so nothing starves.
5. **Non-urgent fill**, per peer with free slots: playback by offset, then readahead rarest-first then offset (swarm friendly).
6. **Per-peer depth** `2 + ceil(rate x 1 s / 16 KiB)` clamped to `[4, min(reqq, 256)]` (128 on the 1 GB preset).
7. **Hedging** (Tail at Scale): an urgent block with one requester that will miss its deadline and whose ETA exceeds `max(750 ms, p90 block service time)` is also requested from the best other peer, if that peer would beat it. Duplicate request bytes in the last 10 s stay within `max(5% of payload, 256 KiB)`. First arrival wins; the other request is cancelled. Never more than two requests per block, and only for urgent blocks (a window change cancels stale duplicates).
8. **Stall release**: once the hedge budget is spent, a peer that sent nothing for over 1 s plus its RTT while we wait on it gives its urgent blocks back to the fan-out (cancel and reassign), if another eligible peer has the piece.
9. **Seek**: when the focused reader lands outside the previous urgent span, every queued request that is now readahead-only or unwanted is cancelled, so the per-peer FIFOs carry the new urgent blocks first.
10. **Snub**: no payload for 10 s while we wait: depth 1, all but the oldest request cancelled, no urgent blocks. The first payload lifts it. Peers are never disconnected for being slow.
11. **Readiness**: `Ready { demand }` once the demand's blocking window and its file's tail are verified (served bytes are always verified bytes).

## Deviations from research/10 6.2 (and why)

| Spec | Here | Why |
|---|---|---|
| blocking = bytes the reader waits on now | `blocking_bytes` (1 MiB) from the reader, at least the piece under it | `Ready` then means "the player can start" at every piece size; with one 256 KiB piece the signal would fire on 50 ms of 4K video |
| critical = next `max(4 MiB, 3 s x bitrate)` | same, minus bytes buffered past `PlayPosition` | a player with a full buffer should not make readahead urgent (and burn hedge budget) |
| peer choice by `outstanding_bytes / rate` | ETA `max(rtt, queued / rate) + block / rate`; the 2 s stop rule still uses `queued / rate` | libtorrent's key prefers an idle 10 KB/s peer (0 s) over a busy 5 MB/s peer (3 ms) |
| hedge when ETA > `max(750 ms, p90 block RTT)` | also requires the block to miss its deadline; p90 is of per-block service time (arrival minus the later of send and the previous arrival from that peer) | request latency includes our own deliberate 1 s queue, so the threshold tracked our queue depth rather than slow peers (in traces, the last blocks of a seek piece waited on a 129 KB/s peer unhedged); without the deadline test, steady playback would spend the budget on critical blocks needed seconds later |
| duplicate bytes capped at 5% of last-10 s payload | `max(5%, 256 KiB)` | at a cold start there is no payload yet, so 5% of it forbids hedging exactly when the head needs it |
| (not in spec) | stall release once the hedge budget is spent | nuvio's 1 s stall budget. In the simulator, a block stuck on a stalled peer after the budget was spent held healthy 256 KiB starts for up to 13 s. Paired A/B over 40 seeds per cell: healthy 256 KiB start mean -0.82 s, p90 7.6 s to 4.3 s; every other cell's mean start and seek time within +0.03 s to -0.11 s. Releasing stalled holders always (before hedging) made 4 MiB seeks worse (mean +0.38 s), so hedging stays first |
| (not in spec) | only a block the peer holds for us refreshes its last-payload time (stall and snub detection); stale blocks it sends anyway do not | measured on real swarms through `mq-engine`: peers often ignore CANCEL and keep sending cancelled readahead blocks for seconds. Counting those kept a peer that held an urgent block behind them from ever looking stalled, so neither the hedge nor the stall release moved the block (in one traced Big Buck Bunny run a seek waited 10.2 s on a single block; in the next run with the fix, single runs on a live swarm, the first seek's first byte came after 1.4 s instead of 7.8 s). Simulator, 200 seeds: unchanged within noise, healthy 256 KiB playable p99 6.67 to 5.50 s and seek p99 2.69 to 2.55 s |
| (not in spec) | a rate sample taken while the peer had nothing to send may raise the estimate, never lower it | libtorrent's peak-rate rule; otherwise an idle period after the readahead window fills collapses depth and ETAs right before the next seek |
| cancel non-urgent requests on the 8 fastest peers on a seek | on all peers; a cancelled block may be requested again behind the new urgent work | simpler, and a slow peer's stale queue is where an urgent block would wait longest |
| deadline order | (focused blocking, tail, other blocking, critical by offset); deadlines are materialized only for hedging | only the focused reader has a critical window, so offset order is deadline order |
| inputs as listed | adds `PeerDisconnected`, `reqq` in `PeerConnected`, and `Scheduler::set_bitrate` | churn and BEP 10 `reqq` need them; bitrate comes from the addon or the probe |

## Tests

`cargo test -p mq-engine-sched` runs unit tests, property tests and a simulator smoke test.

The property tests drive random streams of every event (peers with random bitfields and `reqq`, chokes, rates, opens, advances, closes, play positions, deliveries, corrupt pieces, rejects, timeouts, ticks) through a harness that plays the adapter and checks on every action:

- never request a block the peer lacks, from a choked peer, twice from one peer, or one we already have;
- never exceed the per-peer depth or `min(reqq, max_depth)`;
- at most two requests in flight per block, and a duplicate only for an urgent block;
- hedge bytes within `max(5% of payload, floor)` over the trailing window;
- the focused demand's blocking and critical pieces are urgent right after it opens;
- every request that is stale after a seek is cancelled in the same step;
- `Ready` only after the blocking window and the tail are verified, once per demand;
- the scheduler's queue view matches the adapter's;
- the same event sequence gives the same actions (determinism).

A coverage test checks that these streams actually reach hedges, snubs, seeks, cancels and readiness. The smoke test checks that in the simulator DWH's own `Ready` fires in the same step the simulated player can start, for the start and for every seek.

## Simulator

`examples/sim` is a discrete-event model of one client in a swarm:

- 5 ms steps; block transfers are fluid with max-min fair sharing of a 50 Mbit/s downlink; requests, cancels and blocks travel with each peer's one-way delay; the remote serves its request queue in FIFO order and drops cancelled requests that have not started.
- Peers: lognormal upload rate to us, uniform RTT, 70% (healthy) or 50% (sparse) seeders and the rest random partial bitfields that grow over time, connection arrivals after a cold start, unchoke delay, churn (mean lifetime 180 s, replacement after about 5 s), 15% flaky peers that re-choke every 10 s with probability 0.3, rate jitter 0.6 to 1.4 per second, and 1%/s per-peer stalls of 2 to 8 s.
- Profiles: `healthy` (25 to 50 peers, median 300 KB/s, RTT 20 to 250 ms) and `sparse` (4 to 8 peers, median 150 KB/s, RTT 50 to 400 ms).
- Torrent: a 37,000-byte `.nfo` then an 8 GiB, 2 h video (about 9.5 Mbit/s), so pieces straddle files.
- Player: reads the head until its first byte is servable, then the last 2 MiB (container index), then the head again; playable when 1 MiB at the head is servable (time to playable, from torrent add, including peer acquisition). It then plays at the bitrate with a 30 s buffer and seeks 5 times to uniform positions in 5% to 95%, 6 s of playback apart; seek-to-ready is the time until 1 MiB at the target is servable. The new connection opens before the old one closes. Caps: 120 s to start, 60 s per seek; capped runs count at the cap.
- All swarm randomness comes from per-seed and per-peer streams that do not depend on the scheduler, so every scheduler meets the same swarm.

Baselines (behavioral models written from research/10 and research/04, not from their code):

- `rqbit-like`, stock librqbit 9: one peer per whole piece, blocks from 0; FIFO window `min(128, reqq)`; steal from a 10x then 3x slower holder only once the stealer has an average piece time (so nothing is stolen in the first seconds); 32 MiB lookahead per stream, interleaved; first and last piece queued first, then the file in order.
- `stremio-like`, torrent-stream in server.js: sequential block reservations from each reader's position, newest first, then the whole file in the background; depth `round(45 r^4 + 5)` from the unchoked count; wires under 48 KiB/s kept off the 1 MiB critical range; hotswap of a critical block from a slow wire to a 2x faster one; a wire that answers nothing for 4 s is destroyed and usable again 5 s later; no tail prefetch, no cancels on a seek. 512 KiB virtual pieces are not modeled.

Limits: no TCP slow start or uTP, no hashing or disk time, no upload side, synthetic swarms. The numbers rank schedulers under identical conditions; they are not predictions for a real device. The spike in research/10 6.3 measures the real thing.

Reproduce (about 1 minute on 4 threads):

```sh
CARGO_TARGET_DIR=$HOME/.cargo-targets/marquee CARGO_INCREMENTAL=0 \
  cargo run --release -p mq-engine-sched --example sim -- --seeds 200 --threads 4
```

Per-run records go to `bench/torrent/sim-results.jsonl`.

## Results

Seconds, 200 seeded runs per row (3,600 runs), 5 seeks per run. "playable" is time from torrent add to the first 1 MiB plus the container tail servable, peer acquisition included. "cap" means the 120 s start or 60 s seek cap was hit (counted at the cap); "rebuffer" is stall time over playback time; "dup bytes" is payload received for blocks we already had.

| swarm | piece | scheduler | playable p50 | p90 | p99 | seek p50 | p90 | p99 | capped start / seeks | rebuffer | dup bytes |
|---|---|---|---|---|---|---|---|---|---|---|---|
| healthy | 256 KiB | dwh | 3.35 | 4.33 | 5.50 | 0.69 | 2.08 | 2.55 | 0 / 0 | 0.8% | 0.18% |
| healthy | 256 KiB | rqbit-like | 20.15 | 39.86 | 49.82 | 20.06 | 36.79 | 49.99 | 0 / 2 | 36.9% | 0.18% |
| healthy | 256 KiB | stremio-like | 6.70 | 13.54 | 26.75 | 1.51 | 4.64 | 5.42 | 0 / 0 | 9.4% | 0.03% |
| healthy | 4 MiB | dwh | 4.63 | 5.72 | 6.95 | 2.23 | 3.06 | 5.13 | 0 / 0 | 0.4% | 0.38% |
| healthy | 4 MiB | rqbit-like | 64.47 | 101.98 | cap 120 | 32.71 | 58.76 | cap 60 | 10 / 89 | 17.4% | 0.01% |
| healthy | 4 MiB | stremio-like | 11.76 | 19.61 | 29.57 | 2.48 | 5.43 | 6.53 | 0 / 0 | 5.6% | 0.03% |
| healthy | 16 MiB | dwh | 8.27 | 9.25 | 10.99 | 4.14 | 6.26 | 8.32 | 0 / 0 | 2.0% | 0.57% |
| healthy | 16 MiB | rqbit-like | cap 120 | cap 120 | cap 120 | cap 60 | cap 60 | cap 60 | 177 / 63 | 9.2% | 0.01% |
| healthy | 16 MiB | stremio-like | 15.43 | 24.59 | 45.70 | 5.50 | 7.89 | 10.40 | 0 / 0 | 4.1% | 0.04% |
| sparse | 256 KiB | dwh | 10.96 | 16.03 | 26.57 | 2.56 | 5.88 | 11.86 | 0 / 2 | 22.7% | 0.33% |
| sparse | 256 KiB | rqbit-like | 31.93 | 65.47 | 104.53 | 23.43 | 47.25 | cap 60 | 0 / 33 | 40.3% | 0.21% |
| sparse | 256 KiB | stremio-like | 16.77 | 29.17 | 45.37 | 7.79 | 14.05 | 33.70 | 0 / 4 | 35.2% | 0.04% |
| sparse | 4 MiB | dwh | 15.85 | 30.07 | 54.07 | 7.25 | 17.63 | 42.10 | 0 / 5 | 27.9% | 0.43% |
| sparse | 4 MiB | rqbit-like | 71.64 | cap 120 | cap 120 | 32.96 | cap 60 | cap 60 | 31 / 166 | 29.5% | 0.02% |
| sparse | 4 MiB | stremio-like | 29.92 | 52.28 | 114.74 | 12.80 | 25.45 | 57.70 | 1 / 8 | 29.4% | 0.04% |
| sparse | 16 MiB | dwh | 46.03 | 93.06 | cap 120 | 21.11 | 42.40 | cap 60 | 6 / 30 | 11.5% | 0.19% |
| sparse | 16 MiB | rqbit-like | cap 120 | cap 120 | cap 120 | cap 60 | cap 60 | cap 60 | 185 / 51 | 8.2% | 0.02% |
| sparse | 16 MiB | stremio-like | 61.63 | cap 120 | cap 120 | 26.21 | 48.66 | cap 60 | 21 / 47 | 11.3% | 0.06% |

Reading it:

- DWH is fastest in every cell and percentile, or tied where every scheduler hit the cap (sparse 16 MiB p99). Healthy swarm against the Stremio-like baseline: playable p90 is 68% lower at 256 KiB (4.33 vs 13.54 s), 71% at 4 MiB and 62% at 16 MiB; seek p90 is 55%, 44% and 21% lower. That clears the research/10 6.3 bar of "at least 30% better p90" everywhere except 16 MiB seeks.
- 16 MiB pieces are where block-level fan-out matters most: whole-piece ownership (rqbit-like) never became playable in 177 of 200 healthy runs, while DWH's head piece arrives at close to link speed (p50 8.3 s for 32 MiB of head plus tail, peer ramp-up included).
- DWH's duplicate bytes stay at 0.2% to 0.6% of payload, far under the 5% budget.
- The sparse profile is capacity-bound (aggregate peer upload is close to the 9.5 Mbit/s bitrate, and few peers hold each piece), so every scheduler rebuffers there; DWH still starts and seeks fastest.
- The rqbit-like model is probably pessimistic: with 25 to 50 peers sharing the downlink, each 128-block FIFO is 10 s or more deep, so a seek waits behind it. The one real data point, the rqbit PR #702 baseline on a real 6.5 GB swarm, is seek first-byte p90 7.2 s. Treat its rows as the shape of the problem (FIFO depth, whole-piece ownership, no steal before a peer's first completed piece), not as a measurement.
