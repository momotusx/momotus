//! Title clean-up (PTT `clean_title`).

use regex::Regex;
use std::sync::LazyLock;

use crate::engine::{F, Rule, build, next_char, rule};
use crate::transform::Tf;

/// Scripts PTT treats as "non English" (kana, CJK, Cyrillic, Arabic, Kannada, Malayalam, Thai).
macro_rules! non_english {
    () => {
        r"\u{3040}-\u{30ff}\u{3400}-\u{4dbf}\u{4e00}-\u{9fff}\u{f900}-\u{faff}\u{ff66}-\u{ff9f}\u{0400}-\u{04ff}\u{0600}-\u{06ff}\u{0750}-\u{077f}\u{0c80}-\u{0cff}\u{0d00}-\u{0d7f}\u{0e00}-\u{0e7f}"
    };
}

macro_rules! lazy {
    ($pat:expr) => {
        LazyLock::new(|| build($pat, false))
    };
}

static MOVIE: LazyLock<Regex> = LazyLock::new(|| build(r"[\[(]movie[)\]]", true));
static EDGE_SYMBOLS: LazyLock<Regex> = lazy!(concat!(
    r"^[^\w",
    non_english!(),
    r"#\[【★]+|[ \-:/\\\[|{(#$&^]+$"
));
static STAR_1: LazyLock<Regex> = lazy!(r"^[\[【★].*[\]】★][ .]?(.+)");
static STAR_2: LazyLock<Regex> = lazy!(r"(.+)[ .]?[\[【★].*[\]】★]$");
static ALT_TITLES: LazyLock<Regex> = lazy!(concat!(
    r"[^/|(]*[",
    non_english!(),
    r"][^/|]*[/|]|[/|][^/|(]*[",
    non_english!(),
    r"][^/|]*"
));
static EDGE_REMAINING: LazyLock<Regex> = lazy!(concat!(r"^[^\w", non_english!(), r"#]+|\]$"));
static EMPTY_BRACKETS: LazyLock<Regex> = lazy!(r"\(\s*\)|\[\s*\]|\{\s*\}");
static MP3: LazyLock<Regex> = lazy!(r"\bmp3$");
static BRACKETS_NO_CONTENT: LazyLock<Regex> = lazy!(r"\(\W*\)|\[\W*\]|\{\W*\}");
static SPECIAL_SPACING: LazyLock<Regex> = lazy!(r"[\-\+_\{\}\[\]]\W{2,}");
static TRAILING: LazyLock<Regex> = lazy!(r"[ \-:./\\]+$");
static SPACES: LazyLock<Regex> = lazy!(r"\s+");

static CAST: LazyLock<Rule> = LazyLock::new(|| {
    rule(F::Title, r"\([^)]*[\u{0400}-\u{04ff}][^)]*\)$", Tf::Raw)
        .alt(r"\(.*\)$")
        .pb(r"/.*")
});

static NOT_ONLY_NON_ENGLISH: LazyLock<Rule> = LazyLock::new(|| {
    rule(
        F::Title,
        concat!(r"[", non_english!(), r"].*[", non_english!(), r"]"),
        Tf::Raw,
    )
    .pb(concat!(r"[a-zA-Z][^", non_english!(), r"]+"))
    .alt(concat!(r"[", non_english!(), r"].*[", non_english!(), r"]"))
    .la(concat!(r"[^", non_english!(), r"]+[a-zA-Z]"))
});

/// `regex.sub(pattern, "", text)` for rules that need guards.
fn strip_matches(r: &Rule, text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut from = 0;
    while let Some(f) = r.find_from(text, from) {
        out.push_str(&text[last..f.start]);
        last = f.end;
        from = if f.end > f.start {
            f.end
        } else {
            next_char(text, f.end)
        };
        if from > text.len() {
            break;
        }
    }
    out.push_str(&text[last.min(text.len())..]);
    out
}

fn remove_unbalanced(mut s: String) -> String {
    for (open, close) in [('{', '}'), ('[', ']'), ('(', ')')] {
        let (o, c) = (s.matches(open).count(), s.matches(close).count());
        if o != c {
            s.retain(|ch| ch != open && ch != close);
        }
    }
    s
}

pub(crate) fn clean_title(raw: &str) -> String {
    let mut t = raw.replace('_', " ");
    t = MOVIE.replace_all(&t, "").into_owned();
    t = EDGE_SYMBOLS.replace_all(&t, "").into_owned();
    t = strip_matches(&CAST, &t);
    t = STAR_1.replace_all(&t, "$1").into_owned();
    t = STAR_2.replace_all(&t, "$1").into_owned();
    t = ALT_TITLES.replace_all(&t, "").into_owned();
    t = strip_matches(&NOT_ONLY_NON_ENGLISH, &t);
    for re in [
        &EDGE_REMAINING,
        &EMPTY_BRACKETS,
        &MP3,
        &BRACKETS_NO_CONTENT,
        &SPECIAL_SPACING,
    ] {
        t = re.replace_all(&t, "").into_owned();
    }
    t = remove_unbalanced(t);
    if !t.contains(' ') && t.contains('.') {
        t = t.replace('.', " ");
    }
    t = TRAILING.replace_all(&t, "").into_owned();
    SPACES.replace_all(&t, " ").trim().to_string()
}
