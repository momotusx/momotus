//! Synthetic MKV and MP4 files built with minimal EBML and box writers,
//! probed through an in-memory reader and a local HTTP server.

use bytes::Bytes;
use futures_util::future::BoxFuture;
use mq_http::{ClientConfig, HttpClient};
use mq_media::*;
use proptest::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

// ---------------------------------------------------------------- readers

struct Mem {
    data: Bytes,
    delay: Duration,
    fail: bool,
    /// Offsets of every `read_at`.
    log: std::sync::Mutex<Vec<u64>>,
}

fn mem(data: &[u8]) -> Mem {
    Mem {
        data: Bytes::copy_from_slice(data),
        delay: Duration::ZERO,
        fail: false,
        log: Default::default(),
    }
}

impl RangeReader for Mem {
    fn len(&self) -> BoxFuture<'_, Result<u64, ProbeError>> {
        Box::pin(async move { Ok(self.data.len() as u64) })
    }

    fn read_at(&self, offset: u64, len: u64) -> BoxFuture<'_, Result<Bytes, ProbeError>> {
        Box::pin(async move {
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            if self.fail {
                return Err(ProbeError::Http("boom".into()));
            }
            self.log.lock().unwrap().push(offset);
            let start = (offset as usize).min(self.data.len());
            let end = start.saturating_add(len as usize).min(self.data.len());
            Ok(self.data.slice(start..end))
        })
    }
}

async fn run(reader: &dyn RangeReader) -> StreamProbe {
    probe(reader, ProbeBudget::default(), &CancellationToken::new()).await
}

fn val<T: Clone>(f: &Field<T>) -> T {
    f.value.clone().expect("field should be known")
}

// ---------------------------------------------------------------- EBML writer

const EBML: u32 = 0x1A45_DFA3;
const SEGMENT: u32 = 0x1853_8067;
const SEEK_HEAD: u32 = 0x114D_9B74;
const INFO: u32 = 0x1549_A966;
const TRACKS: u32 = 0x1654_AE6B;
const ATTACHMENTS: u32 = 0x1941_A469;
const CUES: u32 = 0x1C53_BB6B;
const CLUSTER: u32 = 0x1F43_B675;

fn id_bytes(id: u32) -> Vec<u8> {
    id.to_be_bytes()
        .into_iter()
        .skip_while(|b| *b == 0)
        .collect()
}

/// Element with an 8-byte size, valid for any EBMLMaxSizeLength of 8.
fn el(id: u32, body: &[u8]) -> Vec<u8> {
    let mut v = id_bytes(id);
    v.push(0x01);
    v.extend_from_slice(&(body.len() as u64).to_be_bytes()[1..]);
    v.extend_from_slice(body);
    v
}

fn eu(id: u32, x: u64) -> Vec<u8> {
    el(id, &x.to_be_bytes())
}

fn es(id: u32, x: &str) -> Vec<u8> {
    el(id, x.as_bytes())
}

fn em(id: u32, kids: &[Vec<u8>]) -> Vec<u8> {
    el(id, &kids.concat())
}

/// Segment layout: [SeekHead], Info, Tracks, [Attachments], Cluster, [Cues].
fn mkv(
    info: Vec<u8>,
    tracks: Vec<u8>,
    attachments: Option<Vec<u8>>,
    cluster_len: usize,
    seek_head: bool,
) -> Vec<u8> {
    let header = em(EBML, &[eu(0x4286, 1), es(0x4282, "matroska")]);
    let cluster = em(CLUSTER, &[eu(0xE7, 0), el(0xA3, &vec![0x55; cluster_len])]);
    let mut body = vec![info, tracks];
    if let Some(a) = attachments {
        body.push(a);
    }
    body.push(cluster);
    let seek = |entries: &[(u32, u64)]| {
        let kids: Vec<_> = entries
            .iter()
            .map(|(id, pos)| em(0x4DBB, &[el(0x53AB, &id_bytes(*id)), eu(0x53AC, *pos)]))
            .collect();
        em(SEEK_HEAD, &kids)
    };
    let mut ids: Vec<u32> = body[..body.len() - 1]
        .iter()
        .map(|e| u32::from_be_bytes(e[..4].try_into().unwrap()))
        .collect();
    if seek_head {
        ids.push(CUES);
    }
    let head_len = if seek_head {
        seek(&ids.iter().map(|id| (*id, 0)).collect::<Vec<_>>()).len()
    } else {
        0
    };
    let mut positions = Vec::new();
    let mut pos = head_len as u64;
    for e in &body {
        positions.push(pos);
        pos += e.len() as u64;
    }
    let cues_pos = pos;
    let cues = em(
        CUES,
        &[em(
            0xBB,
            &[
                eu(0xB3, 0),
                em(0xB7, &[eu(0xF7, 1), eu(0xF1, positions[body.len() - 1])]),
            ],
        )],
    );
    let mut seg = Vec::new();
    if seek_head {
        let mut entries: Vec<(u32, u64)> = ids[..ids.len() - 1]
            .iter()
            .copied()
            .zip(positions.iter().copied())
            .collect();
        entries.push((CUES, cues_pos));
        seg.extend(seek(&entries));
    }
    seg.extend(body.concat());
    seg.extend(cues);
    [header, el(SEGMENT, &seg)].concat()
}

