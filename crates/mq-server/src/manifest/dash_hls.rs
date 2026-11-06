//! DASH served as HLS (plans/dash-everywhere.md) to a player that plays HLS
//! and not DASH (AVPlayer, mpv, browsers). The core reads its own rewritten
//! MPD (allowlisted, unprefixed, every URL already the core's) and writes an
//! HLS master and one media playlist per Representation, fMP4 with
//! `EXT-X-MAP`. Only a static presentation of one Period whose segments come
//! from `SegmentTemplate`s is taken; every other shape is refused with the
//! format words, as a player that cannot play it.

use super::dash::xs_duration_ms;
use super::{Children, FORMAT_NOT_HERE, OUTPUT_MAX, Role, checkpoint, output_check, template_url};
use mq_http::Url;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;
use std::fmt::Write as _;

/// Segments one Representation may list (as the rewrite's timeline bound).
const MAX_SEGMENTS: i64 = 100_000;
/// The DASH-IF trick mode property: such a set is for fast scan, never a variant.
const TRICK_MODE: &str = "http://dashif.org/guidelines/trickmode";
/// The longest audio rendition `NAME`.
const MAX_NAME: usize = 64;

/// An HLS master and its media playlists, in the order the master names them.
pub(crate) struct Converted {
    pub(crate) master: String,
    pub(crate) media: Vec<String>,
}

/// One element of the rewritten MPD.
#[derive(Default)]
struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    text: String,
    children: Vec<Node>,
}

impl Node {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |c| c.name == name)
    }

    fn first(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    /// Whether `name` is this element or anywhere below it.
    fn holds(&self, name: &str) -> bool {
        self.name == name || self.children.iter().any(|c| c.holds(name))
    }
}

/// A `SegmentTemplate` with what it inherits: attributes of the lower level
/// win, and a lower level's `SegmentTimeline` replaces the one above.
#[derive(Clone, Default)]
struct Template {
    attrs: Vec<(String, String)>,
    /// `S` elements: `t`, `d`, `r`.
    timeline: Option<Vec<(Option<i64>, i64, i64)>>,
}

impl Template {
    fn under(&self, level: &Node) -> Result<Self, &'static str> {
        let Some(own) = level.first("SegmentTemplate") else {
            return Ok(self.clone());
        };
        let mut out = self.clone();
        for (name, value) in &own.attrs {
            out.attrs.retain(|(n, _)| n != name);
            out.attrs.push((name.clone(), value.clone()));
        }
        if let Some(timeline) = own.first("SegmentTimeline") {
            let number = |s: &Node, name: &str| {
                s.attr(name)
                    .map(|v| v.trim().parse::<i64>().map_err(|_| FORMAT_NOT_HERE))
                    .transpose()
            };
            let mut list = Vec::new();
            for s in timeline.all("S") {
                let d = number(s, "d")?.filter(|d| *d > 0).ok_or(FORMAT_NOT_HERE)?;
                list.push((number(s, "t")?, d, number(s, "r")?.unwrap_or(0)));
            }
            out.timeline = Some(list);
        }
        Ok(out)
    }

    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    fn number(&self, name: &str, default: i64) -> Result<i64, &'static str> {
        self.attr(name).map_or(Ok(default), |v| {
            v.trim().parse().map_err(|_| FORMAT_NOT_HERE)
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Video,
    Audio,
}

/// A Representation as a rendition, with what it inherits from its set.
struct Rendition {
    kind: Kind,
    id: String,
    bandwidth: u64,
    codecs: Option<String>,
    width: Option<String>,
    height: Option<String>,
    frame_rate: Option<String>,
    lang: Option<String>,
    label: Option<String>,
    channels: Option<String>,
    /// The set promises segments that each start with a key frame.
    independent: bool,
    base: Url,
    template: Template,
}

/// One segment: its media URL (already the core's) and length in the
/// template's timescale.
struct Segment {
    url: String,
    duration: i64,
}

/// Converts the core's rewritten MPD `mpd`; `renditions` is
/// `{core}h/{token}/`, where the media playlists are served.
pub(crate) fn dash_to_hls(
    mpd: &str,
    renditions: &str,
    children: &mut Children<'_>,
) -> Result<Converted, &'static str> {
    let alive = children.alive;
    let root = parse(mpd, alive)?;
    if root.attr("type").is_some_and(|t| t.trim() != "static") {
        return Err(FORMAT_NOT_HERE);
    }
    if root.holds("SegmentBase") || root.holds("SegmentList") {
        return Err(FORMAT_NOT_HERE);
    }
    let mut periods = root.all("Period");
    let (Some(period), None) = (periods.next(), periods.next()) else {
        return Err(FORMAT_NOT_HERE);
    };
    let end_ms = period_end_ms(&root, period)?;
    let mpd_base = base_of(None, &root)?.ok_or("no MPD BaseURL")?;
    let period_base = base_of(Some(&mpd_base), period)?.unwrap_or(mpd_base);
    let period_template = Template::default().under(period)?;
    let mut list = Vec::new();
    for set in period.all("AdaptationSet") {
        renditions_of(set, &period_base, &period_template, &mut list)?;
    }
    let videos: Vec<&Rendition> = list.iter().filter(|r| r.kind == Kind::Video).collect();
    if videos.is_empty() {
        return Err(FORMAT_NOT_HERE);
    }
    let groups = audio_groups(&list);
    let mut media = Vec::new();
    let mut named = Vec::new();
    for rendition in videos
        .iter()
        .copied()
        .chain(groups.iter().flat_map(|g| g.1.iter().copied()))
    {
        checkpoint(media.len(), alive)?;
        media.push(media_playlist(rendition, end_ms, children)?);
        named.push(rendition.id.clone());
    }
    let index = |r: &Rendition| named.iter().position(|id| *id == r.id).expect("listed");
    let mut master = String::from("#EXTM3U\n#EXT-X-VERSION:7\n");
    if list.iter().all(|r| r.independent) {
        master.push_str("#EXT-X-INDEPENDENT-SEGMENTS\n");
    }
    let mut names: Vec<String> = Vec::new();
    for (n, (_, members)) in groups.iter().enumerate() {
        for (k, a) in members.iter().enumerate() {
            let mut line = format!("#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a{n}\"");
            if let Some(lang) = a.lang.as_deref().filter(|l| language_ok(l)) {
                let _ = write!(line, ",LANGUAGE=\"{lang}\"");
            }
            let name = unique(&mut names, n, rendition_name(a));
            let default = if k == 0 { "YES" } else { "NO" };
            let _ = write!(line, ",NAME=\"{name}\",DEFAULT={default},AUTOSELECT=YES");
            if let Some(channels) = a.channels.as_deref().filter(|c| digits(c)) {
                let _ = write!(line, ",CHANNELS=\"{channels}\"");
            }
            let _ = write!(line, ",URI=\"{renditions}{}/audio.m3u8\"", index(a));
            emit(&mut master, &line)?;
        }
    }
    for v in &videos {
        let audio: Vec<Option<(usize, &Vec<&Rendition>)>> = if groups.is_empty() {
            vec![None]
        } else {
            groups.iter().map(|g| &g.1).enumerate().map(Some).collect()
        };
        for group in audio {
            let audio_bandwidth =
                group.map_or(0, |(_, m)| m.iter().map(|a| a.bandwidth).max().unwrap_or(0));
            let mut line = format!(
                "#EXT-X-STREAM-INF:BANDWIDTH={}",
                v.bandwidth + audio_bandwidth
            );
            if let (Some(w), Some(h)) = (v.width.as_deref(), v.height.as_deref())
                && digits(w)
                && digits(h)
            {
                let _ = write!(line, ",RESOLUTION={w}x{h}");
            }
            if let Some(rate) = v.frame_rate.as_deref().and_then(frame_rate) {
                let _ = write!(line, ",FRAME-RATE={rate:.3}");
            }
            let audio_codec = group.and_then(|(_, m)| m.first()?.codecs.clone());
            let codecs: Vec<String> = v
                .codecs
                .iter()
                .chain(audio_codec.iter())
                .map(|c| hls_codec(c))
                .collect();
            if v.codecs.is_some() && codecs.iter().all(|c| codec_ok(c)) {
                let _ = write!(line, ",CODECS=\"{}\"", codecs.join(","));
            }
            if let Some((n, _)) = group {
                let _ = write!(line, ",AUDIO=\"a{n}\"");
            }
            emit(&mut master, &line)?;
            emit(&mut master, &format!("{renditions}{}/video.m3u8", index(v)))?;
        }
    }
    output_check(&master, children.core, alive)?;
    for text in &media {
        output_check(text, children.core, alive)?;
    }
    Ok(Converted { master, media })
}

