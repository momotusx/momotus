//! The Internet Archive, built in (plans/plugins-and-marketplace.md 5): a
//! weekly local list of the `feature_films` collection's films dated 1963
//! or earlier, matched on the device by title and year; a matched film's
//! item is read for its video files, which become Versions rows of their
//! own pass (4.4). Nothing is asked of archive.org for a profile that has
//! not seen the notice, has the source off, or is a kids profile, and
//! every request, playback included, keeps to archive.org
//! (`mq_server::upstream_http`).

use crate::Core;
use mq_http::{HttpError, RequestOptions, Url};
use mq_library::ArchiveFilm;
use mq_streams::ParsedStream;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The source's name on its rows.
pub const NAME: &str = "Internet Archive";
/// The host its rows play from.
pub const HOST: &str = "archive.org";
/// Where the source is; tests point a core at a fixture (`Core::use_archive_at`).
pub const ORIGIN: &str = "https://archive.org";
/// A row key's prefix: `archive:{identifier}:{file}` (4.4).
pub const KEY_PREFIX: &str = "archive:";
const QUERY: &str =
    "collection:feature_films AND (licenseurl:*publicdomain* OR licenseurl:*creativecommons*)";
const FIELDS: &str = "identifier,title,year,downloads";
/// The newest effective year the list keeps (5.2).
const LATEST_YEAR: u16 = 1963;
const DAY_MS: u64 = 86_400_000;
/// The list is asked again once this old.
const WEEK_MS: u64 = 7 * DAY_MS;
/// Days to wait after the service turns the list away, the last repeating.
const BACKOFF_DAYS: [u64; 3] = [1, 3, 7];
/// After a try that never reached the service (offline, a timeout), the
/// next waits this long.
const RETRY_SOON: Duration = Duration::from_secs(600);
/// Past either, the answer is refused and the old list kept (5.3).
const MAX_ITEMS: usize = 50_000;
const INDEX_BYTES: usize = 8 << 20;
const INDEX_WAIT: Duration = Duration::from_secs(30);
/// Pages of the list read at most.
const MAX_PAGES: usize = 20;
/// How long a film's pass waits for the very first list; the fetch goes on
/// without it.
const FIRST_LIST: Duration = Duration::from_secs(10);
const META_BYTES: usize = 2 << 20;
const META_FILES: usize = 2_000;
const META_WAIT: Duration = Duration::from_secs(3);
/// An item's files are kept this long, in memory only: the ids of opened
/// films are viewing history.
const META_KEPT: Duration = Duration::from_secs(86_400);
const META_ITEMS: usize = 64;
/// Rows per film.
const ROWS: usize = 3;
/// Characters of item text shown.
const TEXT_MAX: usize = 120;
/// The device value holding when the list was fetched and its backoff.
const STATE: &str = "archive_index";

/// One video file of an item.
#[derive(Debug, Clone, PartialEq)]
struct File {
    name: String,
    height: Option<u32>,
    size: Option<u64>,
}

/// An item's video files and when they were read.
type Read = (Instant, Arc<Vec<File>>);

/// The source's device-wide state: where it is, one refresh at a time, and
/// the items read.
pub(crate) struct Archive {
    origin: Url,
    refreshing: tokio::sync::Mutex<()>,
    /// When a try last failed without an answer.
    missed: Mutex<Option<Instant>>,
    items: Mutex<HashMap<String, Read>>,
}

impl Archive {
    pub(crate) fn new(origin: Url) -> Self {
        Self {
            origin,
            refreshing: tokio::sync::Mutex::new(()),
            missed: Mutex::new(None),
            items: Mutex::default(),
        }
    }
}

/// When the list was fetched, and the service's refusals since.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Fetched {
    fetched_ms: Option<u64>,
    refusals: u32,
    retry_ms: u64,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The active profile may contact archive.org: not a kids profile, the
/// source on, and the notice seen (5.1).
pub(crate) fn wanted(core: &Core) -> bool {
    if core.policy().kids() {
        return false;
    }
    let prefs = core.prefs();
    prefs.archive && prefs.archive_notice_seen
}

/// The link the source's streams carry as their install.
pub(crate) fn link() -> Url {
    Url::parse(mq_server::ARCHIVE).expect("a valid built-in link")
}

