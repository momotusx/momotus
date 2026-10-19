//! Facts from Stremio addon stream text (`name`, `description`/`title`, `behaviorHints.filename`).
//!
//! Addons write seeders, size, indexer and debrid state into emoji-prefixed lines. Each field has
//! an ordered list of patterns (data, below); the first pattern that matches wins. The release
//! name itself goes through [`crate::parse`], with the other strings only filling gaps.

use std::sync::LazyLock;

use crate::lean::{Groups, Re};
use crate::model::{AddonStreamFacts, ParsedRelease, Quality, Resolution, VideoCodec};
use crate::parse;

/// Longest input considered per string (addon text is untrusted).
const MAX_TEXT_BYTES: usize = 4096;
const MAX_SOURCE_CHARS: usize = 64;

/// Seeders: capture 1 is the count.
const SEEDERS: &[&str] = &[
    r"👤\s*(\d[\d,]*)",
    r"🌱\s*(\d[\d,]*)",
    r"(?i)\bseeders?\s*[:=]?\s*(\d[\d,]*)",
    r"(?i)\bseeds?\s*[:=]\s*(\d[\d,]*)",
];

/// Size: capture 1 is the number, capture 2 the unit. Labeled patterns come first so that a
/// size written inside a release name ("800MB") loses against the addon's own field.
const SIZE: &[&str] = &[
    r"(?i)[💾📦📁📂]\u{FE0F}?\s*(\d+(?:[.,]\d+)?)\s*(KiB|KB|MiB|MB|GiB|GB|TiB|TB|B)\b",
    r"(?i)\bsize\s*[:=]\s*(\d+(?:[.,]\d+)?)\s*(KiB|KB|MiB|MB|GiB|GB|TiB|TB)\b",
    r"(?i)\b(\d+(?:[.,]\d+)?)\s*(KiB|KB|MiB|MB|GiB|GB|TiB|TB)\b",
];

/// Indexer or tracker: capture 1 is the site name.
const SOURCE: &[&str] = &[
    r"⚙\u{FE0F}?\s*([^\n]+)",
    r"🔎\s*([^\n]+)",
    r"🔍\s*([^\n]+)",
    r"🔗\s*([^\n🧑]+)",
    r"(?i)\b(?:indexer|source)\s*[:=]\s*([^\n]+)",
];

/// Debrid tag in the stream name: capture 1 is the service code, capture 2 the cache marker.
const SERVICE: &[&str] = &[
    r"\[\s*([A-Z]{2,4})\s*(\+| download|⚡\u{FE0F}?|⬇\u{FE0F}?|⏳|✅|❌)?\s*\]",
    r"(?:🧲|📰)\s*([A-Z]{2,4})\b[^\n]*?(⚡\u{FE0F}?|⏳)",
];

/// Cache marker to state. Anything else leaves the state unknown.
const CACHED_MARKERS: &[(&str, bool)] = &[
    ("+", true),
    ("⚡", true),
    ("✅", true),
    (" download", false),
    ("⬇", false),
    ("⏳", false),
    ("❌", false),
];

/// Debrid short code to service id (ids as used by AIOStreams `behaviorHints.service`).
const SERVICE_IDS: &[(&str, &str)] = &[
    ("RD", "realdebrid"),
    ("AD", "alldebrid"),
    ("PM", "premiumize"),
    ("DL", "debridlink"),
    ("TB", "torbox"),
    ("OC", "offcloud"),
    ("PP", "pikpak"),
    ("PUT", "putio"),
    ("EN", "easynews"),
    ("ED", "easydebrid"),
    ("DR", "debrider"),
];