fn hvcc(profile: u8, level: u8, bit_depth: u8, avg_fps_x256: u16) -> Vec<u8> {
    let mut c = vec![
        1, profile, 0x60, 0, 0, 0, 0x90, 0, 0, 0, 0, 0, level, 0xF0, 0, 0xFC,
    ];
    c.push(0xFD);
    c.push(0xF8 | (bit_depth - 8));
    c.push(0xF8 | (bit_depth - 8));
    c.extend_from_slice(&avg_fps_x256.to_be_bytes());
    c.extend_from_slice(&[0x0F, 0]);
    c
}

/// DOVIDecoderConfigurationRecord, 24 bytes.
fn dovi(profile: u8, level: u8, el: bool, compat: u8) -> Vec<u8> {
    let mut c = vec![
        1,
        0,
        profile << 1 | level >> 5,
        (level & 0x1F) << 3 | 1 << 2 | u8::from(el) << 1 | 1,
        compat << 4,
    ];
    c.resize(24, 0);
    c
}

fn mkv_track(kind: u64, codec: &str, extra: Vec<Vec<u8>>) -> Vec<u8> {
    let mut kids = vec![eu(0xD7, 1), eu(0x83, kind), es(0x86, codec)];
    kids.extend(extra);
    em(0xAE, &kids)
}

fn audio_ch(n: u64) -> Vec<u8> {
    em(0xE1, &[el(0xB5, &48000f64.to_be_bytes()), eu(0x9F, n)])
}

fn mkv_hdr10() -> Vec<u8> {
    let info = em(
        INFO,
        &[
            eu(0x2A_D7B1, 1_000_000),
            el(0x4489, &5_400_000f64.to_be_bytes()),
        ],
    );
    let video = mkv_track(
        1,
        "V_MPEGH/ISO/HEVC",
        vec![
            el(0x63A2, &hvcc(2, 153, 10, 0)),
            eu(0x23_E383, 41_708_333),
            em(
                0xE0,
                &[
                    eu(0xB0, 3840),
                    eu(0xBA, 2160),
                    em(0x55B0, &[eu(0x55B2, 10), eu(0x55BA, 16), eu(0x55BB, 9)]),
                ],
            ),
        ],
    );
    let tracks = em(
        TRACKS,
        &[
            video,
            mkv_track(
                2,
                "A_EAC3",
                vec![
                    es(0x536E, "English DDP 5.1 Atmos"),
                    es(0x22_B59C, "eng"),
                    audio_ch(6),
                ],
            ),
            mkv_track(
                2,
                "A_TRUEHD",
                vec![
                    es(0x536E, "TrueHD 7.1"),
                    es(0x22_B59C, "eng"),
                    es(0x22_B59D, "en-US"),
                    eu(0x88, 0),
                    audio_ch(8),
                ],
            ),
            mkv_track(
                2,
                "A_AAC",
                vec![
                    es(0x536E, "Commentary"),
                    es(0x22_B59C, "jpn"),
                    eu(0x88, 0),
                    audio_ch(2),
                ],
            ),
            mkv_track(
                0x11,
                "S_TEXT/ASS",
                vec![es(0x536E, "Signs"), es(0x22_B59C, "eng"), eu(0x55AA, 1)],
            ),
            mkv_track(0x11, "S_HDMV/PGS", vec![eu(0x88, 0), eu(0x55AB, 1)]),
            mkv_track(
                0x11,
                "S_TEXT/UTF8",
                vec![
                    es(0x536E, "Forced (SDH)"),
                    es(0x22_B59C, "und"),
                    eu(0x88, 0),
                ],
            ),
        ],
    );
    let attachments = em(
        ATTACHMENTS,
        &[
            font("Arial.ttf", "font/ttf", 80 * 1024),
            font("Noto.otf", "font/otf", 40 * 1024),
        ],
    );
    mkv(info, tracks, Some(attachments), 300 * 1024, true)
}

fn font(name: &str, mime: &str, size: usize) -> Vec<u8> {
    em(
        0x61A7,
        &[
            es(0x466E, name),
            es(0x4660, mime),
            el(0x465C, &vec![0xAA; size]),
            eu(0x46AE, 7),
        ],
    )
}

/// Anime-style release: 40 fonts of 100 KiB, more than the byte budget.
fn mkv_many_fonts() -> Vec<u8> {
    let tracks = em(TRACKS, &[mkv_track(0x11, "S_TEXT/ASS", vec![])]);
    let fonts: Vec<_> = (0..40)
        .map(|i| font(&format!("font{i}.ttf"), "font/ttf", 100 * 1024))
        .collect();
    mkv(
        em(INFO, &[]),
        tracks,
        Some(em(ATTACHMENTS, &fonts)),
        100 * 1024,
        true,
    )
}