/// The rewritten MPD as a tree, its root `MPD` returned.
fn parse(text: &str, alive: &dyn Fn() -> bool) -> Result<Node, &'static str> {
    let mut reader = Reader::from_str(text);
    let mut stack = vec![Node::default()];
    for n in 1.. {
        checkpoint(n, alive)?;
        let event = reader
            .read_event()
            .map_err(|_| "an unreadable rewritten MPD")?;
        match event {
            Event::Start(e) => stack.push(node(&reader, &e)?),
            Event::Empty(e) => {
                let node = node(&reader, &e)?;
                stack.last_mut().expect("the document").children.push(node);
            }
            Event::End(_) => {
                let done = stack.pop().expect("an open element");
                stack
                    .last_mut()
                    .ok_or("an unreadable rewritten MPD")?
                    .children
                    .push(done);
            }
            Event::Text(t) => {
                let t = t.decode().map_err(|_| "not UTF-8")?;
                stack.last_mut().expect("the document").text.push_str(&t);
            }
            Event::GeneralRef(r) => {
                // The core's writer escapes only these.
                let c = match r.decode().map_err(|_| "not UTF-8")?.as_ref() {
                    "amp" => '&',
                    "lt" => '<',
                    "gt" => '>',
                    "quot" => '"',
                    "apos" => '\'',
                    _ => r
                        .resolve_char_ref()
                        .ok()
                        .flatten()
                        .ok_or("an unknown entity")?,
                };
                stack.last_mut().expect("the document").text.push(c);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let mut document = stack
        .pop()
        .filter(|_| stack.is_empty())
        .ok_or("an unreadable rewritten MPD")?;
    document
        .children
        .pop()
        .filter(|n| n.name == "MPD")
        .ok_or("no MPD")
}

fn node(reader: &Reader<&[u8]>, e: &BytesStart<'_>) -> Result<Node, &'static str> {
    let name = std::str::from_utf8(e.local_name().as_ref())
        .map_err(|_| "not UTF-8")?
        .to_owned();
    let mut attrs = Vec::new();
    for attr in e.attributes() {
        let attr = attr.map_err(|_| "a malformed attribute")?;
        let key = std::str::from_utf8(attr.key.as_ref())
            .map_err(|_| "not UTF-8")?
            .to_owned();
        let value = attr
            .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
            .map_err(|_| "a malformed attribute")?;
        attrs.push((key, value.into_owned()));
    }
    Ok(Node {
        name,
        attrs,
        ..Node::default()
    })
}

/// The Period's end from its start, in ms: `Period@duration`, else the
/// presentation's end (ISO/IEC 23009-1 5.3.2); `None` when neither says.
fn period_end_ms(root: &Node, period: &Node) -> Result<Option<i64>, &'static str> {
    let ms = |v: Option<&str>| {
        v.map(|v| xs_duration_ms(v.trim()).map_err(|_| FORMAT_NOT_HERE))
            .transpose()
    };
    if let Some(duration) = ms(period.attr("duration"))? {
        return Ok(Some(duration));
    }
    let start = ms(period.attr("start"))?.unwrap_or(0);
    Ok(ms(root.attr("mediaPresentationDuration"))?.map(|end| end - start))
}

/// A level's base: its first `BaseURL` resolved against `parent`.
fn base_of(parent: Option<&Url>, level: &Node) -> Result<Option<Url>, &'static str> {
    let Some(base) = level.first("BaseURL") else {
        return Ok(None);
    };
    let text = base.text.trim();
    let url = match parent {
        Some(parent) => parent.join(text),
        None => Url::parse(text),
    };
    url.map(Some).map_err(|_| "a bad rewritten BaseURL")
}

/// The set's video and audio Representations; anything else (text, image,
/// trick mode) is left out, and WebM refuses the MPD.
fn renditions_of(
    set: &Node,
    parent_base: &Url,
    parent: &Template,
    out: &mut Vec<Rendition>,
) -> Result<(), &'static str> {
    let set_base = base_of(Some(parent_base), set)?.unwrap_or_else(|| parent_base.clone());
    let set_template = parent.under(set)?;
    let trick = |n: &Node| {
        n.all("EssentialProperty")
            .any(|p| p.attr("schemeIdUri") == Some(TRICK_MODE))
    };
    for rep in set.all("Representation") {
        let get = |name: &str| rep.attr(name).or_else(|| set.attr(name)).map(str::to_owned);
        let mime = get("mimeType").unwrap_or_default().to_ascii_lowercase();
        let kind = match mime.as_str() {
            "video/mp4" => Kind::Video,
            "audio/mp4" => Kind::Audio,
            "video/webm" | "audio/webm" => return Err(FORMAT_NOT_HERE),
            _ => continue,
        };
        if trick(set) || trick(rep) {
            continue;
        }
        let channels = rep
            .first("AudioChannelConfiguration")
            .or_else(|| set.first("AudioChannelConfiguration"))
            .and_then(|c| c.attr("value"))
            .map(str::to_owned);
        let label = rep
            .first("Label")
            .or_else(|| set.first("Label"))
            .map(|l| l.text.trim().to_owned());
        let sap = get("startWithSAP");
        let independent = matches!(sap.as_deref(), Some("1" | "2"))
            || get("segmentAlignment").as_deref() == Some("true");
        out.push(Rendition {
            kind,
            id: rep.attr("id").unwrap_or_default().to_owned(),
            bandwidth: rep
                .attr("bandwidth")
                .and_then(|b| b.trim().parse().ok())
                .unwrap_or(0),
            codecs: get("codecs")
                .map(|c| c.trim().to_owned())
                .filter(|c| !c.is_empty()),
            width: get("width"),
            height: get("height"),
            frame_rate: get("frameRate"),
            lang: get("lang"),
            label,
            channels,
            independent,
            base: base_of(Some(&set_base), rep)?.unwrap_or_else(|| set_base.clone()),
            template: set_template.under(rep)?,
        });
    }
    Ok(())
}

