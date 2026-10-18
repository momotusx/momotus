//! DASH: the BaseURL rewrite with an output allowlist (plans/c21-c23-loopback.md
//! 3.3). The core reads the MPD's events and writes a document of its own
//! from them: only the elements and attributes of the output contract, every
//! one unprefixed in DASH's namespace, values decoded before they are judged
//! and escaped again on the way out, never input bytes, CDATA, comments or
//! processing instructions.

use super::{Children, OUTPUT_MAX, Rewritten, Role, checkpoint};
use mq_http::Url;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::name::{Namespace, ResolveResult};
use quick_xml::reader::NsReader;
use quick_xml::writer::Writer;
use url::Position;

const DASH_NS: &[u8] = b"urn:mpeg:dash:schema:mpd:2011";
const DVB_NS: &[u8] = b"urn:dvb:dash:dash-extensions:2014-1";
const SCTE_NS: &[u8] = b"urn:scte:dash:scte214-extensions";

/// Nesting deeper than this refuses the MPD.
const MAX_DEPTH: usize = 32;
/// Base alternatives one level may resolve to.
const MAX_BASES: usize = 16;

const MPD: &[&str] = &[
    "id",
    "profiles",
    "type",
    "availabilityStartTime",
    "availabilityEndTime",
    "publishTime",
    "mediaPresentationDuration",
    "minimumUpdatePeriod",
    "minBufferTime",
    "timeShiftBufferDepth",
    "suggestedPresentationDelay",
    "maxSegmentDuration",
    "maxSubsegmentDuration",
];
const PERIOD: &[&str] = &["id", "start", "duration", "bitstreamSwitching"];
/// ISO/IEC 23009-1's own attributes of AdaptationSet, Representation and
/// SubRepresentation, and of RepresentationBase.
const REPRESENTATION: &[&str] = &[
    "id",
    "group",
    "lang",
    "contentType",
    "par",
    "minBandwidth",
    "maxBandwidth",
    "minWidth",
    "maxWidth",
    "minHeight",
    "maxHeight",
    "minFrameRate",
    "maxFrameRate",
    "segmentAlignment",
    "subsegmentAlignment",
    "subsegmentStartsWithSAP",
    "bitstreamSwitching",
    "bandwidth",
    "qualityRanking",
    "dependencyId",
    "associationId",
    "associationType",
    "mediaStreamStructureId",
    "level",
    "dependencyLevel",
    "contentComponent",
    "profiles",
    "width",
    "height",
    "sar",
    "frameRate",
    "audioSamplingRate",
    "mimeType",
    "segmentProfiles",
    "codecs",
    "maximumSAPPeriod",
    "startWithSAP",
    "maxPlayoutRate",
    "codingDependency",
    "scanType",
    "selectionPriority",
    "tag",
];
const DESCRIPTOR: &[&str] = &["id", "schemeIdUri", "value"];
const CONTENT_COMPONENT: &[&str] = &[
    "id",
    "schemeIdUri",
    "value",
    "lang",
    "contentType",
    "par",
    "tag",
];
const SEGMENTS: &[&str] = &[
    "timescale",
    "presentationTimeOffset",
    "presentationDuration",
    "indexRange",
    "indexRangeExact",
    "availabilityTimeOffset",
    "availabilityTimeComplete",
    "duration",
    "startNumber",
    "endNumber",
];
const SEGMENT_REFERENCES: &[&str] = &["media", "index", "initialization", "bitstreamSwitching"];
const BASE_URL: &[&str] = &[
    "serviceLocation",
    "availabilityTimeOffset",
    "availabilityTimeComplete",
];

/// What a kept element's text is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Text {
    /// Ignored (whitespace between children).
    None,
    /// `BaseURL`: rewritten (rules 2 to 4).
    Base,
    /// `Location`: rewritten (rule 6).
    Location,
    /// `Label`: kept as text.
    Plain,
}

/// A kept element: its plain attributes, its URL-bearing ones, its text.
struct Kept {
    name: &'static str,
    plain: &'static [&'static str],
    references: &'static [&'static str],
    text: Text,
}