fn mkv_dv_p7() -> Vec<u8> {
    let video = mkv_track(
        1,
        "V_MPEGH/ISO/HEVC",
        vec![
            el(0x63A2, &hvcc(2, 153, 10, 24 * 256)),
            em(
                0xE0,
                &[
                    eu(0xB0, 3840),
                    eu(0xBA, 2160),
                    em(0x55B0, &[eu(0x55BA, 16)]),
                ],
            ),
            em(
                0x41E4,
                &[
                    eu(0x41E7, u64::from(u32::from_be_bytes(*b"dvcC"))),
                    el(0x41ED, &dovi(7, 6, true, 6)),
                ],
            ),
        ],
    );
    let tracks = em(TRACKS, &[video, mkv_track(2, "A_DTS", vec![audio_ch(6)])]);
    mkv(
        em(INFO, &[eu(0x2A_D7B1, 1_000_000)]),
        tracks,
        None,
        1024,
        false,
    )
}

// ---------------------------------------------------------------- MP4 writer

fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [&((body.len() + 8) as u32).to_be_bytes()[..], kind, body].concat()
}

fn full(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    bx(kind, &[&[0, 0, 0, 0][..], body].concat())
}

fn mvhd(scale: u32, duration: u32) -> Vec<u8> {
    let mut b = vec![0; 8];
    b.extend(scale.to_be_bytes());
    b.extend(duration.to_be_bytes());
    b.resize(96, 0);
    full(b"mvhd", &b)
}

fn packed_lang(lang: &str) -> [u8; 2] {
    let v = lang
        .bytes()
        .fold(0u16, |acc, c| acc << 5 | u16::from(c - 0x60));
    v.to_be_bytes()
}

fn visual(kind: &[u8; 4], w: u16, h: u16, children: &[Vec<u8>]) -> Vec<u8> {
    let mut b = vec![0; 6];
    b.extend([0, 1]);
    b.extend([0; 16]);
    b.extend(w.to_be_bytes());
    b.extend(h.to_be_bytes());
    b.extend(0x0048_0000u32.to_be_bytes());
    b.extend(0x0048_0000u32.to_be_bytes());
    b.extend([0; 4]);
    b.extend(1u16.to_be_bytes());
    b.extend([0; 32]);
    b.extend(0x18u16.to_be_bytes());
    b.extend(0xFFFFu16.to_be_bytes());
    assert_eq!(b.len(), 78);
    b.extend(children.concat());
    bx(kind, &b)
}

fn sound(kind: &[u8; 4], channels: u16, children: &[Vec<u8>]) -> Vec<u8> {
    let mut b = vec![0; 6];
    b.extend([0, 1]);
    b.extend([0; 8]);
    b.extend(channels.to_be_bytes());
    b.extend(16u16.to_be_bytes());
    b.extend([0; 4]);
    b.extend((48000u32 << 16).to_be_bytes());
    assert_eq!(b.len(), 28);
    b.extend(children.concat());
    bx(kind, &b)
}

struct Trak<'a> {
    handler: &'a [u8; 4],
    name: &'a str,
    lang: &'a str,
    scale: u32,
    duration: u32,
    entry: Vec<u8>,
    stts: Vec<(u32, u32)>,
    samples: u32,
    edit: Option<u32>,
}

fn trak(t: Trak<'_>) -> Vec<u8> {
    let mut mdhd = vec![0; 8];
    mdhd.extend(t.scale.to_be_bytes());
    mdhd.extend(t.duration.to_be_bytes());
    mdhd.extend(packed_lang(t.lang));
    mdhd.extend([0, 0]);
    let mut hdlr = vec![0; 4];
    hdlr.extend(t.handler);
    hdlr.extend([0; 12]);
    hdlr.extend(t.name.as_bytes());
    hdlr.push(0);
    let mut stts = (t.stts.len() as u32).to_be_bytes().to_vec();
    for (n, d) in &t.stts {
        stts.extend(n.to_be_bytes());
        stts.extend(d.to_be_bytes());
    }
    let mut stsz = vec![0; 4];
    stsz.extend(t.samples.to_be_bytes());
    stsz.extend(vec![0; 4 * t.samples as usize]);
    let stbl = bx(
        b"stbl",
        &[
            full(b"stsd", &[&1u32.to_be_bytes()[..], &t.entry].concat()),
            full(b"stts", &stts),
            full(b"stsz", &stsz),
            full(b"stco", &[0, 0, 0, 1, 0, 0, 0, 8]),
        ]
        .concat(),
    );
    let mdia = bx(
        b"mdia",
        &[
            full(b"mdhd", &mdhd),
            full(b"hdlr", &hdlr),
            bx(b"minf", &stbl),
        ]
        .concat(),
    );
    let edts = t.edit.map_or_else(Vec::new, |d| {
        let mut e = 1u32.to_be_bytes().to_vec();
        e.extend(d.to_be_bytes());
        e.extend(0u32.to_be_bytes());
        e.extend(0x0001_0000u32.to_be_bytes());
        bx(b"edts", &full(b"elst", &e))
    });
    bx(b"trak", &[full(b"tkhd", &[0; 80]), edts, mdia].concat())
}