/// Audio renditions grouped by codec and channel count (Apple's authoring
/// rule: one group per codec and channel count), one per language, the
/// highest bandwidth of that language.
#[allow(clippy::type_complexity)]
fn audio_groups(list: &[Rendition]) -> Vec<((String, String), Vec<&Rendition>)> {
    let mut groups: Vec<((String, String), Vec<&Rendition>)> = Vec::new();
    for a in list.iter().filter(|r| r.kind == Kind::Audio) {
        let key = (
            a.codecs.clone().unwrap_or_default(),
            a.channels.clone().unwrap_or_default(),
        );
        let at = match groups.iter().position(|(k, _)| *k == key) {
            Some(at) => at,
            None => {
                groups.push((key, Vec::new()));
                groups.len() - 1
            }
        };
        let members = &mut groups[at].1;
        match members.iter().position(|m| m.lang == a.lang) {
            Some(i) if members[i].bandwidth < a.bandwidth => members[i] = a,
            Some(_) => {}
            None => members.push(a),
        }
    }
    groups
}

/// A Representation's media playlist (RFC 8216, version 7 for `EXT-X-MAP`).
fn media_playlist(
    r: &Rendition,
    end_ms: Option<i64>,
    children: &mut Children<'_>,
) -> Result<String, &'static str> {
    let t = &r.template;
    let timescale = t.number("timescale", 1)?;
    if timescale <= 0 {
        return Err(FORMAT_NOT_HERE);
    }
    let media = t.attr("media").ok_or(FORMAT_NOT_HERE)?;
    let init = t.attr("initialization").ok_or(FORMAT_NOT_HERE)?;
    let segments = segments(r, timescale, end_ms, media, children.alive)?;
    let longest = segments
        .iter()
        .map(|s| s.duration)
        .max()
        .ok_or(FORMAT_NOT_HERE)?;
    let target = ceil_div(longest, timescale).max(1);
    let init = expand(init, r, 0, 0)?;
    let init = r.base.join(&init).map_err(|_| FORMAT_NOT_HERE)?;
    let init = init_link(&init, children)?;
    let mut out = format!(
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:{target}\n#EXT-X-MEDIA-SEQUENCE:0\n\
         #EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-MAP:URI=\"{init}\"\n"
    );
    for (n, s) in segments.iter().enumerate() {
        checkpoint(n, children.alive)?;
        let seconds = s.duration as f64 / timescale as f64;
        emit(&mut out, &format!("#EXTINF:{seconds:.6},"))?;
        emit(&mut out, &s.url)?;
    }
    emit(&mut out, "#EXT-X-ENDLIST")?;
    Ok(out)
}

