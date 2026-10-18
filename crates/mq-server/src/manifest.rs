//! C22: HLS playlists served through the loopback server, rewritten so that
//! every URL a player fetches is the core's (plans/c21-c23-loopback.md 3.3).
//! This module holds the parts that need no network: telling a playlist from
//! a file, the allowlist rewrite, the output check and the resource table.

use mq_http::Url;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Bytes read at open to tell a playlist from a file.
pub(crate) const SNIFF: usize = 64 * 1024;
/// Largest playlist read.
pub(crate) const MANIFEST_MAX: usize = 8 << 20;
/// Largest rewritten playlist.
pub(crate) const OUTPUT_MAX: usize = 16 << 20;
/// Largest key served (the small role).
pub(crate) const SMALL_MAX: usize = 64 * 1024;
/// Resources a playback keeps, and the bytes of text they hold, before the
/// least recently used tenth goes.
const MAX_ENTRIES: usize = 131_072;
const MAX_TEXT: usize = 32 << 20;

/// What a body's first bytes are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sniffed {
    Hls,
    Dash,
    /// A DASH patch or a document with a `DOCTYPE`: never played.
    Refused,
    File,
}

/// DASH's namespace (ISO/IEC 23009-1).
const DASH_NS: &[u8] = b"urn:mpeg:dash:schema:mpd:2011";

/// Tells a playlist from a file by its first bytes: `#EXTM3U` after an
/// optional BOM and whitespace is HLS. An XML root element (after the
/// declaration, comments, processing instructions and whitespace) named
/// `MPD` in DASH's namespace, under any prefix the root binds to it, is
/// DASH; one named `MPD` in no or another namespace, and a `Patch`, are
/// refused: never served raw, never rewritten as DASH.
pub(crate) fn sniff(prefix: &[u8]) -> Sniffed {
    let mut rest = prefix.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(prefix);
    rest = rest.trim_ascii_start();
    if rest.starts_with(b"#EXTM3U") {
        return Sniffed::Hls;
    }
    loop {
        rest = rest.trim_ascii_start();
        let skip = |rest: &[u8], close: &[u8]| {
            rest.windows(close.len())
                .position(|w| w == close)
                .map(|at| at + close.len())
        };
        if rest.starts_with(b"<?") {
            let Some(end) = skip(rest, b"?>") else {
                return Sniffed::File;
            };
            rest = &rest[end..];
        } else if rest.starts_with(b"<!--") {
            let Some(end) = skip(rest, b"-->") else {
                return Sniffed::File;
            };
            rest = &rest[end..];
        } else if rest.starts_with(b"<!") {
            let doctype = rest.len() >= 9 && rest[2..9].eq_ignore_ascii_case(b"DOCTYPE");
            return if doctype {
                Sniffed::Refused
            } else {
                Sniffed::File
            };
        } else if let Some(tag) = rest.strip_prefix(b"<") {
            let end = tag
                .iter()
                .position(|b| b.is_ascii_whitespace() || matches!(b, b'>' | b'/'))
                .unwrap_or(tag.len());
            let name = &tag[..end];
            let (prefix, local) = match name.iter().position(|&b| b == b':') {
                Some(at) => (&name[..at], &name[at + 1..]),
                None => (&name[..0], name),
            };
            return match local {
                b"MPD" if namespace(&tag[end..], prefix) == Some(DASH_NS) => Sniffed::Dash,
                b"MPD" | b"Patch" => Sniffed::Refused,
                _ => Sniffed::File,
            };
        } else {
            return Sniffed::File;
        }
    }
}

/// The namespace a start tag's own attributes (`attrs`, after its name) bind
/// to `prefix` (empty: the default namespace), if any.
fn namespace<'a>(attrs: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    let mut rest = attrs;
    loop {
        rest = rest.trim_ascii_start();
        let name_end = rest
            .iter()
            .position(|&b| b == b'=' || b == b'>' || b == b'/')?;
        let name = rest[..name_end].trim_ascii();
        rest = rest[name_end..].strip_prefix(b"=")?.trim_ascii_start();
        let quote = *rest.first().filter(|q| matches!(q, b'"' | b'\''))?;
        let value_end = rest[1..].iter().position(|&b| b == quote)?;
        let value = &rest[1..1 + value_end];
        rest = &rest[2 + value_end..];
        let bound = match name.strip_prefix(b"xmlns") {
            Some(b"") => prefix.is_empty(),
            Some(rest) => rest.strip_prefix(b":") == Some(prefix) && !prefix.is_empty(),
            None => false,
        };
        if bound {
            return Some(value);
        }
    }
}

/// A manifest format, as a response or URL declares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Declared {
    Hls,
    Dash,
}

/// The format a `Content-Type` (parameters and case ignored) or a URL path
/// declares.
pub(crate) fn declared(content_type: Option<&str>, urls: &[&str]) -> Option<Declared> {
    let kind = content_type
        .and_then(|t| t.split(';').next())
        .map(|t| t.trim().to_ascii_lowercase());
    match kind.as_deref() {
        Some(
            "application/vnd.apple.mpegurl"
            | "application/x-mpegurl"
            | "audio/mpegurl"
            | "audio/x-mpegurl",
        ) => return Some(Declared::Hls),
        Some("application/dash+xml") => return Some(Declared::Dash),
        _ => {}
    }
    urls.iter().find_map(|url| {
        let path = Url::parse(url).ok()?.path().to_ascii_lowercase();
        if path.ends_with(".m3u8") || path.ends_with(".m3u") {
            Some(Declared::Hls)
        } else if path.ends_with(".mpd") {
            Some(Declared::Dash)
        } else {
            None
        }
    })
}

/// Whether an undeclared body could still be a playlist: not a video or an
/// audio type, and no longer than a playlist may be (or of unknown length).
pub(crate) fn candidate(content_type: Option<&str>, len: Option<u64>) -> bool {
    let kind = content_type.unwrap_or_default().trim().to_ascii_lowercase();
    let media = kind.starts_with("video/") || kind.starts_with("audio/");
    !media && len.is_none_or(|len| len <= MANIFEST_MAX as u64)
}

/// What named a child, which decides how it is served (never its bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    /// An HLS media playlist: parsed and rewritten.
    Playlist,
    /// Segments, init sections and parts: bytes, `Range` passed through.
    Media,
    /// Keys: bytes up to `SMALL_MAX`.
    Small,
}

struct Entry {
    dir: Arc<str>,
    last: Box<str>,
    query: Option<Arc<str>>,
    role: Role,
    used: u64,
}

type Key = (Arc<str>, Box<str>, Option<Arc<str>>);