/// HEVC DV P8.1 + HDR10, E-AC-3 JOC, AAC and a tx3g track, `moov` after `mdat`.
fn mp4_dv_tail(samples: u32) -> Vec<u8> {
    let colr = bx(b"colr", &[&b"nclx"[..], &[0, 9, 0, 16, 0, 9, 0]].concat());
    let video = trak(Trak {
        handler: b"vide",
        name: "VideoHandler",
        lang: "und",
        scale: 24000,
        duration: 129_600 * 1001,
        entry: visual(
            b"hvc1",
            3840,
            2160,
            &[
                bx(b"hvcC", &hvcc(2, 153, 10, 0)),
                bx(b"dvvC", &dovi(8, 6, false, 1)),
                colr,
            ],
        ),
        stts: vec![(129_600, 1001)],
        samples,
        edit: None,
    });
    let eac3 = trak(Trak {
        handler: b"soun",
        name: "Surround Atmos",
        lang: "eng",
        scale: 48000,
        duration: 48000 * 5400,
        entry: sound(
            b"ec-3",
            2,
            &[bx(b"dec3", &[0x14, 0x00, 0x20, 0x0F, 0x00, 0x01, 0x10])],
        ),
        stts: vec![(1, 1536)],
        samples: 1,
        edit: None,
    });
    let aac = trak(Trak {
        handler: b"soun",
        name: "SoundHandler",
        lang: "deu",
        scale: 48000,
        duration: 48000 * 5400,
        entry: sound(b"mp4a", 2, &[full(b"esds", &[3, 0])]),
        stts: vec![(1, 1024)],
        samples: 1,
        edit: None,
    });
    let text = trak(Trak {
        handler: b"sbtl",
        name: "",
        lang: "fra",
        scale: 1000,
        duration: 5_400_000,
        entry: bx(b"tx3g", &[0; 38]),
        stts: vec![(1, 1000)],
        samples: 1,
        edit: None,
    });
    let moov = bx(
        b"moov",
        &[mvhd(1000, 5_400_000), video, eac3, aac, text].concat(),
    );
    let ftyp = bx(
        b"ftyp",
        &[&b"isom"[..], &[0, 0, 2, 0], b"isomiso2"].concat(),
    );
    [ftyp, bx(b"mdat", &vec![0x11; 300 * 1024]), moov].concat()
}

/// H.264 High 4.1 + AC-3 5.1 in a QuickTime file with `moov` first.
fn mov_avc_head() -> Vec<u8> {
    let mut avcc = vec![1, 100, 0, 41, 0xFF, 0xE1];
    avcc.extend(4u16.to_be_bytes());
    avcc.extend([0x67, 100, 0, 41]);
    avcc.push(1);
    avcc.extend(2u16.to_be_bytes());
    avcc.extend([0x68, 0xEE]);
    let video = trak(Trak {
        handler: b"vide",
        name: "Core Media Video",
        lang: "und",
        scale: 12800,
        duration: 250 * 512,
        entry: visual(b"avc1", 1920, 1080, &[bx(b"avcC", &avcc)]),
        stts: vec![(100, 512), (150, 512)],
        samples: 250,
        edit: Some(60_000),
    });
    let ac3 = trak(Trak {
        handler: b"soun",
        name: "",
        lang: "spa",
        scale: 48000,
        duration: 48000 * 100,
        entry: sound(b"ac-3", 2, &[bx(b"dac3", &[0x10, 0x3D, 0xE0])]),
        stts: vec![(1, 1536)],
        samples: 1,
        edit: None,
    });
    let moov = bx(b"moov", &[mvhd(600, 0), video, ac3].concat());
    let ftyp = bx(b"ftyp", &[&b"qt  "[..], &[0, 0, 2, 0], b"qt  "].concat());
    [ftyp, moov, bx(b"mdat", &vec![0x22; 4096])].concat()
}

// ---------------------------------------------------------------- MKV tests

