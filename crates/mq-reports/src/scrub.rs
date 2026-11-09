//! The scrubber for a report's free text (`message` and `stack` lines),
//! plans/error-reports.md section 4. Rules run in a fixed order and every
//! replacement is a placeholder no rule matches again, so `scrub` is
//! idempotent: the relay runs it a second time and changes nothing on a
//! clean batch.

use regex::{Captures, Regex};
use std::sync::LazyLock;

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("a valid pattern")
}

// Rule 1: links, with or without a scheme, and magnet links.
static URL: LazyLock<Regex> =
    LazyLock::new(|| re(r#"[A-Za-z][A-Za-z0-9+.-]*://[^\s"'<>()\[\]{}`]+"#));
static MAGNET: LazyLock<Regex> = LazyLock::new(|| re(r#"magnet:\?[^\s"'<>`]+"#));
static BARE_URL: LazyLock<Regex> =
    LazyLock::new(|| re(r#"\b(?:[A-Za-z0-9-]+\.)+[A-Za-z]{2,}(?::\d+)?/[^\s"'<>()\[\]{}`]*"#));
// Rule 2: secrets by their label. A value that is already a placeholder
// (`<url>`, `<text>`) stays, which keeps the rule idempotent.
static AUTHORIZATION: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(authorization\s*:\s*)[^\n]*"));
static BEARER: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(bearer\s+)[^\s<][^\s]*"));
static KEY_VALUE: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r#"(?i)\b([A-Za-z0-9_-]*(?:key|token|secret|pass|auth|session|pin|code|cookie)[A-Za-z0-9_-]*\s*=\s*)[^\s&"',;<][^\s&"',;]*"#,
    )
});
// Rule 3: double and typographic quotes (single quotes: `single_quotes`).
static QUOTED: LazyLock<Regex> = LazyLock::new(|| re(r#""[^"\n]*"|“[^”\n]*”|‘[^’\n]*’"#));
// Rule 4: an absolute path, from a boundary to the end of the line or a quote.
static PATH: LazyLock<Regex> =
    LazyLock::new(|| re(r#"(?m)(^|[\s=(:'"])(?:~/|/[\w.~-]|[A-Za-z]:\\|file:)[^\n"“”]*"#));
// Rule 5: addresses.
static EMAIL: LazyLock<Regex> =
    LazyLock::new(|| re(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}"));
static MAC: LazyLock<Regex> = LazyLock::new(|| re(r"\b[0-9A-Fa-f]{2}(?::[0-9A-Fa-f]{2}){5}\b"));
static IPV4: LazyLock<Regex> = LazyLock::new(|| re(r"\b(?:\d{1,3}\.){3}\d{1,3}\b"));
static IPV6: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?:[0-9A-Fa-f]{1,4})?(?::[0-9A-Fa-f]{0,4}){2,7}(?:%[A-Za-z0-9]+)?"));
static LOCAL_HOST: LazyLock<Regex> = LazyLock::new(|| re(r"\b[A-Za-z0-9-]+\.local\b"));
// Rule 6: title ids.
static TITLE_ID: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)\btt\d{4,}\b|\b(?:kitsu|tmdb|tvdb|mal|anilist|imdb|trakt)[:_][^\s,;)\]]+")
});
// Rule 7: media file names and release names.
static MEDIA_FILE: LazyLock<Regex> = LazyLock::new(|| {
    re(r#"[^\s/\\<>"']+\.(?:mkv|mp4|m4v|avi|webm|m3u8|mpd|srt|vtt|ass|ssa|sub)\b"#)
});
static WORD_RUN: LazyLock<Regex> = LazyLock::new(|| re(r"[A-Za-z0-9][A-Za-z0-9._-]*"));
static YEAR_IN_RUN: LazyLock<Regex> =
    LazyLock::new(|| re(r"[A-Za-z0-9-][._](?:19|20)\d\d[._][A-Za-z0-9-]"));
static RESOLUTION: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?:^|[._-])(?:480|720|1080|2160)p(?:$|[._-])"));
static BRACKET_YEAR: LazyLock<Regex> =
    LazyLock::new(|| re(r#"(?:[^\s:,;"<>()\[\]]+ )*[(\[](?:19|20)\d\d[)\]]"#));
// Rule 8: identifiers and keys.
static UUID: LazyLock<Regex> = LazyLock::new(|| {
    re(r"\b[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}\b")
});
static HEX: LazyLock<Regex> = LazyLock::new(|| re(r"\b[0-9A-Fa-f]{32,}\b"));
static JWT: LazyLock<Regex> =
    LazyLock::new(|| re(r"eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+"));
static TOKEN: LazyLock<Regex> = LazyLock::new(|| re(r"[A-Za-z0-9]+"));

/// The shortest run of letters and digits taken for a key. Real-Debrid's,
/// AllDebrid's and Premiumize's keys are longer; TorBox's is a UUID.
pub const TOKEN_FLOOR: usize = 16;

/// `text` with everything a report must never carry replaced by a
/// placeholder (plans/error-reports.md 3.2, 4).
pub fn scrub(text: &str) -> String {
    // 1
    let s = URL.replace_all(text, "<url>");
    let s = MAGNET.replace_all(&s, "<magnet>");
    let s = BARE_URL.replace_all(&s, "<url>");
    // 2
    let s = AUTHORIZATION.replace_all(&s, "${1}<secret>");
    let s = BEARER.replace_all(&s, "${1}<secret>");
    let s = KEY_VALUE.replace_all(&s, "${1}<secret>");
    // 3
    let s = QUOTED.replace_all(&s, "<text>");
    let s = single_quotes(&s);
    // 4
    let s = PATH.replace_all(&s, "${1}<path>");
    // 5
    let s = EMAIL.replace_all(&s, "<email>");
    let s = MAC.replace_all(&s, "<mac>");
    let s = IPV4.replace_all(&s, "<ip>");
    let s = replace_where(&IPV6, &s, "<ip>", |m, before, after| {
        let colons = m.matches(':').count();
        !word(before)
            && !word(after)
            && m.bytes().any(|b| b.is_ascii_digit())
            && (m.contains("::") || colons >= 4)
    });
    let s = LOCAL_HOST.replace_all(&s, "<host>");
    // 6
    let s = TITLE_ID.replace_all(&s, "<title>");
    // 7
    let s = MEDIA_FILE.replace_all(&s, "<file>");
    let s = replace_where(&WORD_RUN, &s, "<file>", |m, before, _| {
        !word(before) && (YEAR_IN_RUN.is_match(m) || RESOLUTION.is_match(m))
    });
    let s = BRACKET_YEAR.replace_all(&s, "<file>");
    // 8
    let s = UUID.replace_all(&s, "<uuid>");
    let s = HEX.replace_all(&s, "<hash>");
    let s = JWT.replace_all(&s, "<token>");
    replace_where(&TOKEN, &s, "<token>", |m, _, _| {
        m.len() >= TOKEN_FLOOR
            && m.bytes().any(|b| b.is_ascii_digit())
            && m.bytes().any(|b| b.is_ascii_alphabetic())
    })
}

fn word(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// `re`'s matches replaced by `with` where `keep` says so, given the
/// characters just before and after the match.
fn replace_where(
    re: &Regex,
    text: &str,
    with: &str,
    keep: impl Fn(&str, Option<char>, Option<char>) -> bool,
) -> String {
    re.replace_all(text, |c: &Captures| {
        let m = c.get(0).expect("the whole match");
        let before = text[..m.start()].chars().next_back();
        let after = text[m.end()..].chars().next();
        if keep(m.as_str(), before, after) {
            with.to_owned()
        } else {
            m.as_str().to_owned()
        }
    })
    .into_owned()
}

/// Rule 3 for single quotes: a pair on one line becomes `<text>`. A quote
/// between two letters is an apostrophe (`can't`), not a quote.
fn single_quotes(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let quote = |i: usize| {
        chars[i] == '\''
            && !(i > 0
                && chars[i - 1].is_alphanumeric()
                && chars.get(i + 1).is_some_and(|c| c.is_alphanumeric()))
    };
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if quote(i)
            && let Some(end) = (i + 1..chars.len())
                .take_while(|&j| chars[j] != '\n')
                .find(|&j| quote(j))
        {
            out.push_str("<text>");
            i = end + 1;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}