/// The Representation's segments in the Period: `@duration` gives
/// ceil(end / d) of them, the last trimmed to the end; a `SegmentTimeline`
/// is expanded (`r="-1"` to the next `t` or the end), each `S` placed at
/// `t` minus `presentationTimeOffset`, and what falls outside the Period is
/// dropped or trimmed. `$Number$` counts from `startNumber` (default 1).
fn segments(
    r: &Rendition,
    timescale: i64,
    end_ms: Option<i64>,
    media: &str,
    alive: &dyn Fn() -> bool,
) -> Result<Vec<Segment>, &'static str> {
    let t = &r.template;
    let start_number = t.number("startNumber", 1)?;
    let offset = t.number("presentationTimeOffset", 0)?;
    let end = end_ms.map(|ms| (i128::from(ms) * i128::from(timescale) / 1000) as i64);
    let mut out = Vec::new();
    let mut push =
        |number: i64, time: i64, start: i64, duration: i64| -> Result<bool, &'static str> {
            checkpoint(out.len(), alive)?;
            if out.len() as i64 >= MAX_SEGMENTS {
                return Err(FORMAT_NOT_HERE);
            }
            // From the Period's start, in the timescale.
            let (from, to) = (start - offset, start - offset + duration);
            if to <= 0 {
                return Ok(true);
            }
            if end.is_some_and(|end| from >= end) {
                return Ok(false);
            }
            let to = end.map_or(to, |end| to.min(end));
            let url = r
                .base
                .join(&expand(media, r, number, time)?)
                .map_err(|_| FORMAT_NOT_HERE)?;
            out.push(Segment {
                url: url.into(),
                duration: to - from.max(0),
            });
            Ok(true)
        };
    match &t.timeline {
        None => {
            let d = t.number("duration", 0)?;
            let end = end.filter(|end| *end > 0).ok_or(FORMAT_NOT_HERE)?;
            if d <= 0 {
                return Err(FORMAT_NOT_HERE);
            }
            let count = ceil_div(end, d);
            for k in 0..count {
                push(start_number + k, offset + k * d, offset + k * d, d)?;
            }
        }
        Some(timeline) => {
            let mut at = 0i64;
            let mut number = start_number;
            'list: for (i, &(t, d, repeat)) in timeline.iter().enumerate() {
                if let Some(t) = t {
                    at = t;
                }
                let repeats = if repeat >= 0 {
                    repeat
                } else {
                    // To the next `S`'s `t`, else to the Period's end.
                    let until = match timeline.get(i + 1).and_then(|next| next.0) {
                        Some(next) => next,
                        None => offset + end.ok_or(FORMAT_NOT_HERE)?,
                    };
                    ceil_div(until - at, d) - 1
                };
                for _ in 0..=repeats.min(MAX_SEGMENTS) {
                    if !push(number, at, at, d)? {
                        break 'list;
                    }
                    at += d;
                    number += 1;
                }
            }
        }
    }
    if out.is_empty() {
        return Err(FORMAT_NOT_HERE);
    }
    Ok(out)
}