/// Flag emoji (country code) to the language an addon means by it.
const FLAG_LANGUAGES: &[(&str, &str)] = &[
    ("GB", "en"),
    ("US", "en"),
    ("AU", "en"),
    ("CA", "en"),
    ("RU", "ru"),
    ("UA", "uk"),
    ("FR", "fr"),
    ("IT", "it"),
    ("ES", "es"),
    ("MX", "es"),
    ("AR", "es"),
    ("DE", "de"),
    ("AT", "de"),
    ("PT", "pt"),
    ("BR", "pt"),
    ("JP", "ja"),
    ("KR", "ko"),
    ("CN", "zh"),
    ("TW", "zh"),
    ("IN", "hi"),
    ("TR", "tr"),
    ("PL", "pl"),
    ("NL", "nl"),
    ("SE", "sv"),
    ("NO", "no"),
    ("DK", "da"),
    ("FI", "fi"),
    ("CZ", "cs"),
    ("SK", "sk"),
    ("HU", "hu"),
    ("RO", "ro"),
    ("BG", "bg"),
    ("GR", "el"),
    ("IL", "he"),
    ("SA", "ar"),
    ("TH", "th"),
    ("VN", "vi"),
    ("ID", "id"),
    ("HR", "hr"),
    ("RS", "sr"),
    ("SI", "sl"),
    ("LT", "lt"),
    ("LV", "lv"),
    ("EE", "et"),
];

/// Cinema and pre-release copy tags, worst first (r2 P2-1). They are looked
/// for apart from the release parser, whose first source tag wins. A bare
/// `CAM`, `TS`, `TC` or `R5` counts in capitals only, so a word of an
/// episode's title ("The.Cam.Girl") is no copy.
const COPIES: &[(&str, Quality)] = &[
    (
        r"\b(?:(?i:H[DQ][ .-]*CAM(?:H[DQ])?(?:[ .-]*Rip|Rp)?)|(?i:CAM(?:H[DQ])?(?:[ .-]*Rip|Rp))|CAM(?:H[DQ])?)\b",
        Quality::Cam,
    ),
    (r"\b(?i:(?:H[DQ][ .-]*)?S[ .-]print)\b", Quality::Cam),
    (
        r"\b(?:(?i:HD[ .-]*T(?:ELE)?(?:S(?:YNC)?|C(?:INE)?)(?:Rip)?)|(?i:TELE(?:SYNC|CINE)(?:Rip)?)|(?i:T[SC]Rip)|T[SC])\b",
        Quality::Telesync,
    ),
    (
        r"\b(?i:(?:DVD?|BD|BR|HD|WEB)?[ .-]*SCR(?:EENER)?)\b",
        Quality::Screener,
    ),
    (r"\b(?i:P(?:RE)?-?(?:HD|DVD)(?:Rip)?)\b", Quality::Screener),
    (r"\bR[56]\b", Quality::Screener),
];

/// Where a release's own name ends: its year or its episode code. A copy
/// tag before it is a title word ("Body Cam").
const TITLE_END: &str = r"(?i)\b(?:19|20)\d{2}\b|\bS\d{1,2}E\d{1,4}\b";

/// Video file extensions, cut before parsing so `.ts` never reads as a
/// telesync tag (r2 P2-2).
const EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "m4v", "avi", "ts", "m2ts", "mov", "webm", "wmv", "mpg", "mpeg", "flv", "vob",
    "iso", "divx", "ogv", "3gp",
];

fn compile(pats: &[&str]) -> Vec<Re> {
    pats.iter().map(|p| Re::new(p, false)).collect()
}

static SEEDERS_RE: LazyLock<Vec<Re>> = LazyLock::new(|| compile(SEEDERS));
static SIZE_RE: LazyLock<Vec<Re>> = LazyLock::new(|| compile(SIZE));
static SOURCE_RE: LazyLock<Vec<Re>> = LazyLock::new(|| compile(SOURCE));
static SERVICE_RE: LazyLock<Vec<Re>> = LazyLock::new(|| compile(SERVICE));
static COPIES_RE: LazyLock<Vec<(Re, Quality)>> = LazyLock::new(|| {
    COPIES
        .iter()
        .map(|(p, q)| (Re::new(p, false), *q))
        .collect()
});
static TITLE_END_RE: LazyLock<Re> = LazyLock::new(|| Re::new(TITLE_END, false));

/// `name.ts` as (`name`, `ts`); a name with no video extension as it is.
fn split_extension(name: &str) -> (&str, Option<String>) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()) => {
            (stem, Some(ext.to_ascii_lowercase()))
        }
        _ => (name, None),
    }
}