/// One playback's children, by id. An id's URL and role never change, and
/// ids are never reused, so after an eviction an old id answers 404 and
/// never another resource. Directories and queries are interned: the signed
/// segment URLs of one playlist usually share both.
#[derive(Default)]
pub(crate) struct Table {
    entries: HashMap<u32, Entry>,
    ids: HashMap<Key, u32>,
    strings: HashSet<Arc<str>>,
    /// Interned strings and last segments, in bytes.
    text: usize,
    next: u32,
    tick: u64,
    /// The tick the running rewrite began at: what it named since is its
    /// output's, and never evicted while it runs.
    floor: u64,
    /// The last `#EXT-X-TARGETDURATION` of each playlist (`None`: the main
    /// one), for how long a blocking reload may wait; a child's goes when
    /// its entry goes.
    targets: HashMap<Option<u32>, u64>,
    /// The playlists the latest master advertised (variants, renditions,
    /// I-frame playlists): a player may switch to any of them at any time,
    /// so segment churn never evicts them while that master is current.
    topology: HashSet<u32>,
    /// A master's rewrite is running: the playlists it names replace the
    /// topology, so the old ones hold no place against the limits.
    replacing: bool,
}

impl Table {
    /// Starts a rewrite: entries it names from now on stay until it ends.
    pub(crate) fn begin(&mut self) {
        self.floor = self.tick + 1;
        self.replacing = false;
    }

    /// Starts a master's rewrite: as `begin`, and the playlists the last
    /// master advertised may go unless this one names them again, so a new
    /// master always fits where the old one did.
    pub(crate) fn begin_master(&mut self) {
        self.begin();
        self.replacing = true;
    }

    /// The target duration a playlist's last rewrite saw.
    pub(crate) fn target(&self, id: Option<u32>) -> Option<u64> {
        self.targets.get(&id).copied()
    }

    /// Keeps a playlist's target duration while its entry lasts.
    pub(crate) fn set_target(&mut self, id: Option<u32>, target: u64) {
        if id.is_none_or(|id| self.entries.contains_key(&id)) {
            self.targets.insert(id, target);
        }
    }

    /// Ends a master's rewrite: the playlists it named are now the ones a
    /// player may switch to.
    pub(crate) fn advertise(&mut self) {
        let floor = self.floor;
        self.topology = self
            .entries
            .iter()
            .filter(|(_, e)| e.used >= floor && e.role == Role::Playlist)
            .map(|(&id, _)| id)
            .collect();
    }

    /// The id that serves `url` in `role`. A URL named before in another
    /// role refuses the playlist naming it now, whichever came first. Past
    /// either limit, entries are evicted until both hold (`fit`).
    pub(crate) fn register(
        &mut self,
        url: &Url,
        role: Role,
        alive: &dyn Fn() -> bool,
    ) -> Result<u32, &'static str> {
        let (dir, last, query) = split(url);
        self.tick += 1;
        let probe = (
            Arc::<str>::from(dir),
            Box::<str>::from(last),
            query.map(Arc::<str>::from),
        );
        if let Some(&id) = self.ids.get(&probe) {
            let entry = self.entries.get_mut(&id).expect("indexed");
            if entry.role != role {
                return Err("a resource named in two roles");
            }
            entry.used = self.tick;
            self.fit(alive)?;
            return Ok(id);
        }
        if self.next == u32::MAX {
            return Err("too many resources");
        }
        let id = self.next;
        self.next += 1;
        let dir = self.intern(dir);
        let query = query.map(|q| self.intern(q));
        self.text += last.len();
        let entry = Entry {
            dir: dir.clone(),
            last: last.into(),
            query: query.clone(),
            role,
            used: self.tick,
        };
        self.entries.insert(id, entry);
        self.ids.insert((dir, last.into(), query), id);
        self.fit(alive)?;
        Ok(id)
    }

    /// Evicts until both limits hold, or refuses when nothing more may go.
    fn fit(&mut self, alive: &dyn Fn() -> bool) -> Result<(), &'static str> {
        while self.entries.len() > MAX_ENTRIES || self.text > MAX_TEXT {
            self.evict(alive)?;
        }
        Ok(())
    }

    /// The URL and role of `id`; asking counts as use.
    pub(crate) fn get(&mut self, id: u32) -> Option<(Url, Role)> {
        self.tick += 1;
        let entry = self.entries.get_mut(&id)?;
        entry.used = self.tick;
        let mut url = format!("{}{}", entry.dir, entry.last);
        if let Some(query) = &entry.query {
            url.push('?');
            url.push_str(query);
        }
        Some((Url::parse(&url).ok()?, entry.role))
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    fn intern(&mut self, s: &str) -> Arc<str> {
        if let Some(found) = self.strings.get(s) {
            return found.clone();
        }
        let s: Arc<str> = s.into();
        self.text += s.len();
        self.strings.insert(s.clone());
        s
    }

    /// Drops the least recently used tenth, but none the running rewrite
    /// named and, unless a new master is being rewritten, none the current
    /// master advertised: when only those are left, the rewrite does not
    /// fit. Looks at `alive` as it goes.
    /// ponytail: a scan per eviction; a linked list if evictions ever show
    /// in profiles.
    fn evict(&mut self, alive: &dyn Fn() -> bool) -> Result<(), &'static str> {
        let floor = self.floor;
        let mut by_use: Vec<(u64, u32)> = Vec::new();
        for (n, (&id, e)) in self.entries.iter().enumerate() {
            checkpoint(n, alive)?;
            if e.used < floor && (self.replacing || !self.topology.contains(&id)) {
                by_use.push((e.used, id));
            }
        }
        if by_use.is_empty() {
            return Err("too many resources");
        }
        if !alive() {
            return Err("out of time");
        }
        by_use.sort_unstable();
        let count = (self.entries.len() / 10).clamp(1, by_use.len());
        for (n, &(_, id)) in by_use[..count].iter().enumerate() {
            checkpoint(n, alive)?;
            self.targets.remove(&Some(id));
            if let Some(entry) = self.entries.remove(&id) {
                self.text -= entry.last.len();
                self.ids.remove(&(entry.dir, entry.last, entry.query));
            }
        }
        let text = &mut self.text;
        self.strings.retain(|s| {
            let kept = Arc::strong_count(s) > 1;
            if !kept {
                *text -= s.len();
            }
            kept
        });
        Ok(())
    }
}

/// `url` as its directory (scheme, authority, path up to the last `/`), last
/// segment and query; a fragment is dropped.
fn split(url: &Url) -> (&str, &str, Option<&str>) {
    let s = url.as_str();
    let end = s.find(['?', '#']).unwrap_or(s.len());
    let path = &s[..end];
    let cut = path.rfind('/').map_or(0, |at| at + 1);
    (&path[..cut], &path[cut..], url.query())
}

/// A rewritten playlist.
#[derive(Debug)]
pub(crate) struct Rewritten {
    pub(crate) text: String,
    /// `#EXT-X-TARGETDURATION`, for how long a blocking reload may wait.
    pub(crate) target_duration: Option<u64>,
}

/// A kept tag: whether its value is an attribute list, and the role of its
/// own `URI` (`None`: it may name none).
struct Kept {
    name: &'static str,
    list: bool,
    uri: Option<Role>,
}

const fn kept(name: &'static str, list: bool, uri: Option<Role>) -> Kept {
    Kept { name, list, uri }
}