#[tokio::test]
async fn mkv_hevc_hdr10_tracks_fonts_and_tail_cues() {
    let data = mkv_hdr10();
    let p = run(&mem(&data)).await;
    assert_eq!(val(&p.container), Container::Matroska);
    assert_eq!(val(&p.duration_ms), 5_400_000);
    assert_eq!(p.duration_ms.provenance, Provenance::ContainerHeader);

    let v = p.video.as_ref().unwrap();
    assert_eq!(val(&v.codec), VideoCodec::Hevc);
    assert_eq!(val(&v.profile), "Main 10");
    assert_eq!(val(&v.level), "5.1");
    assert_eq!((val(&v.width), val(&v.height)), (3840, 2160));
    assert!((val(&v.fps) - 23.976).abs() < 0.001);
    assert_eq!(val(&v.bit_depth), 10);
    assert_eq!(val(&v.hdr), vec![Hdr::Hdr10]);
    assert_eq!(v.dv_profile, Field::unknown());

    let [eac3, truehd, aac] = &p.audio[..] else {
        panic!("{:?}", p.audio)
    };
    assert_eq!(val(&eac3.codec), AudioCodec::Eac3);
    assert_eq!(val(&eac3.channels), 6);
    assert_eq!(eac3.atmos_hint, Field::new(true, Provenance::ParsedName));
    assert!(val(&eac3.default));
    assert_eq!(val(&truehd.codec), AudioCodec::TrueHd);
    assert_eq!(val(&truehd.channels), 8);
    assert_eq!(val(&truehd.language), "en-US");
    assert_eq!(truehd.atmos_hint, Field::unknown());
    assert!(!val(&truehd.default));
    assert_eq!(val(&aac.codec), AudioCodec::Aac);
    assert_eq!(val(&aac.language), "jpn");
    assert_eq!(val(&aac.title), "Commentary");
    assert!(!val(&aac.atmos_hint));

    let [ass, pgs, srt] = &p.subtitles[..] else {
        panic!("{:?}", p.subtitles)
    };
    assert_eq!(val(&ass.codec), SubtitleCodec::Ass);
    assert_eq!(ass.forced, Field::new(true, Provenance::ContainerHeader));
    assert!(val(&ass.default));
    assert_eq!(ass.hearing_impaired, Field::unknown());
    assert_eq!(val(&pgs.codec), SubtitleCodec::Pgs);
    assert_eq!(val(&pgs.language), "eng");
    assert!(val(&pgs.hearing_impaired));
    assert_eq!(val(&srt.codec), SubtitleCodec::Srt);
    assert_eq!(srt.language, Field::unknown());
    assert_eq!(srt.forced, Field::new(true, Provenance::ParsedName));
    assert_eq!(
        srt.hearing_impaired,
        Field::new(true, Provenance::ParsedName)
    );

    let names: Vec<_> = p
        .attachments
        .iter()
        .map(|a| (val(&a.filename), val(&a.mime)))
        .collect();
    assert_eq!(
        names,
        [
            ("Arial.ttf".into(), "font/ttf".into()),
            ("Noto.otf".to_string(), "font/otf".to_string())
        ]
    );

    let cues = val(&p.seek_index);
    assert_eq!(cues.offset + cues.len, data.len() as u64);
    assert!(cues.offset > 400 * 1024, "cues at the tail");
    assert!(!p.budget_exhausted && !p.cancelled && p.error.is_none());
    // Head, attachments and the tail block; the 300 KiB cluster is skipped.
    assert!(p.bytes_read <= 4 * BLOCK_SIZE, "{}", p.bytes_read);
}

#[tokio::test]
async fn mkv_fonts_over_budget_still_get_cues_first() {
    let data = mkv_many_fonts();
    let reader = mem(&data);
    let p = run(&reader).await;
    // Cues are read before the font walk, which then runs out of bytes.
    let cues = val(&p.seek_index);
    assert_eq!(cues.offset + cues.len, data.len() as u64);
    assert!(p.budget_exhausted);
    assert!(
        (10..40).contains(&p.attachments.len()),
        "{}",
        p.attachments.len()
    );
    assert_eq!(val(&p.attachments[0].filename), "font0.ttf");
    assert_eq!(val(&p.subtitles[0].codec), SubtitleCodec::Ass);
    assert!(p.bytes_read <= ProbeBudget::default().max_bytes);
    // The SeekHead already located everything, so the block holding the
    // first Cluster header (after 4 MB of fonts) is never fetched.
    // Cluster: 12 byte header, 17 byte Timestamp, 9 + 100 KiB SimpleBlock.
    let cluster = data.len() as u64 - cues.len - (12 + 17 + 9 + 100 * 1024);
    let cluster_block = cluster / BLOCK_SIZE * BLOCK_SIZE;
    let log = reader.log.lock().unwrap().clone();
    assert!(!log.contains(&cluster_block), "{log:?}");
    assert_eq!(log[1], cues.offset / BLOCK_SIZE * BLOCK_SIZE, "{log:?}");
}

#[tokio::test]
async fn mkv_dolby_vision_p7_without_seek_head() {
    let p = run(&mem(&mkv_dv_p7())).await;
    let v = p.video.unwrap();
    assert_eq!(val(&v.dv_profile), 7);
    assert_eq!(val(&v.dv_bl_compat_id), 6);
    assert_eq!(val(&v.hdr), vec![Hdr::DolbyVision, Hdr::Hdr10]);
    // No DefaultDuration: fps from the hvcC average frame rate.
    assert_eq!(val(&v.fps), 24.0);
    assert_eq!(p.duration_ms, Field::unknown());
    assert_eq!(p.seek_index, Field::unknown());
    assert_eq!(val(&p.audio[0].codec), AudioCodec::Dts);
    assert_eq!(val(&p.audio[0].channels), 6);
}

// ---------------------------------------------------------------- MP4 tests