/// Parses a file name or a title line without its video extension, which
/// becomes the container.
fn parse_text(text: &str) -> ParsedRelease {
    let (stem, ext) = split_extension(text.trim());
    let mut release = parse(&normalize_labels(stem));
    if ext.is_some() {
        release.container = ext;
    }
    release
}

/// The worst copy tag in `text`, past the release's own name and before
/// its trailing group ("x264-HDCAM" is a group, not a copy).
fn copy_in(text: &str) -> Option<Quality> {
    let (stem, _) = split_extension(text.trim());
    let from = TITLE_END_RE
        .captures_at(stem, 0)
        .and_then(|g| g.span(0))
        .map_or(0, |(_, end)| end);
    let rest = &stem[from..];
    let rest = match rest.rsplit_once('-') {
        Some((head, group))
            if !group.is_empty() && group.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            head
        }
        _ => rest,
    };
    COPIES_RE
        .iter()
        .find(|(re, _)| re.is_match(rest))
        .map(|(_, q)| *q)
}

fn is_copy(q: Quality) -> bool {
    matches!(q, Quality::Cam | Quality::Telesync | Quality::Screener)
}

fn clip(s: &str) -> &str {
    crate::custom::slice(s, 0, MAX_TEXT_BYTES)
}

/// One match with its capture groups, bound to the text it was found in.
struct Cap<'t> {
    text: &'t str,
    groups: Groups,
}

impl<'t> Cap<'t> {
    /// Text of group `i`, empty when the group did not take part.
    fn get(&self, i: usize) -> &'t str {
        self.groups.text(self.text, i).unwrap_or("")
    }
}

/// First match of the first pattern that matches any of the texts (texts in order).
fn first_capture<'t>(pats: &[Re], texts: &[&'t str]) -> Option<Cap<'t>> {
    pats.iter().find_map(|re| {
        texts
            .iter()
            .find_map(|t| re.captures_at(t, 0).map(|groups| Cap { text: t, groups }))
    })
}

fn seeders(texts: &[&str]) -> Option<u32> {
    let cap = first_capture(&SEEDERS_RE, texts)?;
    cap.get(1).replace(',', "").parse().ok()
}

fn unit_factor(unit: &str) -> f64 {
    let power = match unit.to_ascii_uppercase().chars().next() {
        Some('B') => 0,
        Some('K') => 1,
        Some('M') => 2,
        Some('G') => 3,
        _ => 4,
    };
    1024f64.powi(power)
}

/// "4,2" is 4.2, "1,024" is 1024, "1,024.5" is 1024.5.
fn number(s: &str) -> Option<f64> {
    let norm = match (s.contains(','), s.contains('.')) {
        (true, true) => s.replace(',', ""),
        (true, false) => match s.rsplit_once(',') {
            Some((_, frac)) if frac.len() == 3 => s.replace(',', ""),
            _ => s.replace(',', "."),
        },
        _ => s.to_string(),
    };
    norm.parse().ok()
}

fn size_bytes(texts: &[&str]) -> Option<u64> {
    let cap = first_capture(&SIZE_RE, texts)?;
    let bytes = number(cap.get(1))? * unit_factor(cap.get(2));
    (bytes.is_finite() && (1.0..1e16).contains(&bytes)).then_some(bytes as u64)
}

fn source_site(texts: &[&str]) -> Option<String> {
    let cap = first_capture(&SOURCE_RE, texts)?;
    let site = cap.get(1).trim();
    let site: String = site.chars().take(MAX_SOURCE_CHARS).collect();
    (!site.is_empty()).then_some(site)
}

/// The first debrid tag whose code is a known service, with its cache state if it states one.
fn service_and_cached(name: &str) -> (Option<String>, Option<bool>) {
    for re in SERVICE_RE.iter() {
        let mut from = 0;
        while from <= name.len() {
            let Some(groups) = re.captures_at(name, from) else {
                break;
            };
            from = groups.span(0).map_or(name.len() + 1, |s| s.1.max(from + 1));
            let cap = Cap { text: name, groups };
            if let Some((_, id)) = SERVICE_IDS.iter().find(|(c, _)| *c == cap.get(1)) {
                let marker = cap.get(2);
                let cached = CACHED_MARKERS
                    .iter()
                    .find(|(m, _)| marker.starts_with(m))
                    .map(|(_, v)| *v);
                return (Some((*id).to_string()), cached);
            }
        }
    }
    (None, None)
}