/// The rows for a film named `names` (the meta's name first) of `year`,
/// for a profile that wants the source: none, with no request, when the
/// list has no such film. The first use waits for the list; a stale list
/// is used while a new one is fetched.
pub(crate) async fn streams(core: &Core, names: &[&str], year: Option<u16>) -> Vec<ParsedStream> {
    let Some(year) = year.filter(|_| wanted(core)) else {
        return Vec::new();
    };
    ready(core).await;
    let found = names
        .iter()
        .map(|n| norm(n))
        .filter(|n| !n.is_empty())
        .find_map(|n| core.library().archive_film(&n, year).ok().flatten());
    // The profile may have changed while the list came.
    let Some(id) = found.filter(|_| wanted(core)) else {
        return Vec::new();
    };
    let Some(files) = files(core, &id).await else {
        return Vec::new();
    };
    let origin = &core.archive().origin;
    files
        .iter()
        .enumerate()
        .filter_map(|(i, f)| stream(origin, &id, f, i as u32))
        .collect()
}

/// Makes sure a list is there: a fetch runs on its own, so leaving the
/// page never cancels it; with no list yet the pass waits for it up to
/// `FIRST_LIST`. Never on mobile data, never while a backoff holds.
async fn ready(core: &Core) {
    if !due(core) {
        return;
    }
    let first = core.library().archive_films().unwrap_or(0) == 0;
    let task = tokio::spawn({
        let core = core.clone();
        async move { refresh(&core).await }
    });
    if first {
        let _ = tokio::time::timeout(FIRST_LIST, task).await;
    }
}

fn fetched(core: &Core) -> Fetched {
    (core.library().device_value(STATE).ok().flatten())
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default()
}

/// The list is older than a week (or missing), no backoff holds, the last
/// unanswered try is not recent, and the device is not on mobile data.
fn due(core: &Core) -> bool {
    let now = mq_library::hlc::now_ms();
    let f = fetched(core);
    // A list from the future (the clock moved back) is stale too.
    let stale = f.fetched_ms.is_none_or(|t| now >= t + WEEK_MS || t > now)
        || core.library().archive_films().unwrap_or(0) == 0;
    let missed = lock(&core.archive().missed).is_some_and(|t| t.elapsed() < RETRY_SOON);
    stale && now >= f.retry_ms && !missed && core.network() != crate::speed::Network::Cellular
}

/// Fetches the list and stores it, one at a time; a refusal backs off 1,
/// 3, then 7 days, keeping the old list.
async fn refresh(core: &Core) {
    let archive = core.archive();
    let _one = archive.refreshing.lock().await;
    // Another refresh may have finished while this one waited, or the
    // profile changed.
    if !due(core) || !wanted(core) {
        return;
    }
    let mut f = fetched(core);
    let now = mq_library::hlc::now_ms();
    match fetch_index(core).await {
        // Switched off or away while it came: nothing is kept.
        Ok(_) if !wanted(core) => return,
        Ok(films) => {
            if let Err(e) = core.library().put_archive_index(&films) {
                tracing::warn!("archive list not stored: {e}");
                *lock(&archive.missed) = Some(Instant::now());
                return;
            }
            f = Fetched {
                fetched_ms: Some(now),
                ..Fetched::default()
            };
            tracing::info!(films = films.len(), "archive list fetched");
        }
        Err(e) if unanswered(&e) => {
            tracing::info!("archive list not reached: {}", e.kind());
            *lock(&archive.missed) = Some(Instant::now());
            return;
        }
        Err(e) => {
            let wait = BACKOFF_DAYS[(f.refusals as usize).min(BACKOFF_DAYS.len() - 1)];
            f.refusals += 1;
            f.retry_ms = now + wait * DAY_MS;
            tracing::info!("archive list refused ({}), next in {wait} days", e.kind());
        }
    }
    if let Ok(text) = serde_json::to_string(&f) {
        let _ = core.library().set_device_value(STATE, &text);
    }
}

/// No answer came: offline, a timeout, a name that resolved only to
/// private addresses. A hop the rule refused is an answer (archive.org sent
/// it elsewhere) and backs off.
fn unanswered(e: &HttpError) -> bool {
    match e {
        HttpError::Network(_) | HttpError::Timeout(_) => true,
        HttpError::BlockedPrivate(_) => !e.hop_refused(),
        _ => false,
    }
}