#[tokio::test]
async fn mp4_dv_p81_eac3_joc_with_moov_at_tail() {
    let data = mp4_dv_tail(1000);
    let p = run(&mem(&data)).await;
    assert_eq!(val(&p.container), Container::Mp4);
    assert_eq!(val(&p.duration_ms), 5_400_000);
    let v = p.video.as_ref().unwrap();
    assert_eq!(val(&v.codec), VideoCodec::Hevc);
    assert_eq!(val(&v.profile), "Main 10");
    assert_eq!(val(&v.bit_depth), 10);
    assert_eq!((val(&v.width), val(&v.height)), (3840, 2160));
    assert!((val(&v.fps) - 23.976).abs() < 0.001);
    assert_eq!(val(&v.dv_profile), 8);
    assert_eq!(val(&v.dv_bl_compat_id), 1);
    assert_eq!(val(&v.hdr), vec![Hdr::DolbyVision, Hdr::Hdr10]);

    let [eac3, aac] = &p.audio[..] else {
        panic!("{:?}", p.audio)
    };
    assert_eq!(val(&eac3.codec), AudioCodec::Eac3);
    assert_eq!(val(&eac3.channels), 6);
    assert_eq!(
        eac3.atmos_hint,
        Field::new(true, Provenance::ContainerHeader)
    );
    assert_eq!(val(&eac3.language), "eng");
    assert_eq!(val(&eac3.title), "Surround Atmos");
    assert_eq!(val(&aac.codec), AudioCodec::Aac);
    assert_eq!(val(&aac.channels), 2);
    assert_eq!(val(&aac.language), "deu");
    assert_eq!(aac.title, Field::unknown());
    assert!(!val(&aac.atmos_hint));

    let [sub] = &p.subtitles[..] else {
        panic!("{:?}", p.subtitles)
    };
    assert_eq!(val(&sub.codec), SubtitleCodec::MovText);
    assert_eq!(val(&sub.language), "fra");

    let moov = val(&p.seek_index);
    assert_eq!(moov.offset + moov.len, data.len() as u64);
    assert!(!p.budget_exhausted);
}

#[tokio::test]
async fn mov_avc_with_moov_at_head() {
    let p = run(&mem(&mov_avc_head())).await;
    assert_eq!(val(&p.container), Container::QuickTime);
    // mvhd duration is 0, so the edit list decides: 60000 / 600 s.
    assert_eq!(val(&p.duration_ms), 100_000);
    let v = p.video.unwrap();
    assert_eq!(val(&v.codec), VideoCodec::H264);
    assert_eq!(val(&v.profile), "High");
    assert_eq!(val(&v.level), "4.1");
    assert_eq!(val(&v.bit_depth), 8);
    assert_eq!(val(&v.fps), 25.0);
    // No colr box and no Dolby Vision record: HDR is unknown, not SDR.
    assert_eq!(v.hdr, Field::unknown());
    assert_eq!(val(&p.audio[0].codec), AudioCodec::Ac3);
    assert_eq!(val(&p.audio[0].channels), 6);
    assert_eq!(val(&p.audio[0].language), "spa");
}

#[tokio::test]
async fn mp4_moov_larger_than_budget_is_walked_lazily() {
    // 130k samples: a 520 KB stsz inside a moov bigger than the budget.
    let data = mp4_dv_tail(130_000);
    let budget = ProbeBudget {
        max_bytes: 5 * BLOCK_SIZE,
        ..Default::default()
    };
    let p = probe(&mem(&data), budget, &CancellationToken::new()).await;
    let moov = val(&p.seek_index);
    assert!(moov.len > budget.max_bytes, "{moov:?}");
    assert_eq!(val(&p.video.unwrap().dv_profile), 8);
    assert_eq!(p.audio.len(), 2);
    assert_eq!(p.subtitles.len(), 1);
    assert!(p.bytes_read <= budget.max_bytes);
}

// ---------------------------------------------------------------- budget, cancel, errors

#[tokio::test]
async fn byte_budget_exhausted_keeps_partial_results() {
    // MP4: only the head block fits, moov at the tail is out of reach.
    let one_block = ProbeBudget {
        max_bytes: BLOCK_SIZE,
        ..Default::default()
    };
    let p = probe(&mem(&mp4_dv_tail(10)), one_block, &CancellationToken::new()).await;
    assert!(p.budget_exhausted);
    assert_eq!(val(&p.container), Container::Mp4);
    assert!(p.video.is_none());
    assert_eq!(p.bytes_read, BLOCK_SIZE);

    // MKV: head and Cues fit; the second font header, one block further,
    // does not.
    let two_blocks = ProbeBudget {
        max_bytes: 2 * BLOCK_SIZE,
        ..Default::default()
    };
    let p = probe(&mem(&mkv_hdr10()), two_blocks, &CancellationToken::new()).await;
    assert!(p.budget_exhausted);
    assert!(p.video.is_some());
    assert_eq!(p.audio.len(), 3);
    assert!(p.seek_index.value.is_some());
    assert_eq!(p.attachments.len(), 1);
}