/// Languages named by flag emoji (pairs of regional indicator symbols), plus "🌎" for Latino.
fn flag_languages(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let lang = match (regional(c), chars.peek().copied().and_then(regional)) {
            (Some(a), Some(b)) => {
                chars.next();
                let code: String = [a, b].iter().collect();
                FLAG_LANGUAGES
                    .iter()
                    .find(|(cc, _)| *cc == code)
                    .map(|(_, l)| *l)
            }
            _ if c == '🌎' => Some("es"),
            _ => None,
        };
        if let Some(l) = lang
            && !out.iter().any(|o| o == l)
        {
            out.push(l.to_string());
        }
    }
    out
}

fn regional(c: char) -> Option<char> {
    let n = c as u32;
    (0x1F1E6..=0x1F1FF)
        .contains(&n)
        .then(|| char::from(b'A' + (n - 0x1F1E6) as u8))
}

/// A description line that carries addon metrics or flags rather than release text.
fn is_metric_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with(['👤', '💾', '⚙', '🔎', '🔍', '🔗', '🌱', '📦'])
        || t.chars()
            .next()
            .is_some_and(|c| regional(c).is_some() || c == '🌎')
}

/// Drop leading emoji and punctuation ("📄 name.mkv" becomes "name.mkv").
fn strip_marker(line: &str) -> &str {
    line.trim_start_matches(|c: char| !c.is_alphanumeric() && c != '[' && c != '(')
}

/// Comet names its audio classes "DTS Lossless" and "DTS Lossy".
fn normalize_labels(line: &str) -> String {
    line.replace("DTS Lossless", "DTS-HD MA")
        .replace("DTS Lossy", "DTS")
}

/// What an addon puts in `name` besides its own label: the lines after the first
/// ("Torrentio\n4k HDR"), or the whole name without its `[tag]` ("[TORRENT🧲] Comet 1080p").
fn name_hints(name: &str) -> ParsedRelease {
    let lines: Vec<&str> = name.lines().collect();
    let text = match lines.as_slice() {
        [] => String::new(),
        [only] => only.split(']').next_back().unwrap_or(only).to_string(),
        [_, rest @ ..] => rest.join(" "),
    };
    let mut hints = parse(&normalize_labels(&text));
    hints.year = None;
    hints
}

/// Fill what `base` does not know from `other`; release-level flags are OR-ed. The title only
/// ever comes from the primary string: other lines are labels and metrics, not names.
fn fill_gaps(base: &mut ParsedRelease, other: &ParsedRelease) {
    base.year = base.year.or(other.year);
    if base.resolution == Resolution::Unknown {
        base.resolution = other.resolution;
    }
    if base.quality == Quality::Unknown {
        base.quality = other.quality;
    }
    if base.codec == VideoCodec::Unknown {
        base.codec = other.codec;
    }
    base.bit_depth = base.bit_depth.or(other.bit_depth);
    base.channels = base.channels.take().or_else(|| other.channels.clone());
    base.group = base.group.take().or_else(|| other.group.clone());
    base.container = base.container.take().or_else(|| other.container.clone());
    if base.hdr.is_empty() {
        base.hdr.clone_from(&other.hdr);
    }
    if base.audio.is_empty() {
        base.audio.clone_from(&other.audio);
    }
    if base.seasons.is_empty() {
        base.seasons.clone_from(&other.seasons);
    }
    if base.episodes.is_empty() {
        base.episodes.clone_from(&other.episodes);
    }
    merge_languages(&mut base.languages, &other.languages);
    base.complete |= other.complete;
    base.remux |= other.remux;
    base.proper |= other.proper;
    base.repack |= other.repack;
    base.extended |= other.extended;
    base.cut = base.cut.or(other.cut);
    base.hardcoded_subs |= other.hardcoded_subs;
    base.dubbed |= other.dubbed;
    base.three_d |= other.three_d;
    base.upscaled |= other.upscaled;
}

