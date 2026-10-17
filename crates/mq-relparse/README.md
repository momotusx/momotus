# mq-relparse

Turns torrent and release names, and the text Stremio addons put in a stream entry, into
structured facts. Part of Marquee (GPL-3.0-or-later). Rust, edition 2024.

```rust
use mq_relparse::{parse, parse_addon_stream, Resolution};

let r = parse("Show.Name.S01E02.2160p.WEB-DL.DDP5.1.Atmos.HDR10+.DV.H.265-GRP.mkv");
assert_eq!(r.title, "Show Name");
assert_eq!(r.resolution, Resolution::R2160p);
assert_eq!((r.seasons, r.episodes), (vec![1], vec![2]));

let f = parse_addon_stream(
    "[RD+] Torrentio\n4k HDR",                                  // name
    "Movie.2020.2160p.BluRay.x265-GRP\n👤 92 💾 54.3 GB ⚙️ 1337x", // description (or title)
    Some("Movie.2020.2160p.BluRay.x265-GRP.mkv"),                // behaviorHints.filename
);
assert_eq!(f.seeders, Some(92));
assert_eq!(f.service.as_deref(), Some("realdebrid"));
assert_eq!(f.cached, Some(true));
```

Public API (stable): `parse`, `parse_addon_stream`, `warm_up`, and the types `ParsedRelease`,
`AddonStreamFacts`, `Resolution`, `Quality`, `VideoCodec`, `Hdr`, `Audio`. All result types derive
`Debug, Clone, PartialEq` and `Serialize, Deserialize`; `ParsedRelease` and `AddonStreamFacts` also
derive `Default`, and the other types are plain enums.

`warm_up()` builds the handler table and compiles the common patterns (about 0.5 to 0.7 s on a busy
M4 Pro). Call it from a background thread at startup; it is optional, the first `parse` does the
same work otherwise.

## Result fields

`ParsedRelease` (booleans are flat fields, not a nested struct):

| Field | Type | Notes |
|---|---|---|
| `title` | `String` | cleaned title, empty when the text holds none |
| `year` | `Option<u16>` | |
| `resolution` | `Resolution` | `R480p R576p R720p R1080p R1440p R2160p R4320p Unknown`; `720i` counts as `R720p`, `WxH` and odd heights go to the nearest step |
| `quality` | `Quality` | `Cam Telesync Screener Dvd Hdtv WebRip WebDl BluRay Remux Unknown` |
| `codec` | `VideoCodec` | `H264 H265 Av1 Vp9 Xvid Mpeg2 Vc1 Unknown` |
| `bit_depth` | `Option<u8>` | 8, 10 or 12 |
| `hdr` | `Vec<Hdr>` | `Hdr10 Hdr10Plus DolbyVision Hlg Sdr` |
| `audio` | `Vec<Audio>` | `Aac Ac3 Eac3 Dts DtsHd DtsHdMa DtsX TrueHd Atmos Flac Opus Mp3 Pcm`, in text order |
| `channels` | `Option<String>` | best layout seen: `7.1`, `5.1`, `2.0`, `stereo`, `mono` |
| `languages` | `Vec<String>` | ISO 639-1 codes, in detection order |
| `seasons`, `episodes` | `Vec<u32>` | ranges are expanded |
| `complete` | `bool` | season pack, full series or collection |
| `group` | `Option<String>` | release group |
| `container` | `Option<String>` | lowercase `mkv`, `mp4`, `avi`, ...; falls back to the file extension |
| `remux proper repack extended hardcoded_subs dubbed three_d upscaled` | `bool` | |

Mapping decisions worth knowing: PTT "HDRip" is `WebRip`, "R5" is `Screener`, "TeleCine" is
`Telesync`, "BRRip/BDRip/UHDRip" are `BluRay`; PTT's "DTS Lossless" splits into `DtsHdMa` and
`DtsX`, "DTS Lossy" into `Dts` and `DtsHd`; PTT's "la" (Latino) becomes `es`; plain "HDR" is
`Hdr10`. `quality = Remux` implies `remux = true`.