/// Every page of the `scrape` answer, within the caps, as the list.
async fn fetch_index(core: &Core) -> Result<Vec<ArchiveFilm>, HttpError> {
    let http = mq_server::upstream_http(core.http(), Some(&link()));
    let base = core.archive().origin.join("services/search/v1/scrape");
    let base = base.map_err(|e| HttpError::InvalidUrl(e.to_string()))?;
    let (mut items, mut bytes, mut cursor) = (Vec::new(), 0usize, None::<String>);
    for n in 0.. {
        // A profile switched off, or to a kids profile, asks no further page.
        if !wanted(core) {
            return Err(HttpError::Network("the list is no longer wanted".into()));
        }
        if n == MAX_PAGES {
            return Err(HttpError::TooLarge { limit: MAX_PAGES });
        }
        let mut url = base.clone();
        url.query_pairs_mut()
            .append_pair("q", QUERY)
            .append_pair("fields", FIELDS)
            .append_pair("count", "10000");
        if let Some(c) = &cursor {
            url.query_pairs_mut().append_pair("cursor", c);
        }
        let opts = RequestOptions {
            timeout: INDEX_WAIT,
            max_bytes: INDEX_BYTES - bytes,
            ..Default::default()
        };
        let response = http.get(url.as_str(), &opts).await?;
        bytes += response.body.len();
        let mut page: Value = response.json()?;
        // An answer without its items is no list: the old one stays.
        let Some(Value::Array(found)) = page.get_mut("items").map(Value::take) else {
            return Err(HttpError::Decode("no items".into()));
        };
        items.extend(found);
        if items.len() > MAX_ITEMS {
            return Err(HttpError::TooLarge { limit: MAX_ITEMS });
        }
        match page.get("cursor").and_then(Value::as_str) {
            Some(c) if !c.is_empty() && cursor.as_deref() != Some(c) => cursor = Some(c.to_owned()),
            _ => break,
        }
    }
    let films = index(&items);
    if films.is_empty() {
        return Err(HttpError::Decode("no films".into()));
    }
    Ok(films)
}

/// The list from the `scrape` items, by the rules of 5.2: a film needs an
/// effective year of 1963 or earlier, trailers go, and one item stands for
/// each normalized title and year: one whose title names no dub or
/// edition, then the most downloaded, then the first.
#[doc(hidden)]
pub fn index(items: &[Value]) -> Vec<ArchiveFilm> {
    let mut best: HashMap<(String, u16), (bool, u64, ArchiveFilm)> = HashMap::new();
    let mut order = Vec::new();
    for item in items {
        let text = |key: &str| match first(item.get(key)) {
            Some(Value::String(s)) => s.clone(),
            _ => String::new(),
        };
        let (identifier, title) = (text("identifier"), text("title"));
        // Archive identifiers are these characters; anything else never
        // reaches a URL.
        let valid = |c: char| c.is_ascii_alphanumeric() || "._-".contains(c);
        let starts = identifier.starts_with(|c: char| c.is_ascii_alphanumeric());
        if !starts || identifier.len() > 100 || !identifier.chars().all(valid) {
            continue;
        }
        let Some(year) = year_of(&title, first(item.get("year"))) else {
            continue;
        };
        if year > LATEST_YEAR
            || format!("{title}{identifier}")
                .to_lowercase()
                .contains("trailer")
        {
            continue;
        }
        let downloads = item.get("downloads").and_then(Value::as_u64).unwrap_or(0);
        let key = (norm(&title), year);
        let rank = (plain(&title), downloads);
        let film = ArchiveFilm {
            title: key.0.clone(),
            year,
            identifier,
            downloads,
        };
        match best.get(&key) {
            Some((p, d, _)) if (*p, *d) >= rank => {}
            Some(_) => {
                best.insert(key, (rank.0, rank.1, film));
            }
            None => {
                order.push(key.clone());
                best.insert(key, (rank.0, rank.1, film));
            }
        }
    }
    order
        .into_iter()
        .filter_map(|k| best.remove(&k).map(|(_, _, f)| f))
        .collect()
}

fn first(v: Option<&Value>) -> Option<&Value> {
    match v {
        Some(Value::Array(list)) => list.first(),
        other => other,
    }
}