fn merge_languages(into: &mut Vec<String>, extra: &[String]) {
    for l in extra {
        if !into.contains(l) {
            into.push(l.clone());
        }
    }
}

fn release_from(name: &str, description: &str, hint: Option<&str>) -> ParsedRelease {
    let lines: Vec<&str> = description.lines().collect();
    let first_text = lines
        .iter()
        .find(|l| !is_metric_line(l) && !l.trim().is_empty());
    let hint = hint.map(str::trim).filter(|h| !h.is_empty());
    let hint_name = hint.map(|h| h.rsplit(['/', '\\']).next().unwrap_or(h));
    let primary = hint_name
        .or(first_text.map(|l| strip_marker(l)))
        .unwrap_or("");
    let mut release = parse_text(primary);
    // The addon labels this file (Torrentio names a pack file 480p when
    // the pack says 1080p): its name fills gaps before the description.
    fill_gaps(&mut release, &name_hints(name));
    let mut extras: Vec<&str> = Vec::new();
    extras.extend(first_text.map(|l| strip_marker(l)));
    extras.extend(
        lines
            .iter()
            .filter(|l| !is_metric_line(l))
            .map(|l| strip_marker(l)),
    );
    for extra in &extras {
        if *extra != primary && !extra.trim().is_empty() {
            fill_gaps(&mut release, &parse_text(extra));
        }
    }
    // A copy tag in the file name or any title line makes the release a
    // copy, the worst one found; none found, it is no copy, whatever tag
    // the parser took first or the addon's label says (Torrentio calls a
    // `.ts` file TeleSync; r2 P2-1, P2-2).
    let worst = std::iter::once(primary)
        .chain(extras)
        .filter_map(copy_in)
        .min_by_key(|q| COPIES.iter().position(|(_, c)| c == q));
    match worst {
        Some(q) => release.quality = q,
        None if is_copy(release.quality) => release.quality = Quality::Unknown,
        None => {}
    }
    merge_languages(&mut release.languages, &flag_languages(description));
    release
}