/// Tags kept: everything else (unknown tags, comments, session data,
/// steering, date ranges) is dropped, as clients ignore unknown tags.
const KEPT: [Kept; 27] = [
    kept("EXTINF", false, None),
    kept("EXT-X-VERSION", false, None),
    kept("EXT-X-TARGETDURATION", false, None),
    kept("EXT-X-MEDIA-SEQUENCE", false, None),
    kept("EXT-X-DISCONTINUITY-SEQUENCE", false, None),
    kept("EXT-X-DISCONTINUITY", false, None),
    kept("EXT-X-ENDLIST", false, None),
    kept("EXT-X-PLAYLIST-TYPE", false, None),
    kept("EXT-X-I-FRAMES-ONLY", false, None),
    kept("EXT-X-INDEPENDENT-SEGMENTS", false, None),
    kept("EXT-X-START", true, None),
    kept("EXT-X-PROGRAM-DATE-TIME", false, None),
    kept("EXT-X-GAP", false, None),
    kept("EXT-X-BITRATE", false, None),
    kept("EXT-X-BYTERANGE", false, None),
    kept("EXT-X-STREAM-INF", true, None),
    kept("EXT-X-MEDIA", true, Some(Role::Playlist)),
    kept("EXT-X-I-FRAME-STREAM-INF", true, Some(Role::Playlist)),
    kept("EXT-X-KEY", true, Some(Role::Small)),
    kept("EXT-X-SESSION-KEY", true, Some(Role::Small)),
    kept("EXT-X-MAP", true, Some(Role::Media)),
    kept("EXT-X-SERVER-CONTROL", true, None),
    kept("EXT-X-PART-INF", true, None),
    kept("EXT-X-PART", true, Some(Role::Media)),
    kept("EXT-X-PRELOAD-HINT", true, Some(Role::Media)),
    kept("EXT-X-RENDITION-REPORT", true, Some(Role::Playlist)),
    kept("EXT-X-SKIP", true, None),
];

/// Where a rewrite sends a child: `{prefix}{id}/{name}`.
pub(crate) struct Children<'a> {
    /// `http://127.0.0.1:{port}/`: every URL a player may see starts so.
    pub(crate) core: &'a str,
    /// `http://127.0.0.1:{port}/c/{token}/`.
    pub(crate) prefix: &'a str,
    pub(crate) table: &'a mut Table,
    /// False once the rewrite's deadline has passed or its playback ended;
    /// asked every `CHECK_EVERY` lines.
    pub(crate) alive: &'a dyn Fn() -> bool,
}

/// How many lines (or entries) a rewrite handles between two looks at
/// `alive`.
const CHECK_EVERY: usize = 1024;

/// Every `CHECK_EVERY` steps of a rewrite, the first included: stop once its
/// time is up or its playback is gone.
fn checkpoint(n: usize, alive: &dyn Fn() -> bool) -> Result<(), &'static str> {
    if n.is_multiple_of(CHECK_EVERY) && !alive() {
        return Err("out of time");
    }
    Ok(())
}

impl Children<'_> {
    fn link(&mut self, url: &Url, role: Role) -> Result<String, &'static str> {
        let id = self.table.register(url, role, self.alive)?;
        let (_, last, _) = split(url);
        Ok(format!("{}{id}/{}", self.prefix, crate::sanitize(last)))
    }
}

/// Rewrites an HLS playlist by allowlist (RFC 8216): kept tags with their
/// `URI` pointing at the core by role, URI lines likewise, everything else
/// dropped or refused. `base` is the playlist's final URL after redirects;
/// `media_only` refuses a master (a child named as a media playlist).
pub(crate) fn rewrite_hls(
    body: &[u8],
    base: &Url,
    media_only: bool,
    children: &mut Children<'_>,
) -> Result<Rewritten, &'static str> {
    let alive = children.alive;
    checkpoint(0, alive)?;
    let text = std::str::from_utf8(body).map_err(|_| "not UTF-8")?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = Vec::new();
    for (n, line) in text.split('\n').enumerate() {
        checkpoint(n + 1, alive)?;
        let line = line.strip_suffix('\r').unwrap_or(line);
        // Media3 ends a line at a bare CR: one inside a kept tag would give
        // it a URI line this rewrite never saw.
        if line.chars().any(control) {
            return Err("a control character");
        }
        // Media3 trims every line as Java's `trim` does (code points up to a
        // space) and skips empty ones: ` #EXT-X-ENDLIST ` is still a tag.
        let line = line.trim_matches(|c: char| c <= ' ');
        if !line.is_empty() {
            lines.push(line);
        }
    }
    let Some((first, lines)) = lines.split_first() else {
        return Err("no #EXTM3U");
    };
    if *first != "#EXTM3U" {
        return Err("no #EXTM3U");
    }
    let (mut master, mut media) = (false, false);
    for (n, line) in lines.iter().enumerate() {
        checkpoint(n + 1, alive)?;
        match tag_of(line) {
            Some("EXT-X-STREAM-INF" | "EXT-X-MEDIA") => master = true,
            Some("EXT-X-TARGETDURATION") => media = true,
            _ => {}
        }
    }
    if master == media {
        return Err("neither or both a master and a media playlist");
    }
    if master && media_only {
        return Err("a master playlist named as a media playlist");
    }
    if master {
        children.table.begin_master();
    } else {
        children.table.begin();
    }
    let mut out = String::from("#EXTM3U\n");
    let mut variant = false;
    let mut target_duration = None;
    for (n, &line) in lines.iter().enumerate() {
        checkpoint(n + 1, alive)?;
        if !line.starts_with('#') {
            let role = if master {
                if !variant {
                    return Err("a URI line with no variant");
                }
                variant = false;
                Role::Playlist
            } else {
                Role::Media
            };
            let url = resolve(base, line)?;
            emit(&mut out, &children.link(&url, role)?)?;
            continue;
        }
        let Some(tag) = tag_of(line) else {
            continue; // a comment
        };
        if tag == "EXT-X-DEFINE" {
            return Err("EXT-X-DEFINE");
        }
        let Some(kept) = KEPT.iter().find(|kept| kept.name == tag) else {
            continue;
        };
        let value = line[1 + tag.len()..].strip_prefix(':');
        match tag {
            "EXTINF" => {
                let duration = value
                    .unwrap_or_default()
                    .split(',')
                    .next()
                    .unwrap_or_default();
                emit(&mut out, &format!("#EXTINF:{duration},"))?;
            }
            "EXT-X-TARGETDURATION" => {
                target_duration = value.and_then(|v| v.trim().parse().ok());
                emit(&mut out, line)?;
            }
            _ if kept.list => {
                if tag == "EXT-X-STREAM-INF" {
                    if variant {
                        return Err("a variant with no URI line");
                    }
                    variant = true;
                }
                let list = value.unwrap_or_default();
                emit(
                    &mut out,
                    &rewrite_list(tag, kept.uri, list, base, children)?,
                )?;
            }
            _ => emit(&mut out, line)?,
        }
    }
    if variant {
        return Err("a variant with no URI line");
    }
    output_check(&out, children.core, alive)?;
    if master {
        children.table.advertise();
    }
    Ok(Rewritten {
        text: out,
        target_duration,
    })
}