/// The year in a "(YYYY)" of the title, else the field's first four
/// characters; never a bare number in a title ("1984 Nigel Kneale" is a
/// 1954 film). Zero is no year.
fn year_of(title: &str, field: Option<&Value>) -> Option<u16> {
    let in_title = title.match_indices('(').find_map(|(at, _)| {
        let rest = title[at + 1..].trim_start();
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        let after = rest[digits.len()..].trim_start();
        (digits.len() == 4 && after.starts_with(')')).then(|| digits.parse().ok())?
    });
    if in_title.is_some() {
        return in_title;
    }
    let field = match field? {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => return None,
    };
    let year: u16 = field.get(..4.min(field.len()))?.trim().parse().ok()?;
    (year > 0).then_some(year)
}

/// Words a title carries for a dub or an edition (5.2), longest first.
const NOISE: [&str; 8] = [
    "full movie",
    "full hd",
    "hd",
    "restored",
    "english",
    "dubbed",
    "subtitled",
    "colorized",
];

fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `text` (lowercase) with every `NOISE` match as whole words replaced by
/// a space, and whether there was one.
fn without_noise(text: &str) -> (String, bool) {
    let (mut out, mut found, mut prev) = (String::new(), false, None::<char>);
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        let at_start = !prev.is_some_and(word);
        let hit = at_start
            .then(|| {
                NOISE.iter().find(|n| {
                    rest.starts_with(*n) && !rest[n.len()..].chars().next().is_some_and(word)
                })
            })
            .flatten();
        if let Some(n) = hit {
            out.push(' ');
            found = true;
            prev = n.chars().last();
            rest = &rest[n.len()..];
        } else {
            out.push(c);
            prev = Some(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    (out, found)
}

/// The title as the list keys it (5.2): lowercase, without brackets,
/// parentheses, the `NOISE` words and a leading "the", in ASCII letters and
/// digits. The meta's name is matched the same way.
#[doc(hidden)]
pub fn norm(title: &str) -> String {
    let lower = fold_accents(&title.to_lowercase());
    let (bare, _) = without_noise(&without_brackets(&lower));
    let words: Vec<&str> = bare
        .split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit()))
        .filter(|w| !w.is_empty())
        .collect();
    let words = match words.split_first() {
        Some((&"the", rest)) if !rest.is_empty() => rest,
        _ => &words[..],
    };
    words.join(" ")
}

/// Latin letters with accents as their plain ones ("Fantômas" is
/// "fantomas"), lowercase in and out; the rules script folds the same.
fn fold_accents(text: &str) -> String {
    const FOLDS: [(&str, &str); 22] = [
        ("àáâãäåāăą", "a"),
        ("çćĉċč", "c"),
        ("ďđ", "d"),
        ("èéêëēĕėęě", "e"),
        ("ĝğġģ", "g"),
        ("ĥħ", "h"),
        ("ìíîïĩīĭįı", "i"),
        ("ĵ", "j"),
        ("ķ", "k"),
        ("ĺļľŀł", "l"),
        ("ñńņňŉ", "n"),
        ("òóôõöøōŏő", "o"),
        ("ŕŗř", "r"),
        ("śŝşš", "s"),
        ("ţťŧ", "t"),
        ("ùúûüũūŭůűų", "u"),
        ("ŵ", "w"),
        ("ýÿŷ", "y"),
        ("źżž", "z"),
        ("ß", "ss"),
        ("æ", "ae"),
        ("œ", "oe"),
    ];
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match FOLDS.iter().find(|(from, _)| from.contains(c)) {
            Some((_, to)) => out.push_str(to),
            None => out.push(c),
        }
    }
    out
}