/// Parse one addon stream: `name` and `description` are the Stremio fields (`title` for older
/// addons goes into `description`), `filename_hint` is `behaviorHints.filename`.
pub fn parse_addon_stream(
    name: &str,
    description: &str,
    filename_hint: Option<&str>,
) -> AddonStreamFacts {
    let (name, description) = (clip(name), clip(description));
    let hint = filename_hint.map(clip);
    let texts = [description, name];
    let (service, cached) = service_and_cached(name);
    AddonStreamFacts {
        release: release_from(name, description, hint),
        seeders: seeders(&texts),
        size_bytes: size_bytes(&texts),
        source_site: source_site(&texts),
        cached,
        service,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn torrentio_lines() {
        let f = parse_addon_stream(
            "Torrentio\n4k DV | HDR",
            "The.Shawshank.Redemption.1994.UHD.BluRay.2160p.DTS-HD.MA.5.1.DV.HEVC.REMUX-FraMeSToR\n👤 92 💾 54.33 GB ⚙️ TorrentGalaxy",
            None,
        );
        assert_eq!(f.seeders, Some(92));
        assert_eq!(f.size_bytes, Some((54.33 * 1024f64.powi(3)) as u64));
        assert_eq!(f.source_site.as_deref(), Some("TorrentGalaxy"));
        assert_eq!(f.release.resolution, Resolution::R2160p);
        assert_eq!(f.cached, None);
    }

    /// Avatar: Fire and Ash, row 42 of the default Torrentio recording
    /// (plans/m13-stream-choice.md 1.1): a screener copy Torrentio labels 4k.
    #[test]
    fn a_web_screener_is_a_screener_copy() {
        let f = parse_addon_stream(
            "Torrentio\n4k",
            "Avatar.Fire.and.Ash.2025.2160p.WEBSCREENER.H.265.Dual.YG⭐\n👤 3 💾 6.1 GB ⚙️ ThePirateBay",
            Some("Avatar.Fire.and.Ash.2025.2160p.WEBSCREENER.H.265.Dual YG.mkv"),
        );
        assert_eq!(f.release.quality, Quality::Screener);
        for (name, quality) in [
            ("Movie.2025.1080p.WEB SCREENER.x264", Quality::Screener),
            ("Movie.2025.DVDSCR.x264", Quality::Screener),
            ("Movie.2025.1080p.WEB-DL.x264", Quality::WebDl),
            ("Movie.2025.1080p.WEBRip.x264", Quality::WebRip),
        ] {
            assert_eq!(parse(name).quality, quality, "{name}");
        }
    }

    /// Code review r2 P2-1 and P2-2: a copy tag in the file name or any
    /// title line makes a copy, the worst one; an extension, a title word,
    /// a release group or the addon's own label never does.
    #[test]
    fn copies_are_found_in_every_line_and_nowhere_else() {
        let quality = |label: &str, title: &str, file: Option<&str>| {
            parse_addon_stream(&format!("Torrentio\n{label}"), title, file)
                .release
                .quality
        };
        for (title, file, want) in [
            (
                "Dune.Part.Two.2024.HDCAM.c1nem4.x264-SUNSCREEN",
                None,
                Quality::Cam,
            ),
            ("Movie.2025.1080p.HDRip.HQ-CAM.x264.AAC", None, Quality::Cam),
            ("Movie.2025.720p.hdtc.x264-GRP", None, Quality::Telesync),
            (
                "Avatar Fire and Ash (2025) Telesync TS HD 1080p - x264 - [Tam.Tel.Hindi.Kan.Eng]",
                Some("Avatar.Fire.and.Ash.(2025).HDRip.1080p.x264.mkv"),
                Quality::Telesync,
            ),
            (
                "Оппенгеймер / Oppenheimer (2023) BDRip-AVC от DoMiNo | D | TS | IMAX",
                Some("Oppenheimer.2023.DUB.BDRip.x264.mkv"),
                Quality::Telesync,
            ),
            (
                "Movie.2009.R5.DVDRip.XviD-GRP",
                Some("Movie.2009.R5.DVDRip.XviD-GRP.avi"),
                Quality::Screener,
            ),
            (
                "Interstellar.2014.ENG.1080p.HD.WEBRip.2.64GiB.AAC.x264-PortalGoods",
                Some("Interstellar.2014.ENG.1080p.HD.WEBRip.2.64GiB.AAC.x264-PortalGoods.ts"),
                Quality::WebRip,
            ),
            (
                "Show.S01E05.SATRip",
                Some("Show.S01E05.SATRip.ts"),
                Quality::Hdtv,
            ),
            ("Body.Cam.2025.1080p.x265-GRP", None, Quality::Unknown),
            // Capitals in the title: before its year, a word, not a copy.
            (
                "CAM.2018.1080p.NF.WEB-DL.DDP5.1.x264-GRP",
                None,
                Quality::WebDl,
            ),
            (
                "Show.S01E05.The.Cam.Girl.1080p.WEB-DL.x264-GRP",
                None,
                Quality::WebDl,
            ),
            (
                "Movie.2025.1080p.BluRay.x264-SUNSCREEN",
                None,
                Quality::BluRay,
            ),
            (
                "Movie.2025.1080p.WEB-DL.DDP5.1.H.264-HDCAM",
                None,
                Quality::WebDl,
            ),
            (
                "Дюна: Часть вторая / Dune: Part Two [2024 WEB-DLRip]",
                None,
                Quality::WebRip,
            ),
        ] {
            assert_eq!(quality("TeleSync", title, file), want, "{title}");
        }
        let ts = parse_addon_stream(
            "Torrentio\nTeleSync",
            "x",
            Some("Interstellar.2014.1080p.WEBRip.x264.ts"),
        );
        assert_eq!(ts.release.container.as_deref(), Some("ts"));
    }

    /// One Piece E1 in a batch pack (the M13 corpus): the pack names 1080p,
    /// Torrentio labels each file. The 480p copy's label wins over the pack.
    #[test]
    fn a_pack_file_takes_the_addons_resolution_label() {
        let pack = "[Anime Time] One Piece (0001-1071+Movies+Specials) [BD+CR] [Dual Audio] [1080p][HEVC 10bit x265][AAC][Multi Sub] [Batch]";
        let file = "[Anime Time] One Piece - 0001 - I'm Luffy! The Man Who's Gonna Be King of the Pirates!.mkv";
        for (label, folder, want) in [
            (
                "480p",
                "Episode 001-206 Uncropped (480p)",
                Resolution::R480p,
            ),
            ("1080p", "Season 01 - East Blue", Resolution::R1080p),
        ] {
            let text = format!("{pack}\n{folder}/{file}\n👤 846 💾 215.21 MB ⚙️ NyaaSi");
            let f = parse_addon_stream(&format!("Torrentio\n{label}"), &text, Some(file));
            assert_eq!(f.release.resolution, want, "{folder}");
        }
    }

    #[test]
    fn debrid_tags() {
        let rd = parse_addon_stream(
            "[RD+] Torrentio\n4k",
            "x\n👤 1 💾 4.2 GB ⚙️ ThePirateBay",
            None,
        );
        assert_eq!(
            (rd.service.as_deref(), rd.cached),
            (Some("realdebrid"), Some(true))
        );
        let dl = parse_addon_stream("[RD download] Torrentio\n1080p", "x", None);
        assert_eq!(
            (dl.service.as_deref(), dl.cached),
            (Some("realdebrid"), Some(false))
        );
        let comet = parse_addon_stream("[TB⚡] Comet 1080p", "📄 x.mkv\n💾 1 GB 🔎 DMM", None);
        assert_eq!(
            (comet.service.as_deref(), comet.cached),
            (Some("torbox"), Some(true))
        );
        let p2p = parse_addon_stream("[TORRENT🧲] Comet 1080p", "📄 x.mkv", None);
        assert_eq!((p2p.service, p2p.cached), (None, None));
    }

    // The next two strings follow the layouts the addons document (MediaFusion's description
    // builder, Jackettio's title lines); they are not harvested.
    #[test]
    fn mediafusion_layout() {
        let f = parse_addon_stream(
            "MediaFusion 🧲 RD 1080P ⚡️",
            "🎨 HDR|DV 📺 BluRay 🎞️ HEVC 🎵 DTS-HD|Atmos\n4.2 GB 👤 87\n🌐 English + Hindi\n🔗 TamilMV 🧑‍💻 uploader",
            Some("Some.Movie.2020.1080p.BluRay.HEVC.mkv"),
        );
        assert_eq!(
            (f.service.as_deref(), f.cached),
            (Some("realdebrid"), Some(true))
        );
        assert_eq!(f.seeders, Some(87));
        assert_eq!(f.size_bytes, Some((4.2 * 1024f64.powi(3)) as u64));
        assert_eq!(f.source_site.as_deref(), Some("TamilMV"));
        assert_eq!(f.release.title, "Some Movie");
        assert_eq!(f.release.languages, ["en", "hi"]);
        let waiting = parse_addon_stream("MediaFusion 🧲 TB 720P ⏳", "x", None);
        assert_eq!(
            (waiting.service.as_deref(), waiting.cached),
            (Some("torbox"), Some(false))
        );
    }

    #[test]
    fn jackettio_layout() {
        let f = parse_addon_stream(
            "[AD+] Jackettio\n1080p",
            "Movie.2020.1080p.WEB-DL.x264-GRP\n💾 2.1 GB 👤 15 ⚙️ rarbg\n🇬🇧",
            None,
        );
        assert_eq!(
            (f.service.as_deref(), f.cached),
            (Some("alldebrid"), Some(true))
        );
        assert_eq!(f.seeders, Some(15));
        assert_eq!(f.source_site.as_deref(), Some("rarbg"));
        assert_eq!(f.release.quality, Quality::WebDl);
        assert_eq!(f.release.group.as_deref(), Some("GRP"));
    }

    #[test]
    fn flags_and_numbers() {
        assert_eq!(flag_languages("🇬🇧 / 🇷🇺 / 🌎"), ["en", "ru", "es"]);
        assert_eq!(number("4,2"), Some(4.2));
        assert_eq!(number("1,024"), Some(1024.0));
        assert_eq!(number("1,024.5"), Some(1024.5));
    }
}
