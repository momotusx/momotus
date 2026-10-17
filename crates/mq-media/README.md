# mq-media

Container probe for remote media (architecture section 7.1).

- `probe(reader, budget, cancel)` reads MKV/WebM and MP4/MOV headers and returns a `StreamProbe`. Every field is a `Field<T>` with a `Provenance`; nothing fails hard. `budget_exhausted`, `cancelled` and `error` say why a probe stopped early.
- `RangeReader` is the one read interface (`len`, `read_at`). `HttpRangeReader` implements it without a HEAD request: the length comes from the `Content-Range` of the first ranged read, and an origin that answers 200 to a range is reported as `ProbeError::RangeUnsupported`.
- `ByteCache` is a bounded LRU of 64 KiB aligned blocks keyed by (source, offset). Unaligned and overlapping reads are assembled from blocks, so the loopback server can share it.
- `opensubtitles_hash` is the classic size plus first and last 64 KiB hash.

Default budget: 2 MiB and 1.5 s. Reads are whole blocks, so `bytes_read` counts what was actually fetched.

## What is read

- MKV: EBML header, Segment level 1 elements up to the first Cluster, then SeekHead entries (one nested SeekHead is followed). Once a SeekHead locates Tracks, the walk stops instead of paying a round trip to step over large Attachments. Info and Tracks are read whole; Cues are located through SeekHead and prefetched when they fit (the one tail read); Attachments come last, read whole when they fit the budget, otherwise only each file's name and type until the budget runs out.
- MP4: top level boxes until `moov`, so `moov` after `mdat` costs one tail read. `moov` is read whole when it fits; otherwise the walk skips sample tables by size and reads only `mvhd`, `tkhd`, `elst`, `mdhd`, `hdlr`, `stsd`, `stts` and the `stsz` header.
- Video: codec, profile, level and bit depth from avcC, hvcC or av1C; HDR from MKV Colour or MP4 `colr` transfer characteristics and from the Dolby Vision record (MKV BlockAdditionMapping `dvcC`/`dvvC`/`dvwC`, MP4 `dvcC`/`dvvC`/`dvwC` boxes); fps from MKV DefaultDuration, MP4 `stts` (or `stsz` count over `mdhd` duration), then hvcC avgFrameRate.
- Audio: channels from MKV Channels, MP4 sample entries, `dac3`, `dec3`; Atmos from the `dec3` JOC flag (container header) or "Atmos" in an MKV track name (parsed name).
- Subtitles: forced and hearing impaired from MKV flags, upgraded to parsed-name hints for "Forced" and "SDH".

Not covered yet: colour info inside the SPS VUI (files without container colour fields report HDR as unknown), HDR10+ (needs bitstream sampling), Cues with no SeekHead entry, fragmented MP4 durations (`mehd`), MP4 default and forced flags (unknown), VP9 `vpcC`.

## Tests

```
CARGO_TARGET_DIR=$HOME/.cargo-targets/marquee CARGO_INCREMENTAL=0 cargo test -p mq-media -j 4
```

Synthetic MKV and MP4 files are built in `tests/probe.rs` (HEVC 10-bit HDR10, Dolby Vision P8.1 and P7, E-AC-3 JOC, TrueHD, AAC, AC-3, ASS, PGS, SRT, tx3g, font attachments, Cues and `moov` at the tail), plus budget, time, cancel, HTTP and proptest fuzz cases.

## Live probes

```
CARGO_TARGET_DIR=$HOME/.cargo-targets/marquee CARGO_INCREMENTAL=0 cargo test -p mq-media --test live -- --ignored --nocapture --test-threads=1
```

Measured on 2026-10-17 from a home connection (Cloudflare edge IST), default budget, new HTTP client per file so the first request includes the TLS handshake. Elapsed is the range over four to five runs; it is dominated by origin latency, not bytes.

| File | Size | Bytes read | Elapsed | Result (matches ffprobe) |
|---|---|---|---|---|
| Blender `sintel_trailer-1080p.mp4`, `moov` after `mdat` | 14.6 MB | 138,088 | 307 to 1,022 ms | H.264 1920x1080 24 fps, AAC 2.0, 52.2 s |
| Blender `Sintel.2010.1080p.mkv` | 1.18 GB | 114,910 | 323 to 957 ms; one run of five hit 1.5 s before the Cues read | H.264 1920x818 24 fps, AC-3 5.1, 10 SRT, Cues at the tail |
| Jellyfin `jellyfish-10-mbps-hd-h264.mkv` | 37.4 MB | 80,410 | 927 to 1,465 ms; one run of five exhausted | H.264 1920x1080 29.97 fps |
| Jellyfin `jellyfish-3-mbps-hd-hevc-10bit.mkv` | 11.2 MB | 93,247 | 1,465 ms in one of three runs; the other two exhausted at 1.5 s (1,361 ms with a 10 s budget) | HEVC Main 10, level 4.0, 10-bit, HDR unknown |
| archive.org `big_buck_bunny_720p_surround.mp4` (302 redirect) | 61.9 MB | 393,216 | exhausted at 1.5 s in every run; 3,622 to 4,760 ms with a 10 s budget | H.264 640x360 24 fps, AAC 2.0, `moov` at head (347 KB, read whole) |
| test-videos.co.uk `Big_Buck_Bunny_1080_10s_30MB.mkv` | 29.5 MB | 65,536 | 122 to 349 ms | actually MP4: sniffed by content, not extension |
| OpenSubtitles `breakdance.avi` hash | 12.9 MB | 2 range reads | 452 to 1,910 ms | `8e245d9679d31e12`, the documented vector |

Origins with an 800 ms first byte (Jellyfin) or a redirect to a slow storage node (archive.org) do not fit two sequential round trips into 1.5 s. The probe then returns partial results with `budget_exhausted`, as designed; debrid CDNs are the target and answer faster.