const fn kept(name: &'static str, plain: &'static [&'static str]) -> Kept {
    Kept {
        name,
        plain,
        references: &[],
        text: Text::None,
    }
}

/// The output contract (3.3): every element not here goes with its subtree.
const ELEMENTS: &[Kept] = &[
    kept("MPD", MPD),
    kept("Period", PERIOD),
    kept("AdaptationSet", REPRESENTATION),
    kept("Representation", REPRESENTATION),
    kept("SubRepresentation", REPRESENTATION),
    kept("ContentComponent", CONTENT_COMPONENT),
    kept("Role", DESCRIPTOR),
    kept("Accessibility", DESCRIPTOR),
    kept("Rating", DESCRIPTOR),
    kept("Viewpoint", DESCRIPTOR),
    kept("EssentialProperty", DESCRIPTOR),
    kept("SupplementalProperty", DESCRIPTOR),
    kept("AudioChannelConfiguration", DESCRIPTOR),
    kept("FramePacking", DESCRIPTOR),
    kept("InbandEventStream", DESCRIPTOR),
    Kept {
        text: Text::Plain,
        ..kept("Label", &["id", "lang"])
    },
    kept("ServiceDescription", &["id"]),
    kept("Latency", &["min", "max", "target", "referenceId"]),
    kept("PlaybackRate", &["min", "max"]),
    Kept {
        references: SEGMENT_REFERENCES,
        ..kept("SegmentBase", SEGMENTS)
    },
    Kept {
        references: SEGMENT_REFERENCES,
        ..kept("SegmentList", SEGMENTS)
    },
    Kept {
        references: SEGMENT_REFERENCES,
        ..kept("SegmentTemplate", SEGMENTS)
    },
    kept("SegmentTimeline", &[]),
    kept("S", &["t", "n", "d", "r", "k"]),
    Kept {
        references: &["media", "index"],
        ..kept("SegmentURL", &["mediaRange", "indexRange"])
    },
    Kept {
        references: &["sourceURL"],
        ..kept("Initialization", &["range"])
    },
    Kept {
        references: &["sourceURL"],
        ..kept("RepresentationIndex", &["range"])
    },
    Kept {
        references: &["sourceURL"],
        ..kept("BitstreamSwitching", &["range"])
    },
    Kept {
        text: Text::Base,
        ..kept("BaseURL", BASE_URL)
    },
    Kept {
        text: Text::Location,
        ..kept("Location", &[])
    },
    kept("UTCTiming", &["schemeIdUri", "value"]),
];

/// Elements that consume the bases of the level they are in: a `BaseURL`
/// after one of them is refused.
const CONSUMERS: &[&str] = &[
    "Period",
    "AdaptationSet",
    "Representation",
    "SubRepresentation",
    "SegmentBase",
    "SegmentList",
    "SegmentTemplate",
];
/// `UTCTiming` schemes whose value is a date.
const UTC_DIRECT: &[&str] = &[
    "urn:mpeg:dash:utc:direct:2012",
    "urn:mpeg:dash:utc:direct:2014",
];
/// `UTCTiming` schemes whose value is a list of URLs.
const UTC_URLS: &[&str] = &[
    "urn:mpeg:dash:utc:http-xsdate:2012",
    "urn:mpeg:dash:utc:http-xsdate:2014",
    "urn:mpeg:dash:utc:http-iso:2012",
    "urn:mpeg:dash:utc:http-iso:2014",
    "urn:mpeg:dash:utc:http-ntp:2012",
    "urn:mpeg:dash:utc:http-ntp:2014",
];

/// One open kept element.
struct Frame {
    kept: &'static Kept,
    /// The parent level's bases (absolute upstream URLs) when this one
    /// started.
    inherited: Vec<Url>,
    /// This level's own `BaseURL`s, resolved.
    own: Vec<Url>,
    /// A child that consumes bases started: no `BaseURL` after it.
    consumed: bool,
    /// A `Representation`'s id.
    id: Option<String>,
    /// A relative template naming `$RepresentationID$` applies below.
    by_id: bool,
    /// The ids of the Representations seen below.
    ids: Vec<Option<String>>,
    /// Kept attributes, written at the end for text elements.
    attrs: Vec<(String, String)>,
    /// The text of a `BaseURL`, `Location` or `Label`.
    text: String,
}

impl Frame {
    /// This level's bases: its own, else its parent's.
    fn bases(&self) -> &[Url] {
        if self.own.is_empty() {
            &self.inherited
        } else {
            &self.own
        }
    }
}

/// Rewrites an MPD (`base`: its final URL after redirects) for the core, or
/// refuses it (rules 1 to 10 and the output contract of 3.3).
pub(crate) fn rewrite_dash(
    body: &[u8],
    base: &Url,
    children: &mut Children<'_>,
) -> Result<Rewritten, &'static str> {
    let alive = children.alive;
    checkpoint(0, alive)?;
    let text = std::str::from_utf8(body).map_err(|_| "not UTF-8")?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    children.table.begin();
    let mut rewrite = Rewrite {
        base,
        writer: Writer::new(Vec::new()),
        stack: Vec::new(),
        skip: 0,
        root: Root::Before,
        inserted: false,
    };
    rewrite.write(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;
    let mut reader = NsReader::from_str(text);
    for n in 1.. {
        checkpoint(n, alive)?;
        let event = reader.read_event().map_err(|_| "not well-formed XML")?;
        if rewrite.root == Root::After || matches!(event, Event::Eof) {
            break;
        }
        rewrite.event(&reader, event, children)?;
    }
    if rewrite.root != Root::After {
        return Err("no MPD");
    }
    let text = String::from_utf8(rewrite.writer.into_inner()).map_err(|_| "not UTF-8")?;
    Ok(Rewritten {
        text,
        target_duration: None,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Root {
    Before,
    In,
    After,
}

struct Rewrite<'b> {
    base: &'b Url,
    writer: Writer<Vec<u8>>,
    stack: Vec<Frame>,
    /// Inside a dropped element: how deep.
    skip: usize,
    root: Root,
    /// The MPD's own `BaseURL` was inserted (it had none).
    inserted: bool,
}

impl Rewrite<'_> {
    fn write(&mut self, event: Event<'_>) -> Result<(), &'static str> {
        self.writer
            .write_event(event)
            .map_err(|_| "the output could not be written")?;
        if self.writer.get_ref().len() > OUTPUT_MAX {
            return Err("output too large");
        }
        Ok(())
    }

    fn event(
        &mut self,
        reader: &NsReader<&[u8]>,
        event: Event<'_>,
        children: &mut Children<'_>,
    ) -> Result<(), &'static str> {
        match event {
            Event::DocType(_) => Err("a DOCTYPE"),
            Event::Start(e) => self.start(reader, &e, false, children),
            Event::Empty(e) => self.start(reader, &e, true, children),
            Event::End(_) => self.end(children),
            Event::Text(t) => self.text(&t.decode().map_err(|_| "not UTF-8")?),
            Event::CData(t) => self.text(&t.decode().map_err(|_| "not UTF-8")?),
            Event::GeneralRef(r) => {
                let resolved = match r.resolve_char_ref() {
                    Ok(Some(c)) => c.to_string(),
                    Ok(None) => match r.decode().map_err(|_| "not UTF-8")?.as_ref() {
                        "amp" => "&".into(),
                        "lt" => "<".into(),
                        "gt" => ">".into(),
                        "quot" => "\"".into(),
                        "apos" => "'".into(),
                        _ => return Err("an unknown entity"),
                    },
                    Err(_) => return Err("a bad character reference"),
                };
                self.text(&resolved)
            }
            // Comments, processing instructions and the declaration are
            // never written; inside a BaseURL they split nothing.
            _ => Ok(()),
        }
    }

    /// Text of the open element, kept only where it means something.
    fn text(&mut self, text: &str) -> Result<(), &'static str> {
        if self.skip > 0 {
            return Ok(());
        }
        if let Some(frame) = self.stack.last_mut()
            && frame.kept.text != Text::None
        {
            frame.text.push_str(text);
        }
        Ok(())
    }

    fn start(
        &mut self,
        reader: &NsReader<&[u8]>,
        e: &BytesStart<'_>,
        empty: bool,
        children: &mut Children<'_>,
    ) -> Result<(), &'static str> {
        if self.stack.len() + self.skip + 1 > MAX_DEPTH {
            return Err("nested too deep");
        }
        if self.skip > 0 {
            if !empty {
                self.skip += 1;
            }
            return Ok(());
        }
        let (ns, local) = reader.resolver().resolve_element(e.name());
        let dash = match ns {
            ResolveResult::Unbound => true,
            ResolveResult::Bound(Namespace(ns)) => ns == DASH_NS,
            ResolveResult::Unknown(_) => false,
        };
        let local = std::str::from_utf8(local.as_ref()).map_err(|_| "not UTF-8")?;
        if self.root == Root::Before {
            if !(dash && local == "MPD") {
                return Err("no MPD");
            }
            self.root = Root::In;
        }
        if dash && matches!(local, "PatchLocation" | "ContentProtection") {
            return Err(if local == "PatchLocation" {
                "a PatchLocation"
            } else {
                "protected content"
            });
        }
        // A text element takes no elements: refused, but in a label dropped.
        match self.stack.last().map(|parent| parent.kept.text) {
            Some(Text::Base | Text::Location) => {
                return Err("an element inside a BaseURL or Location");
            }
            Some(Text::Plain) => {
                self.skip = usize::from(!empty);
                return Ok(());
            }
            _ => {}
        }
        let Some(kept) = ELEMENTS.iter().find(|k| dash && k.name == local) else {
            if !empty {
                self.skip = 1;
            }
            return Ok(());
        };
        let attrs = self.attributes(reader, e, kept, children)?;
        self.open(kept, attrs, empty, children)
    }

    /// The kept attributes, judged and rewritten.
    fn attributes(
        &mut self,
        reader: &NsReader<&[u8]>,
        e: &BytesStart<'_>,
        kept: &'static Kept,
        children: &mut Children<'_>,
    ) -> Result<Vec<(String, String)>, &'static str> {
        let mut out = Vec::new();
        for attr in e.attributes() {
            let attr = attr.map_err(|_| "a malformed attribute")?;
            let key = attr.key.as_ref();
            if key == b"xmlns" || key.starts_with(b"xmlns:") {
                continue;
            }
            let (ns, local) = reader.resolver().resolve_attribute(attr.key);
            let local = std::str::from_utf8(local.as_ref()).map_err(|_| "not UTF-8")?;
            let value = attr
                .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                .map_err(|_| "a malformed attribute")?;
            let name = match ns {
                ResolveResult::Unbound => local.to_owned(),
                ResolveResult::Bound(Namespace(ns)) => {
                    if local == "href" {
                        return Err("an XLink href");
                    }
                    let dvb = ns == DVB_NS && kept.name == "BaseURL";
                    let scte = ns == SCTE_NS && REPRESENTATION == kept.plain;
                    match local {
                        "priority" | "weight" if dvb => format!("dvb:{local}"),
                        "supplementalCodecs" | "supplementalProfiles" if scte => {
                            format!("scte214:{local}")
                        }
                        _ => continue,
                    }
                }
                ResolveResult::Unknown(_) if local == "href" => return Err("an XLink href"),
                ResolveResult::Unknown(_) => continue,
            };
            if kept.name == "BaseURL" && name == "byteRange" {
                return Err("a BaseURL byteRange");
            }
            if kept.name == "UTCTiming" {
                out.push((name, value.into_owned()));
                continue;
            }
            if kept.references.contains(&name.as_str()) {
                let template = kept.name == "SegmentTemplate";
                let rewritten = self.reference(&value, template, children)?;
                if template && is_relative(value.trim()) && value.contains("$RepresentationID$") {
                    self.by_id()?;
                }
                out.push((name, rewritten));
                continue;
            }
            if name.contains(':') || kept.plain.contains(&name.as_str()) {
                if kept.plain == DESCRIPTOR || kept.plain == CONTENT_COMPONENT {
                    let v = value.trim();
                    if name == "value" && (v.contains("://") || v.starts_with("//")) {
                        return Err("a descriptor value that is a URL");
                    }
                }
                out.push((name, value.into_owned()));
            }
        }
        if kept.name == "UTCTiming" {
            return self.utc_timing(out, children);
        }
        Ok(out)
    }

    /// `UTCTiming` (rule 7): a direct date stays, URL lists are rewritten
    /// to the core, every other scheme drops the element (empty list).
    fn utc_timing(
        &mut self,
        attrs: Vec<(String, String)>,
        children: &mut Children<'_>,
    ) -> Result<Vec<(String, String)>, &'static str> {
        let get = |name: &str| {
            attrs
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.as_str())
        };
        let scheme = get("schemeIdUri").unwrap_or_default().trim().to_owned();
        let value = get("value").unwrap_or_default().to_owned();
        if UTC_DIRECT.contains(&scheme.as_str()) {
            return Ok(vec![
                ("schemeIdUri".into(), scheme),
                ("value".into(), value),
            ]);
        }
        if !UTC_URLS.contains(&scheme.as_str()) {
            return Ok(Vec::new());
        }
        let mut urls = Vec::new();
        for reference in value.split_whitespace() {
            let url = http_url(self.base.join(reference).ok())?;
            urls.push(children.link(&url, Role::Small)?);
        }
        Ok(vec![
            ("schemeIdUri".into(), scheme),
            ("value".into(), urls.join(" ")),
        ])
    }

    /// A segment reference (rule 5, 8, 10): relative stays, absolute goes to
    /// `/c/` or, a template with identifiers, to `/t/`.
    fn reference(
        &mut self,
        value: &str,
        template: bool,
        children: &mut Children<'_>,
    ) -> Result<String, &'static str> {
        let v = value.trim();
        if v.is_empty() {
            return Ok(String::new());
        }
        if v.starts_with('/') {
            return Err("a root-relative or scheme-relative reference");
        }
        if is_relative(v) {
            dot_segments(v)?;
            return Ok(v.to_owned());
        }
        let mut url = http_url(Url::parse(v).ok())?;
        url.set_fragment(None);
        if template && url.as_str().contains('$') {
            return template_link(&url, children);
        }
        children.link(&url, Role::Media)
    }

    /// A relative template naming `$RepresentationID$` (rule 9): every
    /// Representation it applies to needs a plain id.
    fn by_id(&mut self) -> Result<(), &'static str> {
        let Some(owner) = self.stack.last_mut() else {
            return Ok(());
        };
        if owner.kept.name == "Representation" {
            return check_id(owner.id.as_deref());
        }
        owner.by_id = true;
        owner.ids.iter().try_for_each(|id| check_id(id.as_deref()))
    }

    fn open(
        &mut self,
        kept: &'static Kept,
        attrs: Vec<(String, String)>,
        empty: bool,
        children: &mut Children<'_>,
    ) -> Result<(), &'static str> {
        if kept.name == "UTCTiming" && attrs.is_empty() {
            if !empty {
                self.skip = 1;
            }
            return Ok(());
        }
        let inherited = match self.stack.last() {
            Some(parent) => parent.bases().to_vec(),
            None => vec![self.base.clone()],
        };
        if let Some(parent) = self.stack.last_mut() {
            if kept.name == "BaseURL" && parent.consumed {
                return Err("a BaseURL after what uses the bases");
            }
            if CONSUMERS.contains(&kept.name) && !parent.consumed {
                parent.consumed = true;
                if parent.kept.name == "MPD" {
                    self.insert_base(children)?;
                }
            }
        }
        let id = attrs
            .iter()
            .find(|(n, _)| n == "id")
            .map(|(_, v)| v.clone());
        if kept.name == "Representation" {
            for frame in &mut self.stack {
                if frame.by_id {
                    check_id(id.as_deref())?;
                }
                frame.ids.push(id.clone());
            }
        }
        let frame = Frame {
            kept,
            inherited: if kept.name == "MPD" {
                vec![self.base.clone()]
            } else {
                inherited
            },
            own: Vec::new(),
            consumed: false,
            id,
            by_id: false,
            ids: Vec::new(),
            attrs,
            text: String::new(),
        };
        if kept.text != Text::None {
            // Written whole at the end, once the text is known.
            self.stack.push(frame);
            if empty {
                return self.end(children);
            }
            return Ok(());
        }
        let mut start = BytesStart::new(kept.name);
        if kept.name == "MPD" {
            start.push_attribute(("xmlns", "urn:mpeg:dash:schema:mpd:2011"));
            start.push_attribute(("xmlns:dvb", "urn:dvb:dash:dash-extensions:2014-1"));
            start.push_attribute(("xmlns:scte214", "urn:scte:dash:scte214-extensions"));
        }
        for (name, value) in &frame.attrs {
            start.push_attribute((name.as_str(), value.as_str()));
        }
        self.stack.push(frame);
        if empty && kept.name != "MPD" {
            self.write(Event::Empty(start))?;
            return self.close();
        }
        self.write(Event::Start(start))?;
        if empty {
            // An empty MPD gets its BaseURL too.
            return self.end(children);
        }
        Ok(())
    }

    fn end(&mut self, children: &mut Children<'_>) -> Result<(), &'static str> {
        if self.skip > 0 {
            self.skip -= 1;
            return Ok(());
        }
        let Some(frame) = self.stack.last() else {
            return Ok(());
        };
        let name = frame.kept.name;
        match frame.kept.text {
            Text::None => {
                if name == "MPD" {
                    self.insert_base(children)?;
                }
                self.write(Event::End(BytesEnd::new(name)))?;
            }
            Text::Base => self.base_url(children)?,
            Text::Location => {
                let text = frame.text.trim().to_owned();
                let url = http_url(self.base.join(&text).ok())?;
                let link = children.link(&url, Role::Dash)?;
                self.text_element(&link)?;
            }
            Text::Plain => {
                let text = frame.text.clone();
                self.text_element(&text)?;
            }
        }
        self.close()
    }

    /// Pops the frame just written.
    fn close(&mut self) -> Result<(), &'static str> {
        let frame = self.stack.pop().expect("an open frame");
        if frame.kept.name == "MPD" {
            self.root = Root::After;
        }
        Ok(())
    }

    /// Writes the top frame (a text element) with `text`.
    fn text_element(&mut self, text: &str) -> Result<(), &'static str> {
        let frame = self.stack.last().expect("an open frame");
        let name = frame.kept.name;
        let mut start = BytesStart::new(name);
        for (key, value) in &frame.attrs {
            start.push_attribute((key.as_str(), value.as_str()));
        }
        self.write(Event::Start(start))?;
        self.write(Event::Text(BytesText::new(text)))?;
        self.write(Event::End(BytesEnd::new(name)))
    }

    /// A `BaseURL` (rules 2 to 4, 8): rewritten to `/t/`, or kept relative
    /// under a level whose parent the player resolves on the core.
    fn base_url(&mut self, children: &mut Children<'_>) -> Result<(), &'static str> {
        let depth = self.stack.len();
        let text = self.stack[depth - 1].text.trim().to_owned();
        let level = &self.stack[depth - 2];
        let mpd = level.kept.name == "MPD";
        let parents = level.inherited.clone();
        let (written, resolved) = if mpd || !is_relative(&text) || text.starts_with('/') {
            let url = if mpd {
                http_url(self.base.join(&text).ok())?
            } else if text.starts_with('/') {
                // With the parent's origin, which every alternative shares.
                let first = parents.first().ok_or("no base")?;
                if parents.iter().any(|p| p.origin() != first.origin()) {
                    return Err("a root-relative BaseURL under bases on several origins");
                }
                http_url(first.join(&text).ok())?
            } else {
                http_url(Url::parse(&text).ok())?
            };
            (base_link(&url, children)?, vec![url])
        } else {
            dot_segments(&text)?;
            let joined = parents.iter().filter_map(|p| p.join(&text).ok()).collect();
            (text, joined)
        };
        let own = &mut self.stack[depth - 2].own;
        own.extend(resolved);
        if own.len() > MAX_BASES {
            return Err("more than 16 base alternatives");
        }
        self.text_element(&written)
    }

    /// The MPD's `BaseURL` when it has none: its own directory on the core,
    /// so nothing resolves against the main URL (rule 2).
    fn insert_base(&mut self, children: &mut Children<'_>) -> Result<(), &'static str> {
        if self.inserted {
            return Ok(());
        }
        self.inserted = true;
        let mpd = self.stack.first_mut().expect("the MPD");
        if !mpd.own.is_empty() {
            return Ok(());
        }
        let directory = self.base.join(".").map_err(|_| "an unresolvable URI")?;
        mpd.own.push(directory.clone());
        let link = base_link(&directory, children)?;
        self.write(Event::Start(BytesStart::new("BaseURL")))?;
        self.write(Event::Text(BytesText::new(&link)))?;
        self.write(Event::End(BytesEnd::new("BaseURL")))
    }
}

/// An http or https URL, or the MPD is refused (rule 10).
fn http_url(url: Option<Url>) -> Result<Url, &'static str> {
    let url = url.ok_or("an unresolvable URI")?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        _ => Err("a URI that is not http or https"),
    }
}