/// Appends a line, refusing before the output would pass `OUTPUT_MAX`: one
/// short line repeated can make an accepted playlist's output 50 times its
/// size.
fn emit(out: &mut String, line: &str) -> Result<(), &'static str> {
    if out.len() + line.len() + 1 > OUTPUT_MAX {
        return Err("output too large");
    }
    out.push_str(line);
    out.push('\n');
    Ok(())
}

/// C0 but tab (a bare CR included), DEL, C1, and the Unicode line and
/// paragraph separators.
fn control(c: char) -> bool {
    (c.is_control() && c != '\t') || matches!(c, '\u{2028}' | '\u{2029}')
}

/// The tag name of a `#EXT...` line.
fn tag_of(line: &str) -> Option<&str> {
    let tag = line.strip_prefix('#')?;
    if !tag.starts_with("EXT") {
        return None;
    }
    Some(tag.split(':').next().unwrap_or(tag))
}

fn resolve(base: &Url, reference: &str) -> Result<Url, &'static str> {
    let url = base.join(reference).map_err(|_| "an unresolvable URI")?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        _ => Err("a URI that is not http or https"),
    }
}

/// An attribute-list tag with its `URI` rewritten by role and every other
/// attribute verbatim.
fn rewrite_list(
    tag: &str,
    role: Option<Role>,
    list: &str,
    base: &Url,
    children: &mut Children<'_>,
) -> Result<String, &'static str> {
    let attrs = attributes(list).ok_or("a malformed attribute list")?;
    let mut seen = HashSet::new();
    for (name, _) in &attrs {
        if !seen.insert(*name) {
            return Err("a duplicate attribute");
        }
        // In any case: Media3 and FFmpeg read names as written, another
        // player may not.
        if name.to_ascii_uppercase().contains("URI") && *name != "URI" {
            return Err("an attribute named like URI");
        }
    }
    let key = matches!(tag, "EXT-X-KEY" | "EXT-X-SESSION-KEY");
    if key
        && attrs
            .iter()
            .any(|(n, v)| *n == "KEYFORMAT" && *v != "\"identity\"")
    {
        return Err("a key format other than identity");
    }
    let mut parts = Vec::with_capacity(attrs.len());
    for (name, value) in attrs {
        if name != "URI" {
            parts.push(format!("{name}={value}"));
            continue;
        }
        let role = role.ok_or("a URI on a tag that names none")?;
        let reference = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .ok_or("an unquoted URI")?;
        // Inline keys stay inline; mpv's whitelist then blocks them.
        if key && reference.starts_with("data:") {
            parts.push(format!("URI={value}"));
            continue;
        }
        let url = resolve(base, reference)?;
        parts.push(format!("URI=\"{}\"", children.link(&url, role)?));
    }
    Ok(format!("#{tag}:{}", parts.join(",")))
}

/// `NAME=value` pairs, commas inside quoted values kept.
fn attributes(list: &str) -> Option<Vec<(&str, &str)>> {
    let mut out = Vec::new();
    let mut rest = list.trim();
    while !rest.is_empty() {
        let eq = rest.find('=')?;
        let name = rest[..eq].trim();
        if name.is_empty() || name.contains([',', '"']) {
            return None;
        }
        let after = &rest[eq + 1..];
        let end = if let Some(quoted) = after.strip_prefix('"') {
            quoted.find('"')? + 2
        } else {
            after.find(',').unwrap_or(after.len())
        };
        out.push((name, &after[..end]));
        rest = after[end..].trim_start();
        if let Some(next) = rest.strip_prefix(',') {
            rest = next.trim_start();
            if rest.is_empty() {
                return None;
            }
        } else if !rest.is_empty() {
            return None;
        }
    }
    Some(out)
}

/// Reads a rewritten playlist the way Media3 1.11.1 does (lines end at LF,
/// CR or CRLF; a line not starting with `#` is a URL; `URI="` anywhere is a
/// URL), and accepts it only if every such URL is the core's own (`core` is
/// `http://127.0.0.1:{port}/`), or `data:` in a key.
/// Looks at `alive` as it goes, like the rewrite.
pub(crate) fn output_check(
    text: &str,
    core: &str,
    alive: &dyn Fn() -> bool,
) -> Result<(), &'static str> {
    checkpoint(0, alive)?;
    for (n, line) in text.split(['\n', '\r']).enumerate() {
        checkpoint(n + 1, alive)?;
        if !line.is_empty() && !line.starts_with('#') && !line.starts_with(core) {
            return Err("the output check");
        }
        let key = line.starts_with("#EXT-X-KEY:") || line.starts_with("#EXT-X-SESSION-KEY:");
        let core_only = line.match_indices("URI=\"").all(|(at, found)| {
            let rest = &line[at + found.len()..];
            rest.starts_with(core) || (key && rest.starts_with("data:"))
        });
        if !core_only {
            return Err("the output check");
        }
    }
    Ok(())
}