#[tokio::test(start_paused = true)]
async fn time_budget_and_cancellation() {
    let slow = Mem {
        delay: Duration::from_millis(400),
        ..mem(&mkv_hdr10())
    };
    let budget = ProbeBudget {
        max_time: Duration::from_millis(1000),
        ..Default::default()
    };
    let p = probe(&slow, budget, &CancellationToken::new()).await;
    assert!(p.budget_exhausted && !p.cancelled);
    assert_eq!(p.elapsed, Duration::from_millis(1000));
    // Head and Cues landed at 400 and 800 ms; the font prefetch did not.
    assert!(p.video.is_some());
    assert!(p.seek_index.value.is_some());
    assert_eq!(p.attachments.len(), 1);

    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        trigger.cancel();
    });
    let p = probe(&slow, ProbeBudget::default(), &cancel).await;
    assert!(p.cancelled && !p.budget_exhausted);
    assert_eq!(p.elapsed, Duration::from_millis(100));
    assert_eq!(p.container, Field::unknown());
}

#[tokio::test]
async fn reader_errors_are_reported() {
    let broken = Mem {
        fail: true,
        ..mem(&mkv_hdr10())
    };
    let p = run(&broken).await;
    assert_eq!(p.error, Some(ProbeError::Http("boom".into())));
    assert_eq!(p.container, Field::unknown());
}

// ---------------------------------------------------------------- OpenSubtitles hash

#[tokio::test]
async fn opensubtitles_hash_matches_reference() {
    // Expected value from the reference Python implementation in the
    // OpenSubtitles docs, run over the same bytes.
    let data: Vec<u8> = (0..200_000u32).map(|i| ((i * 7 + 3) % 251) as u8).collect();
    let (hash, size) = opensubtitles_hash(&mem(&data)).await.unwrap();
    assert_eq!((hash.as_str(), size), ("e6ed0e283146465f", 200_000));
    // All zero: the hash is the size.
    let (hash, _) = opensubtitles_hash(&mem(&[0; 131_072])).await.unwrap();
    assert_eq!(hash, "0000000000020000");
    assert_eq!(
        opensubtitles_hash(&mem(&[0; 1000])).await,
        Err(ProbeError::Truncated)
    );
}

// ---------------------------------------------------------------- HTTP

/// Minimal HTTP/1.1 file server; `ranges` false answers every request 200.
async fn serve(data: Vec<u8>, ranges: bool) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let data = Arc::new(data);
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let (data, counter) = (data.clone(), counter.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let mut n = 0;
                while !buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut buf[n..]).await {
                        Ok(0) | Err(_) => return,
                        Ok(r) => n += r,
                    }
                }
                counter.fetch_add(1, Ordering::SeqCst);
                let head = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
                let range = head
                    .lines()
                    .find_map(|l| l.strip_prefix("range: bytes="))
                    .and_then(|r| {
                        let (a, b) = r.trim().split_once('-')?;
                        Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?))
                    })
                    .filter(|_| ranges);
                let (status, extra, body) = match range {
                    Some((a, _)) if a >= data.len() => {
                        ("416 Range Not Satisfiable", String::new(), &data[..0])
                    }
                    Some((a, b)) => {
                        let b = b.min(data.len() - 1);
                        let cr = format!("Content-Range: bytes {a}-{b}/{}\r\n", data.len());
                        ("206 Partial Content", cr, &data[a..=b])
                    }
                    None => ("200 OK", String::new(), &data[..]),
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
                    body.len()
                );
                let _ = sock.write_all(head.as_bytes()).await;
                let _ = sock.write_all(body).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    (format!("http://{addr}/file"), hits)
}

fn http_reader(url: &str, cache: Arc<ByteCache>) -> HttpRangeReader {
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    HttpRangeReader::new(http, url, Vec::new(), cache).allow_private(true)
}

#[tokio::test]
async fn http_reader_learns_length_and_shares_the_cache() {
    let data = mkv_hdr10();
    let (url, hits) = serve(data.clone(), true).await;
    let cache = Arc::new(ByteCache::new(8 << 20));
    let reader = http_reader(&url, cache.clone());
    assert_eq!(reader.len().await.unwrap(), data.len() as u64);
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    // Inside the first block: served from the cache.
    assert_eq!(&reader.read_at(10, 100).await.unwrap()[..], &data[10..110]);
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    let at = 2 * BLOCK_SIZE as usize + 5;
    assert_eq!(
        &reader.read_at(at as u64, 10).await.unwrap()[..],
        &data[at..at + 10]
    );
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    assert!(
        reader
            .read_at(data.len() as u64 + 10, 5)
            .await
            .unwrap()
            .is_empty()
    );

    let over_http = run(&reader).await;
    let in_memory = run(&mem(&data)).await;
    assert_eq!(over_http.video, in_memory.video);
    assert_eq!(over_http.audio, in_memory.audio);
    assert_eq!(over_http.attachments, in_memory.attachments);
    assert_eq!(over_http.seek_index, in_memory.seek_index);

    // A second reader for the same URL starts warm.
    let before = hits.load(Ordering::SeqCst);
    let warm = http_reader(&url, cache);
    warm.read_at(0, 1000).await.unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), before);
}