/// A relative reference as Media3's `UriUtil` sees one: no `:` before the
/// first `/`, `?` or `#`. Any other is absolute, whatever its scheme's
/// characters, and must parse as http or https (rule 10).
fn is_relative(reference: &str) -> bool {
    let end = reference.find(['/', '?', '#']).unwrap_or(reference.len());
    !reference[..end].contains(':')
}

/// A relative reference with a `.`, `..` or `%2e` segment refuses the MPD
/// (rule 8): at open, not as a 404 in the middle of playback.
fn dot_segments(reference: &str) -> Result<(), &'static str> {
    let path = &reference[..reference.find(['?', '#']).unwrap_or(reference.len())];
    // A backslash is a separator to OkHttp and the url crate.
    for segment in path.split(['/', '\\']) {
        let decoded = segment.replace("%2e", ".").replace("%2E", ".");
        if decoded == "." || decoded == ".." {
            return Err("a dot segment");
        }
    }
    Ok(())
}

/// A Representation id a relative `$RepresentationID$` template may carry
/// (rule 9): `A-Za-z0-9._~=+-`, never `.` or `..`.
fn check_id(id: Option<&str>) -> Result<(), &'static str> {
    let ok = id.is_some_and(|id| {
        !id.is_empty()
            && id != "."
            && id != ".."
            && id.chars().all(|c| {
                c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '~' | '=' | '+' | '-')
            })
    });
    if ok {
        Ok(())
    } else {
        Err("a Representation id a template cannot carry")
    }
}