`AddonStreamFacts { release, seeders, size_bytes, source_site, cached, service }`. Sizes use
1024-based units (`GB` means GiB, as the addons print them). `service` is a debrid id such as
`realdebrid`, `alldebrid`, `premiumize`, `debridlink`, `torbox`. `cached` is `Some` only when the
name carries a marker (`[RD+]`, `[RD download]`, `[TB⚡]`, `MediaFusion 🧲 RD 1080P ⏳`).

## How it works

* `src/rules.rs` is PTT's handler table (about 430 regex rules, same order) and `src/engine.rs`
  its main loop: rules match against a shrinking title, matched text is removed, the earliest
  surviving match decides where the title ends. `src/custom.rs` holds PTT's function handlers,
  `src/title.rs` its title clean-up, `src/transform.rs` and `src/date.rs` its transformers.
* Only the linear-time `regex` crate. PTT's patterns use look-arounds and one back-reference, so
  each is expressed as an explicit guard: a regex tested on the text before or after a position
  (`Look`), a trailing look-ahead consumed as a last capture group and cut off (`la`), several
  alternatives with their own guards (`alt`), or a small `check` function (date separators).
  Retrying after a failed guard moves the start by one char, so worst-case work is bounded; input
  is cut to 2048 bytes first.
* `src/gate.rs` makes it fast. A `RegexSet` (in shards of 64 rules) built from relaxed copies of
  every pattern says which rules can match the current title; all others are skipped. Relaxed
  means a superset (no word boundaries, ASCII-sized stand-ins for Unicode classes, guard text
  matched as context), and the gate is re-scanned whenever the title changes, so skipping is
  exact. `gate_never_changes_a_result` checks this against the ungated loop.
* Three deliberate changes to PTT: `RM4K` (restored master, a 1080p encode) is not 2160p;
  `VC-1`, `VP9` and `HLG` are recognised (PTT has no rule for them).
* Addon text: `src/addon.rs`. Per-field pattern lists (data at the top of the file) for seeders,
  size, source site and debrid tags; the release name comes from `behaviorHints.filename` or the
  first non-metric description line, and the remaining strings (other description lines, the name
  after the addon label, flag emoji) only fill gaps. Comet's "DTS Lossless/Lossy" are mapped.

## Tests and data

| What | Where | Result |
|---|---|---|
| PTT golden cases | `testdata/ptt_cases.json` (1020 cases from 34 PTT test files, converted with `ast`, nothing executed), `src/golden.rs` | **3654 of 3654 field assertions (100%)**, 22 fields, 141 cases also check that unlisted fields are absent |
| Agreement with upstream PTT | `RELPARSE_ORACLE=<file> cargo test -p mq-relparse --lib oracle_agreement` (dev aid, needs PTT run through `uv`) | 152,465 of 152,498 field checks (99.98%) over 7,588 distinct strings from the corpus; every difference is one of the three deliberate changes above |
| Harvested corpus | `testdata/harvested/*.jsonl`, 5,676 raw `name`/`description`/`filename` records, 1.6 MB | see below |
| Snapshots | `tests/snapshots.rs` (insta), every 9th record of each addon | regenerate with `INSTA_UPDATE=always` |
| Extractor coverage | `tests/extract.rs` | Torrentio and TorrentsDB: seeders, size, site on 100% of records; Comet: size 99.5% (the rest print `None` or bytes), site 100%, seeders on 44% (Comet prints them only when it counted any) |
| Never panics, never slow | `tests/props.rs` (proptest, hostile inputs) | worst case 15 ms for 2 KB of Cyrillic, others under 2 ms |
| Gate exactness | `src/gate.rs` test | gated and ungated parse agree on golden inputs and a slice of the corpus |