/// A template with its identifiers filled in (ISO/IEC 23009-1 5.3.9.4.4):
/// `$$`, `$RepresentationID$`, `$Bandwidth$`, `$Number$` and `$Time$`, the
/// last three with an optional `%0Nd` width.
fn expand(template: &str, r: &Rendition, number: i64, time: i64) -> Result<String, &'static str> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let close = after.find('$').ok_or(FORMAT_NOT_HERE)?;
        let (name, width) = match after[..close].split_once('%') {
            Some((name, format)) => {
                let digits = format
                    .strip_prefix('0')
                    .unwrap_or(format)
                    .strip_suffix('d')
                    .ok_or(FORMAT_NOT_HERE)?;
                (name, digits.parse::<usize>().map_err(|_| FORMAT_NOT_HERE)?)
            }
            None => (&after[..close], 1),
        };
        let value = match name {
            "" => "$".to_owned(),
            "RepresentationID" => r.id.clone(),
            "Bandwidth" => format!("{:0width$}", r.bandwidth),
            "Number" => format!("{number:0width$}"),
            "Time" => format!("{time:0width$}"),
            _ => return Err(FORMAT_NOT_HERE),
        };
        out.push_str(&value);
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// An init section's URL as an `/c/` resource in the init role, so its
/// `hev1` entries are served as `hvc1`: a `/t/` URL goes back to its
/// upstream URL first; one already on `/c/` is kept.
fn init_link(url: &Url, children: &mut Children<'_>) -> Result<String, &'static str> {
    let s = url.as_str();
    let Some(rest) = s.strip_prefix(children.templates) else {
        return Ok(s.to_owned());
    };
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let (id, tail) = rest.split_once('/').ok_or("a bad template URL")?;
    let id = id.parse().map_err(|_| "a bad template URL")?;
    let (origin, prefix, stored) = children.table.template(id).ok_or("a bad template URL")?;
    let upstream = template_url(&origin, &prefix, stored.as_deref(), tail, url.query())
        .ok_or("a bad template URL")?;
    children.link(&upstream, Role::Init)
}

/// An HLS `CODECS` name: Apple decodes HEVC and Dolby Vision only from
/// `hvc1` and `dvh1` entries, which the init role serves the `hev1` and
/// `dvhe` ones as.
fn hls_codec(codec: &str) -> String {
    for (from, to) in [("hev1.", "hvc1."), ("dvhe.", "dvh1.")] {
        if let Some(rest) = codec.strip_prefix(from) {
            return format!("{to}{rest}");
        }
    }
    codec.to_owned()
}

/// A codec name that is safe inside a quoted string.
fn codec_ok(codec: &str) -> bool {
    !codec.is_empty()
        && codec
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_')
}

fn digits(value: &str) -> bool {
    !value.is_empty() && value.len() <= 9 && value.bytes().all(|b| b.is_ascii_digit())
}

/// `25`, `25/1` or `24000/1001` as frames per second.
fn frame_rate(value: &str) -> Option<f64> {
    let (num, den) = value.trim().split_once('/').unwrap_or((value.trim(), "1"));
    let (num, den) = (num.parse::<u32>().ok()?, den.parse::<u32>().ok()?);
    (num > 0 && den > 0).then(|| f64::from(num) / f64::from(den))
}

/// A BCP 47 tag as `LANGUAGE` may hold it (RFC 8216 4.3.4.1); anything else
/// is left out.
fn language_ok(lang: &str) -> bool {
    (1..=35).contains(&lang.len())
        && lang.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && lang != "und"
}

/// A rendition's name from its `Label`, else its language, with no quote,
/// line break or control character (RFC 8216 4.2), at most `MAX_NAME`.
fn rendition_name(a: &Rendition) -> String {
    let raw = a
        .label
        .clone()
        .filter(|l| !l.is_empty())
        .or_else(|| a.lang.clone().filter(|l| language_ok(l)))
        .unwrap_or_else(|| "Audio".into());
    let clean: String = raw
        .chars()
        .filter(|c| *c != '"' && !c.is_control())
        .take(MAX_NAME)
        .collect();
    if clean.trim().is_empty() {
        "Audio".into()
    } else {
        clean.trim().to_owned()
    }
}

/// `name`, made unique in group `group` (RFC 8216 4.3.4.1.1).
fn unique(names: &mut Vec<String>, group: usize, name: String) -> String {
    let mut out = name.clone();
    let mut n = 2;
    while names.contains(&format!("{group}:{out}")) {
        out = format!("{name} {n}");
        n += 1;
    }
    names.push(format!("{group}:{out}"));
    out
}

/// `a / b` rounded up, for `b > 0`; 0 for `a <= 0`.
fn ceil_div(a: i64, b: i64) -> i64 {
    if a <= 0 { 0 } else { (a - 1) / b + 1 }
}

fn emit(out: &mut String, line: &str) -> Result<(), &'static str> {
    if out.len() + line.len() + 1 > OUTPUT_MAX {
        return Err("output too large");
    }
    out.push_str(line);
    out.push('\n');
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{COPY_PROTECTED, Table, rewrite_dash};
    use super::*;

    const CORE: &str = "http://127.0.0.1:7777/";
    const PREFIX: &str = "http://127.0.0.1:7777/c/token/";
    const TEMPLATES: &str = "http://127.0.0.1:7777/t/token/";
    const RENDITIONS: &str = "http://127.0.0.1:7777/h/token/";

    /// `body` rewritten for the core from `https://cdn.example/dash/main.mpd`
    /// and converted, with the table it filled.
    fn convert(body: &str) -> (Result<Converted, &'static str>, Table) {
        let mut table = Table::default();
        let mut children = Children {
            core: CORE,
            prefix: PREFIX,
            templates: TEMPLATES,
            table: &mut table,
            alive: &|| true,
        };
        let base = Url::parse("https://cdn.example/dash/main.mpd").unwrap();
        let out = rewrite_dash(body.as_bytes(), &base, &mut children)
            .and_then(|mpd| dash_to_hls(&mpd.text, RENDITIONS, &mut children));
        (out, table)
    }

    fn accepted(body: &str) -> (Converted, Table) {
        let (out, table) = convert(body);
        (out.unwrap_or_else(|e| panic!("refused: {e}")), table)
    }

    fn corpus(name: &str) -> String {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/corpus/dash/in/");
        std::fs::read_to_string(format!("{dir}{name}")).unwrap()
    }

    /// `inner` in a static MPD of `duration`.
    fn mpd(duration: &str, inner: &str) -> String {
        format!(
            "<?xml version=\"1.0\"?><MPD xmlns=\"urn:mpeg:dash:schema:mpd:2011\" type=\"static\" \
             mediaPresentationDuration=\"{duration}\">{inner}</MPD>"
        )
    }

    /// A video set of one Representation under `template`.
    fn video(template: &str) -> String {
        format!(
            "<Period><AdaptationSet mimeType=\"video/mp4\" segmentAlignment=\"true\">{template}\
             <Representation id=\"v\" codecs=\"avc1.64001f\" bandwidth=\"1000\" width=\"640\" height=\"360\"/>\
             </AdaptationSet></Period>"
        )
    }

    /// The `EXTINF` lengths and URI lines of a media playlist.
    fn segments(media: &str) -> (Vec<f64>, Vec<&str>) {
        let lengths = media
            .lines()
            .filter_map(|l| l.strip_prefix("#EXTINF:"))
            .map(|l| l.trim_end_matches(',').parse().unwrap())
            .collect();
        let uris = media.lines().filter(|l| !l.starts_with('#')).collect();
        (lengths, uris)
    }

    /// The real MovieBox South Park row (2026-11-05): ffmpeg's `-f dash`,
    /// `$Number%05d$` with a fixed `@duration`, HEVC as `hev1`, AAC.
    #[test]
    fn the_number_and_duration_shape_plays_as_hls() {
        let (hls, mut table) = accepted(&corpus("moviebox_number.mpd"));
        let master = &hls.master;
        assert!(
            master.starts_with("#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-INDEPENDENT-SEGMENTS\n"),
            "{master}"
        );
        assert!(master.contains(
            "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a0\",NAME=\"Audio\",DEFAULT=YES,AUTOSELECT=YES,CHANNELS=\"2\",URI=\"http://127.0.0.1:7777/h/token/3/audio.m3u8\""
        ), "{master}");
        assert!(master.contains(
            "#EXT-X-STREAM-INF:BANDWIDTH=629820,RESOLUTION=1554x1080,FRAME-RATE=25.000,CODECS=\"hvc1.1.6.L120.90,mp4a.40.2\",AUDIO=\"a0\"\nhttp://127.0.0.1:7777/h/token/0/video.m3u8"
        ), "{master}");
        assert_eq!(master.matches("#EXT-X-STREAM-INF").count(), 3);
        assert_eq!(hls.media.len(), 4);
        // 24:25.9 in 5 s segments: 294, the last 0.9 s.
        let video = &hls.media[0];
        let (lengths, uris) = segments(video);
        assert_eq!(lengths.len(), 294);
        assert_eq!(lengths[0], 5.0);
        assert_eq!(*lengths.last().unwrap(), 0.9);
        assert!(
            video.contains("#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:5\n"),
            "{video}"
        );
        assert!(video.contains("#EXT-X-PLAYLIST-TYPE:VOD\n"));
        assert!(video.ends_with("#EXT-X-ENDLIST\n"));
        assert_eq!(
            uris[0],
            "http://127.0.0.1:7777/t/token/0/chunk-stream0-00001.m4s"
        );
        assert_eq!(
            uris[293],
            "http://127.0.0.1:7777/t/token/0/chunk-stream0-00294.m4s"
        );
        // The init is a `/c/` resource in the init role, for the relabel.
        let map = video
            .lines()
            .find_map(|l| l.strip_prefix("#EXT-X-MAP:URI=\""))
            .unwrap();
        let id = map.strip_prefix(PREFIX).unwrap().split('/').next().unwrap();
        let (url, role) = table.get(id.parse().unwrap()).unwrap();
        assert_eq!(url.as_str(), "https://cdn.example/dash/init-stream0.m4s");
        assert_eq!(role, Role::Init);
        let (_, audio) = segments(&hls.media[3]);
        assert_eq!(
            audio[0],
            "http://127.0.0.1:7777/t/token/0/chunk-stream3-00001.m4s"
        );
    }

    /// The real MovieBox Parasite row (2026-11-05): a `SegmentTimeline` with
    /// repeats and `$Number$`; the lengths add up to the presentation.
    #[test]
    fn the_timeline_shape_adds_up_to_the_presentation() {
        let (hls, _) = accepted(&corpus("moviebox_timeline.mpd"));
        for media in &hls.media {
            let (lengths, uris) = segments(media);
            let total: f64 = lengths.iter().sum();
            assert!((total - 7920.7).abs() < 0.01, "{total}");
            assert_eq!(lengths.len(), uris.len());
            assert!(uris[0].ends_with("-00001.m4s"), "{}", uris[0]);
            let last = format!("-{:05}.m4s", uris.len());
            assert!(uris.last().unwrap().ends_with(&last));
            let target: f64 = media
                .lines()
                .find_map(|l| l.strip_prefix("#EXT-X-TARGETDURATION:"))
                .unwrap()
                .parse()
                .unwrap();
            assert!(lengths.iter().all(|l| *l <= target));
        }
    }

    #[test]
    fn numbers_times_and_offsets() {
        // `startNumber="0"`, a 90 kHz timescale and `$Time$` with an offset:
        // the first S ends at the offset and is dropped, the next is placed
        // at 0, and the last one is cut at the end.
        let template = "<SegmentTemplate timescale=\"90000\" presentationTimeOffset=\"180000\" \
                        startNumber=\"0\" initialization=\"i.mp4\" media=\"s-$Time$-$Number%03d$.m4s\">\
                        <SegmentTimeline><S t=\"0\" d=\"180000\"/><S d=\"360000\" r=\"-1\"/></SegmentTimeline>\
                        </SegmentTemplate>";
        let (hls, _) = accepted(&mpd("PT10S", &video(template)));
        let (lengths, uris) = segments(&hls.media[0]);
        assert_eq!(lengths, [4.0, 4.0, 2.0]);
        assert_eq!(
            uris,
            [
                "http://127.0.0.1:7777/t/token/0/s-180000-001.m4s",
                "http://127.0.0.1:7777/t/token/0/s-540000-002.m4s",
                "http://127.0.0.1:7777/t/token/0/s-900000-003.m4s",
            ]
        );
        // `@duration` in a 48 kHz timescale.
        let template = "<SegmentTemplate timescale=\"48000\" duration=\"96000\" startNumber=\"7\" \
                        initialization=\"i.mp4\" media=\"$RepresentationID$/$Bandwidth$/$Number$.m4s\"/>";
        let (hls, _) = accepted(&mpd("PT5S", &video(template)));
        let (lengths, uris) = segments(&hls.media[0]);
        assert_eq!(lengths, [2.0, 2.0, 1.0]);
        assert_eq!(uris[2], "http://127.0.0.1:7777/t/token/0/v/1000/9.m4s");
    }

    #[test]
    fn audio_groups_by_codec_and_channels() {
        let audio = |id: &str, lang: &str, codec: &str, ch: &str, bw: u32| {
            format!(
                "<AdaptationSet mimeType=\"audio/mp4\" lang=\"{lang}\" codecs=\"{codec}\">\
                 <AudioChannelConfiguration schemeIdUri=\"urn:mpeg:dash:23003:3:audio_channel_configuration:2011\" value=\"{ch}\"/>\
                 <Representation id=\"{id}\" bandwidth=\"{bw}\"/></AdaptationSet>"
            )
        };
        let template = "<SegmentTemplate duration=\"2\" initialization=\"$RepresentationID$/i.mp4\" \
                        media=\"$RepresentationID$/$Number$.m4s\"/>";
        let period = format!(
            "<Period>{template}<AdaptationSet mimeType=\"video/mp4\"><Representation id=\"v\" \
             codecs=\"avc1.64001f\" bandwidth=\"1000\"/></AdaptationSet>{}{}{}{}</Period>",
            audio("en1", "en", "mp4a.40.2", "2", 64),
            audio("en2", "en", "mp4a.40.2", "2", 128),
            audio("tr", "tr", "mp4a.40.2", "2", 96),
            audio("en6", "en", "ac-3", "6", 384),
        );
        let (hls, _) = accepted(&mpd("PT4S", &period));
        let master = &hls.master;
        assert!(!master.contains("INDEPENDENT"), "{master}");
        // The higher English AAC, Turkish, and the 5.1 group of its own.
        assert_eq!(hls.media.len(), 4);
        assert!(master.contains("GROUP-ID=\"a0\",LANGUAGE=\"en\",NAME=\"en\",DEFAULT=YES"));
        assert!(master.contains("GROUP-ID=\"a0\",LANGUAGE=\"tr\",NAME=\"tr\",DEFAULT=NO"));
        assert!(master.contains(
            "GROUP-ID=\"a1\",LANGUAGE=\"en\",NAME=\"en\",DEFAULT=YES,AUTOSELECT=YES,CHANNELS=\"6\""
        ));
        assert!(
            master.contains("BANDWIDTH=1128,CODECS=\"avc1.64001f,mp4a.40.2\",AUDIO=\"a0\""),
            "{master}"
        );
        assert!(
            master.contains("BANDWIDTH=1384,CODECS=\"avc1.64001f,ac-3\",AUDIO=\"a1\""),
            "{master}"
        );
        let (_, uris) = segments(&hls.media[1]);
        assert!(uris[0].ends_with("/en2/1.m4s"), "{uris:?}");
    }

    /// Text from the MPD never ends a quoted string or a line (RFC 8216 4.2).
    #[test]
    fn names_and_languages_cannot_inject() {
        let period = "<Period><SegmentTemplate duration=\"2\" initialization=\"i.mp4\" media=\"$RepresentationID$-$Number$.m4s\"/>\
                      <AdaptationSet mimeType=\"video/mp4\"><Representation id=\"v\" bandwidth=\"1\"/></AdaptationSet>\
                      <AdaptationSet mimeType=\"audio/mp4\" lang=\"en&quot;,DEFAULT=YES\">\
                      <Label>x&quot;&#10;#EXT-X-ENDLIST&#13;,URI=&quot;https://evil.example/</Label>\
                      <Representation id=\"a\" bandwidth=\"1\"/></AdaptationSet></Period>";
        let (hls, _) = accepted(&mpd("PT2S", period));
        let media = hls
            .master
            .lines()
            .find(|l| l.starts_with("#EXT-X-MEDIA"))
            .unwrap();
        assert_eq!(
            media,
            "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a0\",NAME=\"x#EXT-X-ENDLIST,URI=https://evil.example/\",\
             DEFAULT=YES,AUTOSELECT=YES,URI=\"http://127.0.0.1:7777/h/token/1/audio.m3u8\""
        );
        assert_eq!(
            hls.master.lines().filter(|l| l.contains("ENDLIST")).count(),
            1
        );
    }

    /// Every converted playlist passes the output check: a rendition naming
    /// anything off the core (a base the rewrite would never have kept) is
    /// refused, never served.
    #[test]
    fn a_rendition_off_the_core_is_refused() {
        let mut table = Table::default();
        let mut children = Children {
            core: CORE,
            prefix: PREFIX,
            templates: TEMPLATES,
            table: &mut table,
            alive: &|| true,
        };
        let mpd = mpd(
            "PT4S",
            "<BaseURL>https://evil.example/</BaseURL><Period><AdaptationSet mimeType=\"video/mp4\">\
             <SegmentTemplate duration=\"2\" initialization=\"http://127.0.0.1:7777/c/token/0/i.mp4\" \
             media=\"$Number$.m4s\"/><Representation id=\"v\" bandwidth=\"1\"/></AdaptationSet></Period>",
        );
        let out = dash_to_hls(&mpd, RENDITIONS, &mut children);
        assert_eq!(out.err(), Some("the output check"));
    }

    /// Every shape phase 1 does not take gives the format words, so the
    /// version reads as one that does not play here.
    #[test]
    fn other_shapes_are_refused_in_plain_words() {
        let template =
            "<SegmentTemplate duration=\"2\" initialization=\"i.mp4\" media=\"$Number$.m4s\"/>";
        let refused = [
            mpd("PT4S", "<Period/>"),
            mpd("PT4S", &format!("{}{}", video(template), video(template))),
            mpd(
                "PT4S",
                &video(
                    "<SegmentBase indexRange=\"0-99\"><Initialization range=\"100-199\"/></SegmentBase>",
                ),
            ),
            mpd(
                "PT4S",
                &video("<SegmentList duration=\"2\"><SegmentURL media=\"a.m4s\"/></SegmentList>"),
            ),
            mpd("PT4S", &video(template)).replace("video/mp4", "video/webm"),
            mpd("PT4S", &video(template)).replace("type=\"static\"", "type=\"dynamic\""),
            // No end: neither `Period@duration` nor the presentation's.
            mpd("PT4S", &video(template)).replace(" mediaPresentationDuration=\"PT4S\"", ""),
            mpd(
                "PT4S",
                &video("<SegmentTemplate initialization=\"i.mp4\" media=\"$Number$.m4s\"/>"),
            ),
            mpd(
                "PT4S",
                &video("<SegmentTemplate media=\"$Number$.m4s\" duration=\"2\"/>"),
            ),
        ];
        for body in &refused {
            assert_eq!(convert(body).0.err(), Some(FORMAT_NOT_HERE), "{body}");
        }
        let protected = mpd(
            "PT4S",
            "<Period><AdaptationSet mimeType=\"video/mp4\"><ContentProtection \
             schemeIdUri=\"urn:mpeg:dash:mp4protection:2011\" value=\"cenc\"/></AdaptationSet></Period>",
        );
        assert_eq!(convert(&protected).0.err(), Some(COPY_PROTECTED));
    }

    #[test]
    fn codecs_name_what_apple_decodes() {
        assert_eq!(hls_codec("hev1.1.6.L120.90"), "hvc1.1.6.L120.90");
        assert_eq!(hls_codec("dvhe.05.06"), "dvh1.05.06");
        assert_eq!(hls_codec("avc1.640028"), "avc1.640028");
        assert_eq!(
            frame_rate("24000/1001").map(|r| (r * 1000.0).round()),
            Some(23976.0)
        );
        assert!(!language_ok("und") && !language_ok("en\"") && language_ok("pt-BR"));
    }
}