/// `url` as `/t/{id}/{tail}` (rule 4): the entry is its origin, directory
/// and query, the tail its last segment (empty for a directory).
fn base_link(url: &Url, children: &mut Children<'_>) -> Result<String, &'static str> {
    let s = url.as_str();
    let path_start = url[..Position::BeforePath].len();
    let path_end = url[..Position::AfterPath].len();
    let cut = s[..path_end].rfind('/').map_or(path_end, |at| at + 1);
    let id =
        children
            .table
            .register_template(&s[..path_start], &s[path_start..cut], url.query())?;
    Ok(format!("{}{id}/{}", children.templates, &s[cut..path_end]))
}

/// An absolute template with identifiers as `/t/{id}/{tail}` (rule 5): the
/// prefix ends at the last `/` before the first `$`, the tail keeps the
/// identifiers (and the query); an identifier before the path refuses it.
fn template_link(url: &Url, children: &mut Children<'_>) -> Result<String, &'static str> {
    let s = url.as_str();
    let first = s.find('$').expect("a template");
    let path_start = url[..Position::BeforePath].len();
    let path_end = url[..Position::AfterPath].len();
    if first < path_start {
        return Err("a template identifier before its path");
    }
    let cut = s[..first.min(path_end)]
        .rfind('/')
        .map_or(path_end, |at| at + 1);
    let id = children
        .table
        .register_template(&s[..path_start], &s[path_start..cut], None)?;
    Ok(format!("{}{id}/{}", children.templates, &s[cut..]))
}

/// The upstream URL of `/t/{token}/{id}/{tail}` (3.3 Routes): the entry's
/// `origin`, `prefix` and stored `query`, the raw `tail` and the request's
/// `query`. `None` answers 404.
pub(crate) fn template_url(
    origin: &str,
    prefix: &str,
    stored: Option<&str>,
    tail: &str,
    query: Option<&str>,
) -> Option<Url> {
    if !tail_ok(tail) {
        return None;
    }
    let url = template_build(origin, prefix, stored, tail, query)?;
    template_kept(&url, origin, prefix).then_some(url)
}

/// A tail with no empty, `.` or `..` segment, no `\` and no `%2e`, `%2f`
/// or `%5c` in either case.
fn tail_ok(tail: &str) -> bool {
    let lower = tail.to_ascii_lowercase();
    !tail.contains('\\')
        && !["%2e", "%2f", "%5c"].iter().any(|bad| lower.contains(bad))
        && tail
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != "..")
}

/// The entry's origin with the path set to `prefix` and `tail`, never
/// joined, and the request's query if it has one, else the stored one.
fn template_build(
    origin: &str,
    prefix: &str,
    stored: Option<&str>,
    tail: &str,
    query: Option<&str>,
) -> Option<Url> {
    let mut url = Url::parse(origin).ok()?;
    url.set_path(&format!("{prefix}{tail}"));
    url.set_query(query.or(stored));
    Some(url)
}

/// The built URL is still on the entry's origin, under its prefix.
fn template_kept(url: &Url, origin: &str, prefix: &str) -> bool {
    &url[..Position::BeforePath] == origin && url.path().starts_with(prefix)
}

#[cfg(test)]
mod tests {
    use super::super::Table;
    use super::*;
    use quick_xml::Reader;

    const PREFIX: &str = "http://127.0.0.1:7777/c/token/";
    const TEMPLATES: &str = "http://127.0.0.1:7777/t/token/";
    const MAIN: &str = "https://cdn.example/v/main.mpd";

    /// `inner` as the children of a static MPD.
    fn mpd(inner: &str) -> String {
        format!(
            "<?xml version=\"1.0\"?><MPD xmlns=\"urn:mpeg:dash:schema:mpd:2011\" \
             xmlns:dvb=\"urn:dvb:dash:dash-extensions:2014-1\" type=\"static\" \
             mediaPresentationDuration=\"PT10S\">{inner}</MPD>"
        )
    }

    fn rewrite_bytes(body: &[u8], table: &mut Table) -> Result<String, &'static str> {
        let mut children = Children {
            core: "http://127.0.0.1:7777/",
            prefix: PREFIX,
            templates: TEMPLATES,
            table,
            alive: &|| true,
        };
        let base = Url::parse(MAIN).unwrap();
        rewrite_dash(body, &base, &mut children).map(|r| r.text)
    }

    fn rewrite(body: &str) -> Result<String, &'static str> {
        rewrite_bytes(body.as_bytes(), &mut Table::default())
    }

    /// The rewritten MPD, or the test fails with the reason.
    fn accepted(body: &str) -> (String, Table) {
        let mut table = Table::default();
        let out = rewrite_bytes(body.as_bytes(), &mut table);
        let out = out.unwrap_or_else(|e| panic!("refused: {e}"));
        (out, table)
    }

    /// Where the core fetches a URL the output names: a `/c/` id's URL, or
    /// a `/t/` entry with the tail; `None` where the core answers 404.
    fn upstream(table: &mut Table, url: &str) -> Option<String> {
        if let Some(rest) = url.strip_prefix(PREFIX) {
            let id = rest.split('/').next()?.parse().ok()?;
            return table.get(id).map(|(url, _)| url.to_string());
        }
        let rest = url.strip_prefix(TEMPLATES)?;
        let (id, tail) = rest.split_once('/')?;
        let (tail, query) = match tail.split_once('?') {
            Some((tail, query)) => (tail, Some(query)),
            None => (tail, None),
        };
        let (origin, prefix, stored) = table.template(id.parse().ok()?)?;
        let url = template_url(&origin, &prefix, stored.as_deref(), tail, query)?;
        Some(url.to_string())
    }

    /// What a player fetches for `chain` (a base and the references under
    /// it, each resolved on the one before), mapped to the origin.
    fn play(table: &mut Table, chain: &[&str]) -> Option<String> {
        let mut url = Url::parse(chain[0]).unwrap();
        for reference in &chain[1..] {
            url = url.join(reference).unwrap();
        }
        assert!(url.as_str().starts_with("http://127.0.0.1:7777/"), "{url}");
        upstream(table, url.as_str())
    }

    /// Every `element`'s text, or its `attr`, in the output, decoded.
    fn values(out: &str, element: &str, attr: Option<&str>) -> Vec<String> {
        let mut reader = Reader::from_str(out);
        let mut found = Vec::new();
        let mut inside: Option<String> = None;
        loop {
            match reader.read_event().unwrap() {
                Event::Start(e) | Event::Empty(e) if e.name().as_ref() == element.as_bytes() => {
                    match attr {
                        Some(attr) => {
                            if let Some(a) = e.try_get_attribute(attr).unwrap() {
                                found.push(
                                    a.normalized_value(XmlVersion::Implicit1_0)
                                        .unwrap()
                                        .into_owned(),
                                );
                            }
                        }
                        None => inside = Some(String::new()),
                    }
                }
                Event::Text(t) => {
                    if let Some(text) = &mut inside {
                        text.push_str(&t.decode().unwrap());
                    }
                }
                Event::GeneralRef(r) => {
                    if let Some(text) = &mut inside {
                        let name = r.decode().unwrap();
                        text.push_str(match name.as_ref() {
                            "amp" => "&",
                            "lt" => "<",
                            "gt" => ">",
                            "quot" => "\"",
                            _ => "'",
                        });
                    }
                }
                Event::End(e) if e.name().as_ref() == element.as_bytes() => {
                    found.extend(inside.take());
                }
                Event::Eof => break,
                _ => {}
            }
        }
        found
    }

    #[test]
    fn template_tail_rules() {
        let refused = [
            "", "a//b", "a/", "/a", ".", "..", "v1/./s", "v1/../s", "a\\b", "%2e%2e", "%2E",
            "a%2Fb", "a%2fb", "a%5Cb", "a%5cb",
        ];
        for tail in refused {
            assert!(!tail_ok(tail), "{tail:?} was accepted");
        }
        for tail in [
            "v1/seg-5.m4s",
            "seg.m4s",
            "a.b/c~d=e+f-g_h",
            "%41.m4s",
            "...",
        ] {
            assert!(tail_ok(tail), "{tail:?} was refused");
        }
    }

    #[test]
    fn template_build_keeps_the_origin() {
        let hostile = [
            "//canary.example/x",
            "https:canary.example",
            "@canary.example",
            "https://canary.example/x",
        ];
        for tail in hostile {
            let url = template_build("https://cdn.example", "/v/", None, tail, None).unwrap();
            assert_eq!(
                &url[..Position::BeforePath],
                "https://cdn.example",
                "{tail}"
            );
            assert!(url.path().starts_with("/v/"), "{tail}: {url}");
        }
        // The request's query, else the stored one.
        let stored = Some("sig=1");
        let url = template_build("https://cdn.example", "/v/", stored, "a.m4s", None);
        assert_eq!(url.unwrap().as_str(), "https://cdn.example/v/a.m4s?sig=1");
        let url = template_build("https://cdn.example", "/v/", stored, "a.m4s", Some("n=2"));
        assert_eq!(url.unwrap().as_str(), "https://cdn.example/v/a.m4s?n=2");
    }

    #[test]
    fn template_post_check() {
        let kept = |url: &str| {
            let url = Url::parse(url).unwrap();
            template_kept(&url, "https://cdn.example", "/v/")
        };
        assert!(kept("https://cdn.example/v/a.m4s"));
        for url in [
            "https://canary.example/v/a.m4s",
            "http://cdn.example/v/a.m4s",
            "https://cdn.example:8443/v/a.m4s",
            "https://user@cdn.example/v/a.m4s",
            "https://cdn.example/w/a.m4s",
            "https://cdn.example/a.m4s",
        ] {
            assert!(!kept(url), "{url} passed");
        }
    }

    #[test]
    fn dash_inherited_template_uses_each_representations_base() {
        let (out, mut table) = accepted(&mpd("<Period><AdaptationSet mimeType=\"video/mp4\">\
             <SegmentTemplate media=\"seg-$Number$.m4s\" initialization=\"init.mp4\" duration=\"4\"/>\
             <Representation id=\"a\" bandwidth=\"1\"><BaseURL>720p/</BaseURL></Representation>\
             <Representation id=\"b\" bandwidth=\"2\"><BaseURL>1080p/</BaseURL></Representation>\
             </AdaptationSet></Period>"));
        let bases = values(&out, "BaseURL", None);
        assert_eq!(bases.len(), 3, "{out}");
        assert_eq!(bases[1..], ["720p/", "1080p/"]);
        let media = values(&out, "SegmentTemplate", Some("media"));
        let init = values(&out, "SegmentTemplate", Some("initialization"));
        let seg = media[0].replace("$Number$", "5");
        for (rep, dir) in [(&bases[1], "720p"), (&bases[2], "1080p")] {
            assert_eq!(
                play(&mut table, &[&bases[0], rep, &seg]),
                Some(format!("https://cdn.example/v/{dir}/seg-5.m4s"))
            );
            assert_eq!(
                play(&mut table, &[&bases[0], rep, &init[0]]),
                Some(format!("https://cdn.example/v/{dir}/init.mp4"))
            );
        }
    }

    #[test]
    fn dash_base_url_cases() {
        // Inserted: the MPD's directory, before the first Period.
        let (out, mut table) = accepted(&mpd("<Period><AdaptationSet/></Period>"));
        let bases = values(&out, "BaseURL", None);
        assert_eq!(bases.len(), 1, "{out}");
        assert!(out.find("<BaseURL>") < out.find("<Period"), "{out}");
        assert_eq!(
            play(&mut table, &[&bases[0], "seg.m4s"]).as_deref(),
            Some("https://cdn.example/v/seg.m4s")
        );
        let (out, _) = accepted("<MPD xmlns=\"urn:mpeg:dash:schema:mpd:2011\"/>");
        assert_eq!(values(&out, "BaseURL", None).len(), 1, "{out}");
        // A relative MPD BaseURL: rewritten, nothing inserted.
        let (out, mut table) = accepted(&mpd("<BaseURL>media/</BaseURL><Period/>"));
        let bases = values(&out, "BaseURL", None);
        assert_eq!(bases.len(), 1, "{out}");
        assert_eq!(
            play(&mut table, &[&bases[0], "s.m4s"]).as_deref(),
            Some("https://cdn.example/v/media/s.m4s")
        );
        // Alternatives keep their selection attributes; a relative
        // Representation BaseURL goes under each.
        let (out, mut table) = accepted(&mpd(
            "<BaseURL serviceLocation=\"a\" dvb:priority=\"1\" dvb:weight=\"2\">https://a.example/x/</BaseURL>\
             <BaseURL serviceLocation=\"b\" dvb:priority=\"2\" dvb:weight=\"1\">https://b.example/y/</BaseURL>\
             <Period><AdaptationSet><Representation id=\"r\" bandwidth=\"1\"><BaseURL>r/</BaseURL>\
             </Representation></AdaptationSet></Period>",
        ));
        let bases = values(&out, "BaseURL", None);
        assert_eq!(values(&out, "BaseURL", Some("serviceLocation")), ["a", "b"]);
        assert_eq!(values(&out, "BaseURL", Some("dvb:priority")), ["1", "2"]);
        assert_eq!(values(&out, "BaseURL", Some("dvb:weight")), ["2", "1"]);
        let alternatives = [
            (&bases[0], "https://a.example/x"),
            (&bases[1], "https://b.example/y"),
        ];
        for (alternative, origin) in alternatives {
            assert_eq!(
                play(&mut table, &[alternative, &bases[2], "s.m4s"]),
                Some(format!("{origin}/r/s.m4s"))
            );
        }
        // Absolute and root-relative Period BaseURLs.
        let (out, mut table) = accepted(&mpd(
            "<Period><BaseURL>https://p.example/abs/</BaseURL></Period>\
             <Period><BaseURL>/root/</BaseURL></Period>",
        ));
        let bases = values(&out, "BaseURL", None);
        assert_eq!(bases.len(), 3, "{out}");
        assert_eq!(
            play(&mut table, &[&bases[1], "s.m4s"]).as_deref(),
            Some("https://p.example/abs/s.m4s")
        );
        assert_eq!(
            play(&mut table, &[&bases[2], "s.m4s"]).as_deref(),
            Some("https://cdn.example/root/s.m4s")
        );
        // Root-relative under alternatives on two origins: refused.
        let two = mpd(
            "<BaseURL>https://a.example/x/</BaseURL><BaseURL>https://b.example/y/</BaseURL>\
             <Period><BaseURL>/root/</BaseURL></Period>",
        );
        assert!(rewrite(&two).is_err());
        // A file BaseURL keeps its signed query, `&amp;` decoded.
        let (out, mut table) = accepted(&mpd(
            "<Period><AdaptationSet><Representation id=\"r\" bandwidth=\"1\">\
             <BaseURL>https://s3.example/b/file.mp4?X-Amz-Signature=abc&amp;X-Amz-Date=1</BaseURL>\
             <SegmentBase indexRange=\"0-99\"><Initialization range=\"0-9\"/></SegmentBase>\
             </Representation></AdaptationSet></Period>",
        ));
        let bases = values(&out, "BaseURL", None);
        assert!(bases[1].ends_with("/file.mp4"), "{out}");
        assert_eq!(
            upstream(&mut table, &bases[1]).as_deref(),
            Some("https://s3.example/b/file.mp4?X-Amz-Signature=abc&X-Amz-Date=1")
        );
        assert_eq!(values(&out, "SegmentBase", Some("indexRange")), ["0-99"]);
        assert_eq!(values(&out, "Initialization", Some("range")), ["0-9"]);
    }

    #[test]
    fn dash_segment_references() {
        let (out, mut table) = accepted(&mpd(
            "<Period><AdaptationSet><Representation id=\"r\" bandwidth=\"1\">\
             <BaseURL>https://cdn.example/v/file.mp4</BaseURL>\
             <SegmentList duration=\"4\">\
             <Initialization sourceURL=\"https://i.example/init.mp4\" range=\"0-99\"/>\
             <RepresentationIndex sourceURL=\"idx.sidx\"/>\
             <SegmentURL mediaRange=\"100-199\"/>\
             <SegmentURL media=\"https://m.example/s2.m4s\"/>\
             <SegmentURL media=\"s3.m4s\" index=\"https://m.example/s3.sidx\"/>\
             </SegmentList></Representation>\
             <Representation id=\"v1\" bandwidth=\"2\">\
             <SegmentTemplate media=\"https://t.example/a/$RepresentationID$/s-$Number$.m4s?t=$Time$\" \
             initialization=\"https://t.example/a/init-$RepresentationID$.mp4\"/>\
             </Representation></AdaptationSet></Period>",
        ));
        let init = values(&out, "Initialization", Some("sourceURL"));
        assert_eq!(init.len(), 1, "{out}");
        assert_eq!(
            upstream(&mut table, &init[0]).as_deref(),
            Some("https://i.example/init.mp4")
        );
        assert_eq!(values(&out, "Initialization", Some("range")), ["0-99"]);
        let index = values(&out, "RepresentationIndex", Some("sourceURL"));
        assert_eq!(index, ["idx.sidx"]);
        assert_eq!(values(&out, "SegmentURL", Some("mediaRange")), ["100-199"]);
        let media = values(&out, "SegmentURL", Some("media"));
        assert_eq!(
            upstream(&mut table, &media[0]).as_deref(),
            Some("https://m.example/s2.m4s")
        );
        assert_eq!(media[1], "s3.m4s");
        let index = values(&out, "SegmentURL", Some("index"));
        assert_eq!(
            upstream(&mut table, &index[0]).as_deref(),
            Some("https://m.example/s3.sidx")
        );
        // Absolute templates: the identifiers stay in the tail.
        let expand = |t: &str| {
            t.replace("$RepresentationID$", "v1")
                .replace("$Number$", "5")
                .replace("$Time$", "9")
        };
        let template = values(&out, "SegmentTemplate", Some("media"));
        assert!(template[0].starts_with(TEMPLATES), "{out}");
        assert_eq!(
            upstream(&mut table, &expand(&template[0])).as_deref(),
            Some("https://t.example/a/v1/s-5.m4s?t=9")
        );
        let init = values(&out, "SegmentTemplate", Some("initialization"));
        assert_eq!(
            upstream(&mut table, &expand(&init[0])).as_deref(),
            Some("https://t.example/a/init-v1.mp4")
        );
        // An identifier in the host: refused.
        let host = "<SegmentTemplate media=\"https://$RepresentationID$.t.example/s.m4s\"/>";
        assert!(rewrite(&mpd(&format!("<Period>{host}</Period>"))).is_err());
        // A relative `$RepresentationID$` template and ids real packagers
        // write.
        let ids = ["video=1500000", "audio_eng=64008", "v1_257"]
            .map(|id| format!("<Representation id=\"{id}\" bandwidth=\"1\"/>"))
            .concat();
        accepted(&mpd(&format!(
            "<Period><AdaptationSet>\
             <SegmentTemplate media=\"$RepresentationID$/seg-$Number$.m4s\"/>{ids}\
             </AdaptationSet></Period>"
        )));
    }

    #[test]
    fn dash_representation_ids_are_checked() {
        let bad = [
            "http://canary.test/x",
            "//canary.test/x",
            "a/b",
            "..",
            "a%2Fb",
            "a:b",
        ];
        let relative = "<SegmentTemplate media=\"$RepresentationID$/s-$Number$.m4s\"/>";
        let absolute = "<SegmentTemplate media=\"https://t.example/a/$RepresentationID$/s.m4s\"/>";
        for id in bad {
            let rep = format!("<Representation id=\"{id}\" bandwidth=\"1\"/>");
            let own =
                format!("<Representation id=\"{id}\" bandwidth=\"1\">{relative}</Representation>");
            // The template before its Representations, after them, and on
            // the Representation itself.
            for inner in [
                format!("{relative}{rep}"),
                format!("<Representation id=\"ok\" bandwidth=\"1\"/>{rep}{relative}"),
                own,
            ] {
                let body = mpd(&format!(
                    "<Period><AdaptationSet>{inner}</AdaptationSet></Period>"
                ));
                assert!(rewrite(&body).is_err(), "{id} was accepted: {inner}");
            }
            // Under an absolute template the expansion lands in a `/t/`
            // tail, whose rules answer 404 for `//` and `..`.
            let body = mpd(&format!(
                "<Period><AdaptationSet>{absolute}{rep}</AdaptationSet></Period>"
            ));
            let (out, mut table) = accepted(&body);
            let media = values(&out, "SegmentTemplate", Some("media"));
            let expanded = media[0].replace("$RepresentationID$", id);
            match upstream(&mut table, &expanded) {
                Some(url) => assert!(url.starts_with("https://t.example/a/"), "{id}: {url}"),
                None => {
                    let refused = id.contains("//") || id == ".." || id.contains("%2F");
                    assert!(refused, "{id} was answered 404");
                }
            }
        }
    }

    #[test]
    fn dash_text_is_decoded_before_it_is_judged() {
        let (out, mut table) = accepted(&mpd(
            "<BaseURL><![CDATA[http://canary.test/]]><!-- c --><?pi x?></BaseURL>\
             <Location>htt<!-- -->p://canary.test/m.mpd</Location>\
             <Period><BaseURL>&#104;ttp://canary.test/p/</BaseURL><AdaptationSet>\
             <SegmentTemplate media=\"&#104;ttp://canary.test/$Number$.m4s\"/>\
             </AdaptationSet></Period>",
        ));
        assert!(!out.contains("canary"), "{out}");
        assert!(!out.contains("CDATA") && !out.contains("<!--"), "{out}");
        assert_eq!(out.matches("<?").count(), 1, "{out}");
        let bases = values(&out, "BaseURL", None);
        assert_eq!(
            play(&mut table, &[&bases[0], "s.m4s"]).as_deref(),
            Some("http://canary.test/s.m4s")
        );
        assert_eq!(
            play(&mut table, &[&bases[1], "s.m4s"]).as_deref(),
            Some("http://canary.test/p/s.m4s")
        );
        let location = values(&out, "Location", None);
        assert_eq!(
            upstream(&mut table, &location[0]).as_deref(),
            Some("http://canary.test/m.mpd")
        );
        let media = values(&out, "SegmentTemplate", Some("media"));
        assert_eq!(
            upstream(&mut table, &media[0].replace("$Number$", "5")).as_deref(),
            Some("http://canary.test/5.m4s")
        );
        // An element inside a BaseURL refuses.
        assert!(rewrite(&mpd("<BaseURL>a<b/>c</BaseURL>")).is_err());
        // A prefixed element in DASH's namespace comes out unprefixed.
        let (out, mut table) = accepted(&mpd(
            "<x:BaseURL xmlns:x=\"urn:mpeg:dash:schema:mpd:2011\">https://p.example/</x:BaseURL><Period/>",
        ));
        assert!(!out.contains("x:"), "{out}");
        let bases = values(&out, "BaseURL", None);
        assert_eq!(
            play(&mut table, &[&bases[0], "s.m4s"]).as_deref(),
            Some("https://p.example/s.m4s")
        );
        // Schemes other than http and https refuse.
        for reference in [
            "<BaseURL>ftp://canary.test/</BaseURL>",
            "<Period><SegmentList><SegmentURL media=\"file:///etc/x\"/></SegmentList></Period>",
        ] {
            assert!(rewrite(&mpd(reference)).is_err(), "{reference}");
        }
    }

    #[test]
    fn dash_output_allowlist() {
        let (out, _) = accepted(&mpd(
            "<ProgramInformation moreInformationURL=\"http://canary.test/i\"><Title>t</Title></ProgramInformation>\
             <Period xmlns:o=\"urn:other\" o:x=\"http://canary.test/a\">\
             <EventStream schemeIdUri=\"urn:x\"><Event messageData=\"http://canary.test/e\"/></EventStream>\
             <Foo>http://canary.test/f</Foo><o:BaseURL>http://canary.test/o/</o:BaseURL>\
             <AdaptationSet><Label lang=\"en\">Main<Foo/><Role schemeIdUri=\"urn:x\" value=\"y\"/></Label>\
             <EssentialProperty schemeIdUri=\"urn:mpeg:dash:urlparam:2014\" value=\"urn:x\"/>\
             <Representation id=\"r\" bandwidth=\"1\" foo=\"bar\" \
             xmlns:scte214=\"urn:scte:dash:scte214-extensions\" \
             scte214:supplementalCodecs=\"dvh1.08.07\" scte214:supplementalProfiles=\"db4h\"/>\
             </AdaptationSet></Period>\
             <Metrics metrics=\"x\"><Reporting schemeIdUri=\"http://canary.test/r\"/></Metrics>\
             <ProducerReferenceTime id=\"1\"><UTCTiming schemeIdUri=\"urn:mpeg:dash:utc:http-iso:2014\" \
             value=\"http://canary.test/t\"/></ProducerReferenceTime>",
        ));
        assert!(!out.contains("canary"), "{out}");
        let gone = [
            "ProgramInformation",
            "EventStream",
            "Foo",
            "Metrics",
            "ProducerReferenceTime",
            "UTCTiming",
            "o:",
            "foo=",
        ];
        for name in gone {
            assert!(!out.contains(name), "{name}: {out}");
        }
        assert_eq!(values(&out, "Label", None), ["Main"]);
        assert!(
            !out.contains("<Role"),
            "an element in a Label was kept: {out}"
        );
        assert_eq!(values(&out, "EssentialProperty", Some("value")), ["urn:x"]);
        assert!(
            out.contains("scte214:supplementalCodecs=\"dvh1.08.07\""),
            "{out}"
        );
        assert!(
            out.contains("scte214:supplementalProfiles=\"db4h\""),
            "{out}"
        );
        assert!(out.contains("xmlns:scte214=\"urn:scte:dash:scte214-extensions\""));
        // A descriptor value that is a URL refuses.
        for value in ["http://canary.test/x", "//canary.test/x"] {
            let body = mpd(&format!(
                "<Period><AdaptationSet><EssentialProperty schemeIdUri=\"urn:x\" value=\"{value}\"/>\
                 </AdaptationSet></Period>"
            ));
            assert!(rewrite(&body).is_err(), "{value}");
        }
    }

    #[test]
    fn dash_utc_timing_allowlist() {
        let timing = |scheme: &str, value: &str| {
            format!("<UTCTiming schemeIdUri=\"urn:mpeg:dash:utc:{scheme}\" value=\"{value}\"/>")
        };
        let two = "https://time.example/a https://time.example/b";
        let (out, mut table) = accepted(&mpd(&[
            "<Period/>".to_owned(),
            timing("http-xsdate:2012", two),
            timing("http-iso:2014", two),
            timing("http-ntp:2012", two),
            timing("direct:2012", "2026-10-18T00:00:00Z"),
            timing("ntp:2014", "time.example"),
            timing("sntp:2014", "time.example"),
            timing("http-head:2014", "https://time.example/"),
            "<UTCTiming schemeIdUri=\"urn:example:time\" value=\"https://time.example/\"/>".into(),
        ]
        .concat()));
        let schemes = values(&out, "UTCTiming", Some("schemeIdUri"));
        assert_eq!(
            schemes,
            [
                "urn:mpeg:dash:utc:http-xsdate:2012",
                "urn:mpeg:dash:utc:http-iso:2014",
                "urn:mpeg:dash:utc:http-ntp:2012",
                "urn:mpeg:dash:utc:direct:2012",
            ]
        );
        let timings = values(&out, "UTCTiming", Some("value"));
        for value in &timings[..3] {
            let urls: Vec<String> = value
                .split(' ')
                .map(|url| {
                    let id = url.strip_prefix(PREFIX).unwrap().split('/').next().unwrap();
                    let (url, role) = table.get(id.parse().unwrap()).unwrap();
                    assert_eq!(role, Role::Small);
                    url.to_string()
                })
                .collect();
            assert_eq!(urls, ["https://time.example/a", "https://time.example/b"]);
        }
        assert_eq!(timings[3], "2026-10-18T00:00:00Z");
    }

    #[test]
    fn dash_refusals() {
        let refused = [
            (
                "xlink",
                "<Period xmlns:xlink=\"http://www.w3.org/1999/xlink\" xlink:href=\"https://canary.test/p\"/>",
            ),
            (
                "x",
                "<Period xmlns:x=\"http://www.w3.org/1999/xlink\" x:href=\"https://canary.test/p\"/>",
            ),
            ("undeclared", "<Period y:href=\"https://canary.test/p\"/>"),
            (
                "patch",
                "<PatchLocation>https://canary.test/p</PatchLocation><Period/>",
            ),
            (
                "protection",
                "<Period><AdaptationSet><ContentProtection \
                 schemeIdUri=\"urn:mpeg:dash:mp4protection:2011\"/></AdaptationSet></Period>",
            ),
            (
                "byte range",
                "<BaseURL byteRange=\"$base$?r=$first$\">a/</BaseURL><Period/>",
            ),
            (
                "dot dot",
                "<Period><AdaptationSet><Representation id=\"r\" bandwidth=\"1\">\
                 <BaseURL>../x/</BaseURL></Representation></AdaptationSet></Period>",
            ),
            (
                "dot",
                "<Period><SegmentTemplate media=\"./s-$Number$.m4s\"/></Period>",
            ),
            (
                "encoded dot",
                "<Period><SegmentTemplate media=\"a/%2e%2E/s-$Number$.m4s\"/></Period>",
            ),
            (
                "backslash dot",
                "<Period><SegmentTemplate media=\"a\\..\\s-$Number$.m4s\"/></Period>",
            ),
            (
                "root-relative",
                "<Period><SegmentTemplate media=\"/s-$Number$.m4s\"/></Period>",
            ),
            (
                "scheme-relative",
                "<Period><SegmentList><SegmentURL media=\"//canary.test/s.m4s\"/></SegmentList></Period>",
            ),
            (
                "base after a period",
                "<Period/><BaseURL>https://canary.test/</BaseURL>",
            ),
            (
                "base after a template",
                "<Period><SegmentTemplate media=\"s.m4s\"/><BaseURL>a/</BaseURL></Period>",
            ),
        ];
        for (name, inner) in refused {
            assert!(rewrite(&mpd(inner)).is_err(), "{name} was accepted");
        }
        let doctype = format!("<!DOCTYPE MPD>{}", mpd("<Period/>"));
        assert!(rewrite(&doctype).is_err(), "a DOCTYPE was accepted");
        let root = "<Period xmlns=\"urn:mpeg:dash:schema:mpd:2011\"><MPD/></Period>";
        assert!(rewrite(root).is_err(), "another root was accepted");
        assert!(rewrite("<MPD xmlns=\"urn:other\"/>").is_err());
        // Latin-1 bytes are not UTF-8.
        let mut latin1 = mpd("<Period id=\"x\"/>").into_bytes();
        let at = latin1.iter().position(|&b| b == b'x').unwrap();
        latin1[at] = 0xe9;
        let refused = rewrite_bytes(&latin1, &mut Table::default());
        assert!(refused.is_err(), "Latin-1 was accepted");
    }

    #[test]
    fn dash_limits() {
        // Sixteen base alternatives at once, and seventeen.
        let alternatives = |n: usize| {
            let bases: String = (0..n)
                .map(|i| format!("<BaseURL>https://h{i}.example/</BaseURL>"))
                .collect();
            mpd(&format!("{bases}<Period/>"))
        };
        accepted(&alternatives(16));
        assert!(rewrite(&alternatives(17)).is_err());
        // Four relative alternatives under four: sixteen; five: twenty.
        let product = |n: usize| {
            let outer: String = (0..4)
                .map(|i| format!("<BaseURL>https://h{i}.example/</BaseURL>"))
                .collect();
            let inner: String = (0..n)
                .map(|i| format!("<BaseURL>d{i}/</BaseURL>"))
                .collect();
            mpd(&format!("{outer}<Period>{inner}</Period>"))
        };
        accepted(&product(4));
        assert!(rewrite(&product(5)).is_err());
        // Depth 32, and 33.
        let nested = |depth: usize| {
            let open = "<Period>".to_owned() + &"<Role>".repeat(depth - 2);
            let close = "</Role>".repeat(depth - 2) + "</Period>";
            mpd(&format!("{open}{close}"))
        };
        accepted(&nested(32));
        assert!(rewrite(&nested(33)).is_err());
        // 1,024 template entries named by one MPD, and 1,025.
        let directories = |n: usize| {
            let periods: String = (1..n)
                .map(|i| format!("<Period><BaseURL>https://cdn.example/{i}/</BaseURL></Period>"))
                .collect();
            mpd(&format!(
                "<BaseURL>https://cdn.example/m/</BaseURL>{periods}"
            ))
        };
        accepted(&directories(1024));
        assert_eq!(rewrite(&directories(1025)), Err("too many templates"));
    }

    #[test]
    fn dash_location_resolves_against_the_mpd() {
        let (out, mut table) = accepted(&mpd(
            "<BaseURL>https://other.example/b/</BaseURL><Location>next.mpd</Location><Period/>",
        ));
        let location = values(&out, "Location", None);
        let id = location[0].strip_prefix(PREFIX).unwrap().split('/').next();
        let (url, role) = table.get(id.unwrap().parse().unwrap()).unwrap();
        assert_eq!(url.as_str(), "https://cdn.example/v/next.mpd");
        assert_eq!(role, Role::Dash);
    }

    /// The `.mpd` files of `tests/corpus/dash/{sub}`, by name.
    fn corpus(sub: &str) -> (std::path::PathBuf, Vec<String>) {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/dash");
        let mut names: Vec<String> = std::fs::read_dir(dir.join(sub))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.ends_with(".mpd"))
            .collect();
        names.sort();
        (dir, names)
    }

    /// `body` rewritten as the corpus is: for the core at
    /// `http://127.0.0.1:7777/` with a fixed token, from
    /// `https://cdn.example/dash/{name}`.
    fn corpus_rewrite(name: &str, body: &[u8]) -> Result<String, &'static str> {
        let token = "0f".repeat(32);
        let prefix = format!("http://127.0.0.1:7777/c/{token}/");
        let templates = format!("http://127.0.0.1:7777/t/{token}/");
        let base = Url::parse(&format!("https://cdn.example/dash/{name}")).unwrap();
        let mut table = Table::default();
        let mut children = Children {
            core: "http://127.0.0.1:7777/",
            prefix: &prefix,
            templates: &templates,
            table: &mut table,
            alive: &|| true,
        };
        rewrite_dash(body, &base, &mut children).map(|r| r.text)
    }

    /// The golden outputs Media3 parses in the Android `ManifestCorpusTest`:
    /// every input in `tests/corpus/dash/in` rewritten (`corpus_rewrite`).
    /// `MQ_CORPUS_WRITE=1` writes them to `tests/corpus/dash/out`;
    /// otherwise any difference fails.
    #[test]
    fn dash_corpus_matches_the_rewriter() {
        let write = std::env::var_os("MQ_CORPUS_WRITE").is_some();
        let (dir, inputs) = corpus("in");
        assert!(!inputs.is_empty());
        for name in &inputs {
            let body = std::fs::read(dir.join("in").join(name)).unwrap();
            let out = corpus_rewrite(name, &body).unwrap_or_else(|e| panic!("{name}: {e}"));
            let golden = dir.join("out").join(name);
            if write {
                std::fs::write(&golden, &out).unwrap();
            } else {
                let kept = std::fs::read_to_string(&golden).unwrap_or_default();
                assert_eq!(kept, out, "{name} differs from its golden output");
            }
        }
        assert_eq!(corpus("out").1, inputs, "a golden output with no input");
    }

    /// The controls in `tests/corpus/dash/control`: inputs Media3 reads an
    /// origin URL from (`ManifestCorpusTest` shows it does), all refused.
    #[test]
    fn dash_controls_are_refused() {
        let (dir, controls) = corpus("control");
        assert!(!controls.is_empty());
        for name in &controls {
            let body = std::fs::read(dir.join("control").join(name)).unwrap();
            assert!(corpus_rewrite(name, &body).is_err(), "{name} was accepted");
        }
    }
}