/// `output_check` with all the time in the world, for tests.
#[cfg(test)]
pub(crate) fn hls_output_ok(text: &str, core: &str) -> bool {
    output_check(text, core, &|| true).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CORE: &str = "http://127.0.0.1:7777/";
    const PREFIX: &str = "http://127.0.0.1:7777/c/token/";

    fn rewrite(body: &str, media_only: bool) -> Result<Rewritten, &'static str> {
        let base = Url::parse("https://cdn.example/v/master.m3u8?sig=1").unwrap();
        let mut table = Table::default();
        let mut children = Children {
            core: CORE,
            prefix: PREFIX,
            table: &mut table,
            alive: &|| true,
        };
        rewrite_hls(body.as_bytes(), &base, media_only, &mut children)
    }

    #[test]
    fn sniffs_playlists_and_files() {
        assert_eq!(sniff(b"\xEF\xBB\xBF \n#EXTM3U\n"), Sniffed::Hls);
        let mpd =
            b"<?xml version=\"1.0\"?><!-- x --><d:MPD xmlns:d=\"urn:mpeg:dash:schema:mpd:2011\">";
        assert_eq!(sniff(mpd), Sniffed::Dash);
        assert_eq!(sniff(b"<!DOCTYPE x><MPD>"), Sniffed::Refused);
        assert_eq!(sniff(b"<?xml?><Patch>"), Sniffed::Refused);
        // Namespace-aware: DASH only in DASH's namespace, under the prefix
        // the root binds to it; any other MPD is refused, never a file.
        let dash = |root: &str| sniff(format!("<?xml version=\"1.0\"?>{root}").as_bytes());
        assert_eq!(
            dash("<MPD xmlns=\"urn:mpeg:dash:schema:mpd:2011\" type='static'>"),
            Sniffed::Dash
        );
        assert_eq!(
            dash("<MPD type=\"static\" xmlns='urn:mpeg:dash:schema:mpd:2011'/>"),
            Sniffed::Dash
        );
        for root in [
            "<MPD>",
            "<d:MPD xmlns:d=\"urn:example:other\">",
            "<x:MPD xmlns=\"urn:mpeg:dash:schema:mpd:2011\" xmlns:x=\"urn:other\">",
            "<MPD xmlns:d=\"urn:mpeg:dash:schema:mpd:2011\">",
            "<d:MPD xmlns:dd=\"urn:mpeg:dash:schema:mpd:2011\">",
            "<Patch xmlns=\"urn:mpeg:dash:schema:mpd-patch:2020\">",
        ] {
            assert_eq!(dash(root), Sniffed::Refused, "{root}");
        }
        assert_eq!(sniff(b"\x1aE\xdf\xa3"), Sniffed::File);
        assert_eq!(sniff(b"<html>"), Sniffed::File);
    }

    #[test]
    fn declared_by_type_or_path() {
        let hls = Some(Declared::Hls);
        assert_eq!(
            declared(Some("Application/X-MpegURL; charset=UTF-8"), &[]),
            hls
        );
        assert_eq!(declared(None, &["https://a.example/x/index.m3u8?t=1"]), hls);
        assert_eq!(
            declared(None, &["https://a.example/v", "https://b.example/v.mpd"]),
            Some(Declared::Dash)
        );
        assert_eq!(
            declared(Some("video/mp4"), &["https://a.example/v.mkv"]),
            None
        );
        assert!(candidate(Some("application/octet-stream"), Some(1000)));
        assert!(candidate(None, None));
        assert!(!candidate(Some("video/x-matroska"), Some(1000)));
        assert!(!candidate(
            Some("text/plain"),
            Some(MANIFEST_MAX as u64 + 1)
        ));
    }

    #[test]
    fn hls_allowlist() {
        let master = "#EXTM3U\n#EXT-X-VERSION:6\n# a comment\n#EXT-X-SESSION-DATA:DATA-ID=\"x\",URI=\"https://evil.example/d\"\n#EXT-X-CONTENT-STEERING:SERVER-URI=\"https://evil.example/s\"\n#EXT-X-FOO:X=\"//evil.example/x\"\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"en, main\",URI=\"audio/en.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=800000,AUDIO=\"a\"\nlow/index.m3u8?u=https://a.example/\n";
        let out = rewrite(master, false).unwrap();
        assert_eq!(
            out.text,
            "#EXTM3U\n#EXT-X-VERSION:6\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"en, main\",URI=\"http://127.0.0.1:7777/c/token/0/en.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=800000,AUDIO=\"a\"\nhttp://127.0.0.1:7777/c/token/1/index.m3u8\n"
        );
        let media = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-DATERANGE:ID=\"x\",X-AD=\"https://evil.example\"\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n#EXT-X-MAP:URI=\"init.mp4\",BYTERANGE=\"720@0\"\n#EXTINF:4.0,a title\n#EXT-X-BYTERANGE:1000@720\nseg.mp4\n#EXT-X-ENDLIST\n";
        let out = rewrite(media, true).unwrap();
        assert_eq!(out.target_duration, Some(4));
        assert_eq!(
            out.text,
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-KEY:METHOD=AES-128,URI=\"http://127.0.0.1:7777/c/token/0/key.bin\"\n#EXT-X-MAP:URI=\"http://127.0.0.1:7777/c/token/1/init.mp4\",BYTERANGE=\"720@0\"\n#EXTINF:4.0,\n#EXT-X-BYTERANGE:1000@720\nhttp://127.0.0.1:7777/c/token/2/seg.mp4\n#EXT-X-ENDLIST\n"
        );
        assert!(hls_output_ok(&out.text, CORE));
        // Lines are trimmed as Media3 trims them, so these keep their meaning.
        let sloppy =
            "#EXTM3U \n#EXT-X-TARGETDURATION:4\t\n  \n #EXTINF:4,\n\tseg.ts \n#EXT-X-ENDLIST \n";
        assert_eq!(
            rewrite(sloppy, true).unwrap().text,
            format!(
                "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\n{PREFIX}0/seg.ts\n#EXT-X-ENDLIST\n"
            )
        );
    }

    #[test]
    fn hls_refusals() {
        let media =
            |line: &str| format!("#EXTM3U\n#EXT-X-TARGETDURATION:4\n{line}\n#EXTINF:4,\nseg.ts\n");
        let canary = "http://127.0.0.1:9";
        let (scheme, named, control) = (
            "a URI that is not http or https",
            "an attribute named like URI",
            "a control character",
        );
        // Each refused by its own rule, so a rule that stops working shows
        // even when another layer (the output check) still refuses.
        let refused = [
            (
                media("#EXT-X-DEFINE:NAME=\"a\",VALUE=\"b\""),
                "EXT-X-DEFINE",
            ),
            (media("#EXT-X-DEFINE:QUERYPARAM=\"token\""), "EXT-X-DEFINE"),
            (
                media(
                    "#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://k\",KEYFORMAT=\"com.apple.streamingkeydelivery\"",
                ),
                "a key format other than identity",
            ),
            (
                media(
                    "#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"k\",KEYFORMAT=\"urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed\"",
                ),
                "a key format other than identity",
            ),
            (media("ftp://a.example/seg.ts"), scheme),
            (media("#EXT-X-MAP:URI=\"data:text/plain,x\""), scheme),
            (
                "#EXTM3U\n#EXTINF:10,Channel\nhttp://a.example/stream\n".into(),
                "neither or both a master and a media playlist",
            ),
            (
                "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-STREAM-INF:BANDWIDTH=1\nv.m3u8\n".into(),
                "neither or both a master and a media playlist",
            ),
            (
                media(&format!(
                    "#EXT-X-KEY:METHOD=AES-128,X-URI=\"{canary}/x\",URI=\"k\""
                )),
                named,
            ),
            (
                media(&format!("#EXT-X-MAP:X-URI=\"{canary}/x\",URI=\"i.mp4\"")),
                named,
            ),
            // Unquoted, so only the name rule stops it: the output check
            // looks for `URI="` as Media3 does, other players may not.
            (
                media(&format!("#EXT-X-MAP:X-URI={canary}/x,URI=\"i.mp4\"")),
                named,
            ),
            (
                media(&format!("#EXT-X-MAP:uri=\"{canary}/x\",URI=\"i.mp4\"")),
                named,
            ),
            (media("#EXT-X-MAP:URI=i.mp4"), "an unquoted URI"),
            (
                media("#EXT-X-KEY:METHOD=AES-128,URI=\"a\",URI=\"b\""),
                "a duplicate attribute",
            ),
            (
                media(&format!(
                    "#EXT-X-PROGRAM-DATE-TIME:2026-01-01T00:00:00Z\r{canary}/seg.ts"
                )),
                control,
            ),
            (media("#EXT-X-KEY:METHOD=AES-128,URI=\"a\r\""), control),
            (media("#EXT-X-VERSION:3\u{0}"), control),
            (media("#EXT-X-VERSION:3\u{85}"), control),
            (media("#EXT-X-VERSION:3\u{2028}"), control),
            // A kept tag copied verbatim cannot carry a URL past the check.
            (
                media(&format!("#EXT-X-PROGRAM-DATE-TIME:URI=\"{canary}/x\"")),
                "the output check",
            ),
            (
                format!(
                    "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"x\",X-URI=\"{canary}/x\",URI=\"a.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=1\nv.m3u8\n"
                ),
                named,
            ),
        ];
        for (body, reason) in refused {
            assert_eq!(rewrite(&body, false).unwrap_err(), reason, "{body:?}");
        }
        // A master as a media child.
        assert_eq!(
            rewrite("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nv.m3u8\n", true).unwrap_err(),
            "a master playlist named as a media playlist"
        );
        // A lowercase tag is no tag: it goes as a comment, URL and all.
        let lowercase = rewrite(
            &media(&format!("#ext-x-key:METHOD=AES-128,URI=\"{canary}/k\"")),
            true,
        );
        assert!(!lowercase.unwrap().text.contains(canary));
        // Inline keys stay.
        assert!(
            rewrite(
                &media("#EXT-X-KEY:METHOD=AES-128,URI=\"data:text/plain;base64,AAAA\""),
                true
            )
            .is_ok()
        );
    }

    #[test]
    fn hls_output_check() {
        let ok = format!(
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"{CORE}c/t/0/k\"\n#EXT-X-SESSION-KEY:METHOD=AES-128,URI=\"data:x\"\n{CORE}c/t/1/s.ts\n"
        );
        assert!(hls_output_ok(&ok, CORE));
        let bad = [
            "#EXTM3U\nhttp://a.example/s.ts\n".to_owned(),
            format!("#EXTM3U\n#EXT-X-MAP:URI=\"{CORE}c/t/0/i\",X-URI=\"http://a.example/x\"\n"),
            "#EXTM3U\n#EXT-X-MAP:URI=\"data:x\"\n".to_owned(),
            format!(
                "#EXTM3U\n#EXT-X-PROGRAM-DATE-TIME:x\rhttp://a.example/s.ts\n{CORE}c/t/1/s.ts\n"
            ),
            "#EXTM3U\n#EXT-X-MEDIA:URI=\"https://a.example/a.m3u8\"\n".to_owned(),
        ];
        for text in bad {
            assert!(!hls_output_ok(&text, CORE), "{text:?}");
        }
    }

    #[test]
    fn the_output_budget_holds_while_writing() {
        // One short line repeated is one resource, so only the output budget
        // stops it, and it must stop before the refusal at the end.
        let mut body = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:4\n");
        body.push_str(&"a\n".repeat(600_000));
        body.push_str("ftp://a.example/x\n");
        assert_eq!(rewrite(&body, true).unwrap_err(), "output too large");
    }

    #[test]
    fn a_rewrite_stops_when_its_time_is_up() {
        let base = Url::parse("https://cdn.example/v/live.m3u8").unwrap();
        let mut table = Table::default();
        let asked = std::cell::Cell::new(0);
        let alive = || {
            asked.set(asked.get() + 1);
            asked.get() < 3
        };
        let mut children = Children {
            core: CORE,
            prefix: PREFIX,
            table: &mut table,
            alive: &alive,
        };
        let body = format!(
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n{}",
            "#EXTINF:4,\ns.ts\n".repeat(CHECK_EVERY * 2)
        );
        let refused = rewrite_hls(body.as_bytes(), &base, true, &mut children);
        assert_eq!(refused.unwrap_err(), "out of time");
        assert_eq!(asked.get(), 3, "asked once every {CHECK_EVERY} lines");
    }

    #[test]
    fn resources_keep_their_role() {
        let url = |s: &str| Url::parse(s).unwrap();
        let mut table = Table::default();
        let a = url("https://cdn.example/v/a.ts?sig=1");
        let id = table.register(&a, Role::Media, &|| true).unwrap();
        assert_eq!(table.register(&a, Role::Media, &|| true), Ok(id));
        assert!(table.register(&a, Role::Small, &|| true).is_err());
        assert!(table.register(&a, Role::Playlist, &|| true).is_err());
        let k = url("https://cdn.example/k");
        table.register(&k, Role::Small, &|| true).unwrap();
        assert!(table.register(&k, Role::Playlist, &|| true).is_err());
        assert_eq!(table.get(id), Some((a, Role::Media)));
        // Through a playlist: a URL first named as a key, then as a variant.
        let master = "#EXTM3U\n#EXT-X-SESSION-KEY:METHOD=AES-128,URI=\"v.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=1\nv.m3u8\n";
        assert!(rewrite(master, false).is_err());
        let reverse = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nv.m3u8\n#EXT-X-SESSION-KEY:METHOD=AES-128,URI=\"v.m3u8\"\n";
        assert!(rewrite(reverse, false).is_err());
    }

    #[test]
    fn reloads_keep_ids_and_refuse_a_new_role() {
        let base = Url::parse("https://cdn.example/v/live.m3u8").unwrap();
        let mut table = Table::default();
        let mut children = Children {
            core: CORE,
            prefix: PREFIX,
            table: &mut table,
            alive: &|| true,
        };
        let first = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\ns1.ts\n";
        let a = rewrite_hls(first.as_bytes(), &base, true, &mut children).unwrap();
        let b = rewrite_hls(first.as_bytes(), &base, true, &mut children).unwrap();
        assert_eq!(a.text, b.text);
        assert_eq!(children.table.len(), 1);
        let key = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-KEY:METHOD=AES-128,URI=\"s1.ts\"\n#EXTINF:4,\ns2.ts\n";
        assert!(rewrite_hls(key.as_bytes(), &base, true, &mut children).is_err());
        let playlist = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-RENDITION-REPORT:URI=\"s1.ts\"\n#EXTINF:4,\ns2.ts\n";
        assert!(rewrite_hls(playlist.as_bytes(), &base, true, &mut children).is_err());
    }

    #[test]
    fn tables_evict_least_recently_used() {
        let mut table = Table::default();
        table.begin();
        // 32,000 signed URLs of about 1 KB with one shared query fit.
        let sig = "x".repeat(900);
        for i in 0..32_000 {
            let url =
                Url::parse(&format!("https://cdn.example/v/seg-{i:05}.m4s?sig={sig}")).unwrap();
            table.register(&url, Role::Media, &|| true).unwrap();
        }
        assert_eq!(table.len(), 32_000);
        assert_eq!(table.get(0).map(|(_, role)| role), Some(Role::Media));
        // Past the entry limit the least recently used tenth of what earlier
        // rewrites named goes: id 1 was never asked for again, id 0 was just
        // asked for.
        table.begin();
        let mut last = 0;
        for i in 32_000..=MAX_ENTRIES {
            let url = Url::parse(&format!("https://cdn.example/v/seg-{i:06}.m4s")).unwrap();
            last = table.register(&url, Role::Media, &|| true).unwrap();
        }
        assert!(table.len() <= MAX_ENTRIES);
        assert!(table.get(0).is_some(), "a recently used entry went");
        assert!(table.get(1).is_none(), "an untouched older entry stayed");
        // Ids are never reused: the next one is new, an evicted one stays gone.
        let url = Url::parse("https://cdn.example/v/new.m4s").unwrap();
        assert_eq!(table.register(&url, Role::Media, &|| true), Ok(last + 1));
        assert!(table.get(1).is_none());
    }

    #[test]
    fn tables_evict_past_their_text_limit() {
        // Signed URLs of about 1 KB, each with its own signature: 32,000
        // fit; past 32 MiB of text the least recently used tenth goes.
        let signed = |i: usize| {
            let sig = format!("{i:06}{}", "x".repeat(900));
            Url::parse(&format!("https://cdn.example/v/seg-{i:06}.m4s?sig={sig}")).unwrap()
        };
        let mut table = Table::default();
        table.begin();
        for i in 0..32_000 {
            table.register(&signed(i), Role::Media, &|| true).unwrap();
        }
        assert_eq!(table.len(), 32_000);
        table.begin();
        for i in 32_000..40_000 {
            table.register(&signed(i), Role::Media, &|| true).unwrap();
        }
        assert!(table.len() < 40_000, "the text limit evicted nothing");
        assert!(table.text <= MAX_TEXT);
        assert!(table.get(0).is_none());
    }

    /// A rewrite that is told to stop does no more work: at its start, while
    /// it collects lines, while it evicts and while it checks its output.
    #[test]
    fn a_rewrite_looks_at_its_time_throughout() {
        let base = Url::parse("https://cdn.example/v/live.m3u8").unwrap();
        // `alive` says stop at its `stop_at`-th question; each stop shows by
        // what the rewrite did not get to.
        let run = |body: &[u8], stop_at: usize| {
            let asked = std::cell::Cell::new(0);
            let alive = || {
                asked.set(asked.get() + 1);
                asked.get() < stop_at
            };
            let mut table = Table::default();
            let mut children = Children {
                core: CORE,
                prefix: PREFIX,
                table: &mut table,
                alive: &alive,
            };
            let result = rewrite_hls(body, &base, true, &mut children);
            (result.map(|r| r.text), asked.get(), table.len())
        };
        let segments = |n: usize| "#EXTINF:4,\ns.ts\n".repeat(n);
        // At once: not even read (this body is not UTF-8).
        assert_eq!(run(b"\xff\xfe", 1), (Err("out of time"), 1, 0));
        // While collecting lines: the control character after them is never
        // reached.
        let control = format!("#EXTM3U\n{}#EXT-X-VERSION:3\u{0}\n", segments(CHECK_EVERY));
        assert_eq!(run(control.as_bytes(), 2), (Err("out of time"), 2, 0));
        // While telling a master from a media playlist: nothing named.
        // 1,202 lines: one look while collecting, one while telling, then the
        // main loop.
        let media = format!("#EXTM3U\n#EXT-X-TARGETDURATION:4\n{}", segments(600));
        assert_eq!(run(media.as_bytes(), 3), (Err("out of time"), 3, 0));
        // In the main loop: some named, not all.
        let (stopped, asked, named) = run(media.as_bytes(), 4);
        assert_eq!((stopped, asked), (Err("out of time"), 4));
        assert!(named > 0, "stopped before the main loop");
        // Eviction: the table is full of what earlier rewrites named, and a
        // new name finds the time up while making room.
        let mut table = Table::default();
        table.begin();
        let url = |i: usize| Url::parse(&format!("https://cdn.example/v/{i}.ts")).unwrap();
        for i in 0..MAX_ENTRIES {
            table.register(&url(i), Role::Media, &|| true).unwrap();
        }
        table.begin();
        let stopped = table.register(&url(MAX_ENTRIES), Role::Media, &|| false);
        assert_eq!(stopped, Err("out of time"));
        assert_eq!(
            table.len(),
            MAX_ENTRIES + 1,
            "evicted after it was told to stop"
        );
        // The output check.
        let clean = format!("#EXTM3U\n{CORE}c/t/0/s.ts\n");
        assert_eq!(output_check(&clean, CORE, &|| true), Ok(()));
        assert_eq!(output_check(&clean, CORE, &|| false), Err("out of time"));
    }

    /// A player may switch to any variant or rendition the master named at
    /// any time: segment churn never evicts them while that master is
    /// current.
    #[test]
    fn advertised_playlists_survive_segment_churn() {
        let base = Url::parse("https://cdn.example/v/master.m3u8").unwrap();
        let mut table = Table::default();
        let mut children = Children {
            core: CORE,
            prefix: PREFIX,
            table: &mut table,
            alive: &|| true,
        };
        let master = "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"en\",URI=\"audio/en.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=1,AUDIO=\"a\"\nlow/index.m3u8\n";
        let text = rewrite_hls(master.as_bytes(), &base, false, &mut children)
            .unwrap()
            .text;
        let audio: u32 = text
            .split("/c/token/")
            .nth(1)
            .unwrap()
            .split('/')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        // The player plays the video rendition: its reloads name fresh
        // signed segments, enough to evict several times over.
        let media = Url::parse("https://cdn.example/v/low/index.m3u8").unwrap();
        for reload in 0..7 {
            let mut body = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:4\n");
            for i in 0..20_000 {
                body.push_str(&format!("#EXTINF:4,\ns{reload}-{i}.ts\n"));
            }
            rewrite_hls(body.as_bytes(), &media, true, &mut children).unwrap();
        }
        assert!(children.table.len() <= MAX_ENTRIES);
        // The audio rendition the master advertised still answers.
        let (url, role) = children.table.get(audio).expect("the audio rendition went");
        assert_eq!(url.as_str(), "https://cdn.example/v/audio/en.m3u8");
        assert_eq!(role, Role::Playlist);
    }

    /// A new master replaces the old one's topology, also at the entry
    /// limit: the old playlists it does not name again may go while it is
    /// rewritten, so it is never refused for the old master's sake.
    #[test]
    fn a_new_master_replaces_the_old_topology() {
        let base = Url::parse("https://cdn.example/v/master.m3u8").unwrap();
        let mut table = Table::default();
        let mut children = Children {
            core: CORE,
            prefix: PREFIX,
            table: &mut table,
            alive: &|| true,
        };
        let master = |names: &mut dyn Iterator<Item = String>| {
            let mut body = String::from("#EXTM3U\n");
            for name in names {
                body.push_str(&format!("#EXT-X-STREAM-INF:BANDWIDTH=1\n{name}\n"));
            }
            body
        };
        // As many short variants as the table holds.
        let full = master(&mut (0..MAX_ENTRIES).map(|i| format!("{i}.m3u8")));
        rewrite_hls(full.as_bytes(), &base, false, &mut children).unwrap();
        assert_eq!(children.table.len(), MAX_ENTRIES);
        // The next master drops one variant and adds another.
        let one_new = master(
            &mut (1..MAX_ENTRIES)
                .map(|i| format!("{i}.m3u8"))
                .chain(["new.m3u8".to_owned()]),
        );
        let replaced = rewrite_hls(one_new.as_bytes(), &base, false, &mut children);
        assert!(replaced.is_ok(), "{replaced:?}");
        // Signed variant URLs that rotate on every master.
        for round in 0..2 {
            let signed = master(&mut (0..MAX_ENTRIES).map(|i| format!("{i}.m3u8?sig={round}")));
            let replaced = rewrite_hls(signed.as_bytes(), &base, false, &mut children);
            assert!(replaced.is_ok(), "round {round}: {replaced:?}");
        }
        assert_eq!(children.table.len(), MAX_ENTRIES);
    }

    /// Past either limit, eviction goes on until both hold; a name already
    /// in the table checks them too.
    #[test]
    fn both_limits_hold_after_every_name() {
        // Each query its own: shared ones are interned once.
        let big = |i: usize, mib: usize| {
            let query = format!("{i}-{}", "q".repeat(mib << 20));
            Url::parse(&format!("https://cdn.example/v/{i}.ts?sig={query}")).unwrap()
        };
        // About 31 MiB of older names, then one with a 7 MiB query: one
        // tenth evicted is not enough.
        let mut table = Table::default();
        table.begin();
        for i in 0..31 {
            table.register(&big(i, 1), Role::Media, &|| true).unwrap();
        }
        table.begin();
        table.register(&big(99, 7), Role::Media, &|| true).unwrap();
        assert!(table.text <= MAX_TEXT, "{} bytes of text", table.text);
        // One rewrite naming more than the text limit is refused, leaving
        // the table over it; the next name, though already there, brings it
        // back under.
        let mut table = Table::default();
        table.begin();
        for i in 0..4 {
            table.register(&big(i, 7), Role::Media, &|| true).unwrap();
        }
        let refused = table.register(&big(4, 7), Role::Media, &|| true);
        assert_eq!(refused, Err("too many resources"));
        assert!(table.text > MAX_TEXT);
        table.begin();
        table.register(&big(0, 7), Role::Media, &|| true).unwrap();
        assert!(table.text <= MAX_TEXT, "{} bytes of text", table.text);
    }

    #[test]
    fn target_durations_go_with_their_playlists() {
        let url = |i: usize| Url::parse(&format!("https://cdn.example/v/{i}.m3u8")).unwrap();
        let mut table = Table::default();
        table.begin();
        let first = table.register(&url(0), Role::Playlist, &|| true).unwrap();
        table.set_target(Some(first), 4);
        table.set_target(None, 6);
        assert_eq!(table.target(Some(first)), Some(4));
        // Later reloads fill the table: the first playlist goes, and its
        // target duration with it.
        table.begin();
        for i in 1..=MAX_ENTRIES {
            table.register(&url(i), Role::Media, &|| true).unwrap();
        }
        assert!(table.get(first).is_none());
        assert_eq!(
            table.target(Some(first)),
            None,
            "an evicted playlist's target stayed"
        );
        table.set_target(Some(first), 4);
        assert_eq!(
            table.target(Some(first)),
            None,
            "a target for a gone playlist"
        );
        assert_eq!(table.target(None), Some(6));
    }

    #[test]
    fn a_rewrite_never_evicts_its_own_resources() {
        let base = Url::parse("https://cdn.example/v/live.m3u8").unwrap();
        let media = |names: std::ops::Range<usize>, tag: &str| {
            let mut body = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:4\n");
            for i in names {
                body.push_str(&format!("{tag}{i}\n"));
            }
            body
        };
        let mut table = Table::default();
        let mut children = Children {
            core: CORE,
            prefix: PREFIX,
            table: &mut table,
            alive: &|| true,
        };
        // An earlier reload's 5,000 make room for this one's 127,000: the
        // limit takes only theirs, so every id this one names still answers.
        rewrite_hls(media(0..5_000, "a").as_bytes(), &base, true, &mut children).unwrap();
        let fits = media(0..127_000, "b");
        let text = rewrite_hls(fits.as_bytes(), &base, true, &mut children)
            .unwrap()
            .text;
        let first: u32 = text
            .lines()
            .nth(2)
            .unwrap()
            .rsplit('/')
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(
            children.table.get(first).map(|(_, role)| role),
            Some(Role::Media)
        );
        assert!(
            children.table.get(0).is_none(),
            "an earlier reload's entry stayed"
        );
        // One playlist naming more than the limit holds does not fit at all.
        let mut table = Table::default();
        let mut children = Children {
            core: CORE,
            prefix: PREFIX,
            table: &mut table,
            alive: &|| true,
        };
        let wide = media(0..MAX_ENTRIES + 1, "c");
        let refused = rewrite_hls(wide.as_bytes(), &base, true, &mut children);
        assert_eq!(refused.unwrap_err(), "too many resources");
    }

    /// The `.m3u8` files of `tests/corpus/hls/{sub}`, by name.
    fn corpus(sub: &str) -> (std::path::PathBuf, Vec<String>) {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/hls");
        let mut names: Vec<String> = std::fs::read_dir(dir.join(sub))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.ends_with(".m3u8"))
            .collect();
        names.sort();
        (dir, names)
    }

    /// The golden outputs Media3 parses in the Android `ManifestCorpusTest`:
    /// every input in `tests/corpus/hls/in` rewritten for the core at
    /// `CORE` with a fixed token. `MQ_CORPUS_WRITE=1` writes them to
    /// `tests/corpus/hls/out`; otherwise any difference fails.
    #[test]
    fn corpus_matches_the_rewriter() {
        let write = std::env::var_os("MQ_CORPUS_WRITE").is_some();
        let (dir, inputs) = corpus("in");
        assert!(!inputs.is_empty());
        let prefix = format!("{CORE}c/{}/", "0f".repeat(32));
        for name in &inputs {
            let body = std::fs::read(dir.join("in").join(name)).unwrap();
            let base = Url::parse(&format!("https://cdn.example/hls/{name}")).unwrap();
            let mut table = Table::default();
            let mut children = Children {
                core: CORE,
                prefix: &prefix,
                table: &mut table,
                alive: &|| true,
            };
            let out = rewrite_hls(&body, &base, false, &mut children)
                .unwrap_or_else(|e| panic!("{name}: {e}"))
                .text;
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

    /// The controls in `tests/corpus/hls/control`: inputs Media3 reads an
    /// origin URL from (`ManifestCorpusTest` shows it does), all refused.
    #[test]
    fn controls_are_refused() {
        let (dir, controls) = corpus("control");
        assert!(!controls.is_empty());
        for name in &controls {
            let body = std::fs::read(dir.join("control").join(name)).unwrap();
            let base = Url::parse(&format!("https://cdn.example/hls/{name}")).unwrap();
            let mut table = Table::default();
            let mut children = Children {
                core: CORE,
                prefix: PREFIX,
                table: &mut table,
                alive: &|| true,
            };
            let rewritten = rewrite_hls(&body, &base, false, &mut children);
            assert!(rewritten.is_err(), "{name} was accepted");
        }
    }
}