#[tokio::test]
async fn http_reader_reports_ignored_range() {
    let (url, _) = serve(vec![7; 300 * 1024], false).await;
    let reader = http_reader(&url, Arc::new(ByteCache::new(1 << 20)));
    assert_eq!(reader.len().await, Err(ProbeError::RangeUnsupported));
    let p = run(&reader).await;
    assert_eq!(p.error, Some(ProbeError::RangeUnsupported));

    // A small file sent whole with 200 is still usable.
    let data = mov_avc_head();
    let (url, _) = serve(data.clone(), false).await;
    let reader = http_reader(&url, Arc::new(ByteCache::new(1 << 20)));
    assert_eq!(reader.len().await.unwrap(), data.len() as u64);
    let p = run(&reader).await;
    assert_eq!(val(&p.video.unwrap().codec), VideoCodec::H264);
}

#[tokio::test]
async fn opensubtitles_hash_over_http() {
    let data: Vec<u8> = (0..200_000u32).map(|i| ((i * 7 + 3) % 251) as u8).collect();
    let (url, hits) = serve(data, true).await;
    let reader = http_reader(&url, Arc::new(ByteCache::new(1 << 20)));
    let (hash, size) = opensubtitles_hash(&reader).await.unwrap();
    assert_eq!((hash.as_str(), size), ("e6ed0e283146465f", 200_000));
    assert_eq!(hits.load(Ordering::SeqCst), 2);
}

// ---------------------------------------------------------------- fuzz

fn probe_blocking(data: Vec<u8>) -> StreamProbe {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    rt.block_on(async { run(&mem(&data)).await })
}

static SEEDS: std::sync::LazyLock<[Vec<u8>; 4]> =
    std::sync::LazyLock::new(|| [mkv_hdr10(), mkv_dv_p7(), mp4_dv_tail(50), mov_avc_head()]);

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    #[test]
    fn random_bytes_never_panic(
        prefix in prop_oneof![
            Just(vec![]),
            Just(vec![0x1A, 0x45, 0xDF, 0xA3]),
            Just(b"\0\0\0\x18ftypisom".to_vec()),
        ],
        tail in proptest::collection::vec(any::<u8>(), 0..2048),
    ) {
        probe_blocking([prefix, tail].concat());
    }

    #[test]
    fn mutated_files_never_panic(
        seed in 0usize..4,
        edits in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..24),
        cut in any::<prop::sample::Index>(),
        truncate in any::<bool>(),
    ) {
        let mut data = SEEDS[seed].clone();
        // Bias edits toward the headers, where sizes and IDs live.
        let head = data.len().min(4096);
        for (i, (at, byte)) in edits.iter().enumerate() {
            let pos = if i % 2 == 0 { at.index(head) } else { at.index(data.len()) };
            data[pos] = *byte;
        }
        if truncate {
            data.truncate(cut.index(data.len()));
        }
        probe_blocking(data);
    }
}

// ---------------------------------------------------------------- hev1

/// A synthetic file from `tests/fixtures` (ffmpeg's testsrc, 64x64, 1 s).
fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(path).unwrap()
}

fn at(data: &[u8], kind: &[u8; 4]) -> u64 {
    data.windows(4)
        .position(|w| w == kind)
        .expect("type in file") as u64
}

async fn hev1(data: &[u8]) -> Vec<Hev1Entry> {
    hev1_entries(
        &mem(data),
        ProbeBudget::default(),
        &CancellationToken::new(),
    )
    .await
}

/// ffmpeg's hev1 files carry VPS, SPS and PPS in `hvcC`: the entry is
/// found where `moov` sits, at the head or after `mdat`, and the full
/// probe reports the same.
#[tokio::test]
async fn hev1_entries_are_found_with_moov_at_the_head_or_the_tail() {
    for name in ["hev1-moov-head.mp4", "hev1-moov-tail.mp4"] {
        let data = fixture(name);
        let want = vec![Hev1Entry {
            fourcc_at: at(&data, b"hev1"),
            parameter_sets: true,
        }];
        assert_eq!(hev1(&data).await, want, "{name}");
        assert_eq!(run(&mem(&data)).await.hev1, want, "{name}");
    }
}

/// An `hvcC` with no parameter set arrays: listed, but not relabelable.
#[tokio::test]
async fn an_hev1_entry_without_parameter_sets_says_so() {
    let mut data = fixture("hev1-moov-tail.mp4");
    let record = at(&data, b"hvcC") as usize + 4;
    data[record + 22] = 0; // numOfArrays
    let want = vec![Hev1Entry {
        fourcc_at: at(&data, b"hev1"),
        parameter_sets: false,
    }];
    assert_eq!(hev1(&data).await, want);
}

/// `hvc1` and H.264 files have none, and a Matroska file costs one read.
#[tokio::test]
async fn hvc1_h264_and_matroska_have_no_hev1_entries() {
    for name in ["hvc1.mp4", "h264.mp4"] {
        assert!(hev1(&fixture(name)).await.is_empty(), "{name}");
    }
    let mut mkv = vec![0x1a, 0x45, 0xdf, 0xa3];
    mkv.resize(4096, 0);
    let reader = mem(&mkv);
    let found = hev1_entries(&reader, ProbeBudget::default(), &CancellationToken::new()).await;
    assert!(found.is_empty());
    assert_eq!(*reader.log.lock().unwrap(), vec![0]);
}