/// `text` with each "(...)" and "[...]" replaced by a space; an opening
/// one with no close after it stays.
fn without_brackets(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        let close = match c {
            '(' => rest[1..].find(')'),
            '[' => rest[1..].find(']'),
            _ => None,
        };
        match close {
            Some(at) => {
                out.push(' ');
                rest = &rest[at + 2..];
            }
            None => {
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    out
}

/// The raw title names no dub or edition: preferred among duplicates.
fn plain(title: &str) -> bool {
    !without_noise(&title.to_lowercase()).1
}

/// The video files of item `id`, from memory when read within a day, else
/// from its metadata (2 MiB, its first 2,000 files, 3 s). None when it
/// could not be read.
async fn files(core: &Core, id: &str) -> Option<Arc<Vec<File>>> {
    let archive = core.archive();
    if let Some((at, files)) = lock(&archive.items).get(id)
        && at.elapsed() < META_KEPT
    {
        return Some(files.clone());
    }
    let mut url = archive.origin.join("metadata/").ok()?;
    url.path_segments_mut().ok()?.pop_if_empty().push(id);
    let opts = RequestOptions {
        timeout: META_WAIT,
        max_bytes: META_BYTES,
        ..Default::default()
    };
    let http = mq_server::upstream_http(core.http(), Some(&link()));
    let response = match http.get(url.as_str(), &opts).await {
        Ok(r) => r,
        Err(e) => {
            tracing::info!("archive item not read: {}", e.kind());
            return None;
        }
    };
    let meta: Value = response.json().ok()?;
    let files = Arc::new(videos(&meta));
    let mut items = lock(&archive.items);
    if items.len() >= META_ITEMS
        && !items.contains_key(id)
        && let Some(oldest) = items
            .iter()
            .min_by_key(|(_, (at, _))| *at)
            .map(|(k, _)| k.clone())
    {
        items.remove(&oldest);
    }
    items.insert(id.to_owned(), (Instant::now(), files.clone()));
    Some(files)
}

/// The MP4 files of an item's metadata, the tallest first, at most `ROWS`.
fn videos(meta: &Value) -> Vec<File> {
    let number = |v: Option<&Value>| match v {
        Some(Value::String(s)) => s.trim().parse().ok(),
        Some(Value::Number(n)) => n.as_u64(),
        _ => None,
    };
    let mut found: Vec<File> = (meta.get("files").and_then(Value::as_array))
        .into_iter()
        .flatten()
        .take(META_FILES)
        .filter_map(|f| {
            let name = f.get("name")?.as_str()?;
            // A name that is no plain file name never becomes a row, key
            // or URL.
            let plain = name.len() <= 255 && !name.chars().any(|c| c.is_control() || hidden(c));
            let mp4 = plain && name.to_ascii_lowercase().ends_with(".mp4");
            mp4.then(|| File {
                name: name.to_owned(),
                height: number(f.get("height")).and_then(|h| u32::try_from(h).ok()),
                size: number(f.get("size")),
            })
        })
        .collect();
    found.sort_by_key(|f| std::cmp::Reverse(f.height.unwrap_or(0)));
    found.truncate(ROWS);
    found
}

/// The row for one file: `https://archive.org/download/<id>/<file>`, built
/// and percent-encoded here, its resolution from the file's height, its
/// key `archive:{id}:{file}`. None for a file name that is not a plain path.
fn stream(origin: &Url, id: &str, file: &File, index: u32) -> Option<ParsedStream> {
    let parts: Vec<&str> = file.name.split('/').collect();
    if parts
        .iter()
        .any(|p| p.is_empty() || *p == "." || *p == "..")
    {
        return None;
    }
    let mut url = origin.join("download/").ok()?;
    {
        let mut path = url.path_segments_mut().ok()?;
        path.pop_if_empty().push(id);
        for p in &parts {
            path.push(p);
        }
    }
    let wire = serde_json::json!({
        "url": url.as_str(),
        "description": file.height.map(|h| format!("{h}p")).unwrap_or_default(),
        "behaviorHints": { "videoSize": file.size },
    });
    let wire: mq_protocol::WireStream = serde_json::from_value(wire).ok()?;
    let mut s = mq_streams::normalize(&wire, "archive", NAME, 0, index);
    s.key = format!("{KEY_PREFIX}{id}:{}", file.name);
    s.addon_url = Some(link());
    s.filename = Some(clean(&file.name));
    Some(s)
}

/// Format and separator characters that change how text reads without
/// showing: bidi marks, overrides and isolates, zero widths, line and
/// paragraph separators, the soft hyphen and the BOM.
fn hidden(c: char) -> bool {
    matches!(c, '\u{00ad}' | '\u{061c}' | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}' | '\u{e0000}'..='\u{e007f}')
}

/// Item text as shown: control and `hidden` characters dropped, at most
/// `TEXT_MAX` characters.
fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() && !hidden(*c))
        .take(TEXT_MAX)
        .collect()
}

/// Deletes the list when no profile on this device that may use it (a kids
/// profile never does) has the source on (5.1).
pub(crate) fn forget_unwanted(core: &Core) {
    let Ok(profiles) = core.library().profiles() else {
        return;
    };
    let any = profiles
        .iter()
        .filter(|p| !p.kids())
        .any(|p| core.for_profile(&p.id).prefs().archive);
    if !any {
        let _ = core.library().clear_archive_index();
        let _ = core.library().set_device_value(STATE, "{}");
    }
}