#[cfg(test)]
mod corpus {
    use super::super::{Table, rewrite_dash};
    use super::*;

    /// Every input of `tests/corpus/dash/in` the conversion takes, as the
    /// core at `http://127.0.0.1:7777/` with a fixed token serves it: the
    /// master, then each media playlist after a `## media n` line, in
    /// `tests/corpus/dash/hls/{name}.txt`. `MQ_CORPUS_WRITE=1` writes them;
    /// otherwise any difference fails, and so does a refused input with one.
    #[test]
    fn dash_as_hls_matches_its_corpus() {
        let write = std::env::var_os("MQ_CORPUS_WRITE").is_some();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/dash");
        let token = "0f".repeat(32);
        let mut converted = Vec::new();
        let mut names: Vec<String> = std::fs::read_dir(dir.join("in"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| n.ends_with(".mpd"))
            .collect();
        names.sort();
        for name in &names {
            let body = std::fs::read(dir.join("in").join(name)).unwrap();
            let (prefix, templates) = (
                format!("http://127.0.0.1:7777/c/{token}/"),
                format!("http://127.0.0.1:7777/t/{token}/"),
            );
            let mut table = Table::default();
            let mut children = Children {
                core: "http://127.0.0.1:7777/",
                prefix: &prefix,
                templates: &templates,
                table: &mut table,
                alive: &|| true,
            };
            let base = Url::parse(&format!("https://cdn.example/dash/{name}")).unwrap();
            let renditions = format!("http://127.0.0.1:7777/h/{token}/");
            let out = rewrite_dash(&body, &base, &mut children)
                .and_then(|mpd| dash_to_hls(&mpd.text, &renditions, &mut children));
            let Ok(hls) = out else { continue };
            let mut text = hls.master;
            for (n, media) in hls.media.iter().enumerate() {
                text.push_str(&format!("## media {n}\n{media}"));
            }
            let golden = dir.join("hls").join(name.replace(".mpd", ".txt"));
            if write {
                std::fs::create_dir_all(dir.join("hls")).unwrap();
                std::fs::write(&golden, &text).unwrap();
            } else {
                let kept = std::fs::read_to_string(&golden).unwrap_or_default();
                assert_eq!(kept, text, "{name} differs from its golden output");
            }
            converted.push(name.replace(".mpd", ".txt"));
        }
        assert!(converted.contains(&"moviebox_number.txt".to_owned()));
        let mut kept: Vec<String> = std::fs::read_dir(dir.join("hls"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        kept.sort();
        assert_eq!(kept, converted, "a golden output with no converted input");
    }
}