Corpus provenance: fetched on 2026-10-17 with `curl` from `torrentio.strem.fun`,
`torrentsdb.com` and `comet.feels.legal` for 30 movies (IMDb ids), 30 series episodes and 6
Kitsu anime episodes, up to 30 streams per request, duplicates dropped, only strings kept (no
infoHash, no trackers). The public ElfHosted Jackettio and AIOStreams instances answer with a
"please configure" stream and MediaFusion's returns none, so those layouts are covered by
hand-written tests that follow their documented formats (`addon::tests`), not by harvested data.
Debrid tags such as `[RD+]` are likewise tested on hand-written names (a real one needs a key).

## Speed and memory

Release build, Apple M4 Pro (12 cores, shared with other builds during the runs), 11,282 release
strings from the corpus (mean length 65 bytes), whole-corpus passes after one warm-up pass:

```
cargo test -p mq-relparse --release --test bench -- --ignored --nocapture
mean per title: best pass 23.1 us, median pass 29.6 us   (target: under 50 us)
p50 16.2 us, p90 44.0 us, p99 123.3 us (best pass)
```

An ungated run (every rule on every title) measured about 100 us per title on the same strings.
Run on its own, the ASCII part of the corpus (90%) averages 18.8 us and the part with non-ASCII
text (10%) 38.0 us, because Unicode word boundaries keep `regex` off its fastest engine.

Memory and start-up are the soft spot: building the table takes about 0.5 to 0.7 s (busy
machine) and adds about 13 MB; after every rule has run over a diverse corpus the process has
grown by about 60 MB (7 MB to 70 MB resident). Most of it is the compiled Unicode classes (`\w`,
`\W`, `\d` cost 60 to 100 KB each per regex) and the lazy DFA caches, which `regex` keeps per
thread. The gate's cache is capped by `GATE_DFA_MB` in `src/gate.rs` (4 MB per shard; at 2 MB the
mean rises to about 38 us and memory drops by about 9 MB). Call `parse` from few threads.

## Known gaps

* `Resolution` has no 240p or 360p step, those parse as `Unknown`; odd heights such as 800p
  round to the nearest step.
* Latino Spanish and Spanish both come out as `es`; Chinese variants are all `zh`.
* Not exposed although PTT computes them (they still shape the title): date, site, network,
  region, extras, edition (only `extended` is exposed), unrated, volumes, episode code, scene,
  trash. PTT's adult keyword list, `anime.py` handlers and `translate_languages` are not ported.
* PTT validates dates with `arrow`; `src/date.rs` imitates its strictness for the formats PTT
  uses, which affects only whether a date-like token is removed from the title.
* A look-around retry moves one char at a time, so guarded rules cost O(n) extra searches on
  hostile input; the 2048-byte cut keeps that bounded.
* The memory and start-up figures above. A cheaper design is possible (ASCII-only automata for
  ASCII titles, Unicode ones only when needed) and was not done here.
* AIOStreams, whose output is user-templated, is only covered through the generic emoji fields
  (`👤`, `🌱`, `💾`, `📦`, `🔍`) and `[XX+]` or `[XX⚡]` service tags.

## Commands

```
CARGO_TARGET_DIR=$HOME/.cargo-targets/marquee CARGO_INCREMENTAL=0 cargo test -p mq-relparse -j 4
CARGO_TARGET_DIR=$HOME/.cargo-targets/marquee CARGO_INCREMENTAL=0 cargo clippy -p mq-relparse --all-targets -j 4 -- -D warnings
```

## NOTICE

The handler table, transformers and title clean-up in this crate are a Rust port of PTT
(parse-torrent-title for Python, https://github.com/dreulavelle/PTT, MIT), written from reading its
source; the golden cases in `testdata/ptt_cases.json` are converted from PTT's test files. PTT is
itself a port of `parse-torrent-title` by TheBeastLT (https://github.com/TheBeastLT/parse-torrent-title,
MIT, derived from Clément Escolano's original). Their licenses follow.

```
MIT License

Copyright (c) 2024 Spoked

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

```
MIT License

Copyright (c) 2018 Clément Escolano

(permission notice and warranty disclaimer identical to the text above)
```

The stream-text layouts handled in `src/addon.rs` were checked against live responses of Torrentio,
TorrentsDB and Comet, and against MediaFusion's description builder (MIT) for its layout.
