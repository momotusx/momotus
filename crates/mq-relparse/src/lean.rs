//! Small regexes. Release names are short, but the `regex` crate pays about 100 KB for every
//! pattern that uses a Unicode `\w`, `\W` or `\d`, and a lazy DFA and prefilters per pattern. With
//! several hundred rules that is tens of megabytes, so rules are compiled here instead:
//!
//! * `\w`, `\W` and `\d` become compact classes (ASCII plus the word-letter blocks of the scripts
//!   that show up in release names; punctuation blocks are left out, so `¿`, `«` or `【` still
//!   separate words). Digits are ASCII only.
//! * the engine is `regex-automata`'s meta regex with only the NFA engines (no lazy DFA, one-pass
//!   DFA or prefilter). A needle gate (`gate.rs`) already keeps most rules from running.
//!
//! Matching stays linear time: the engines used are the Pike VM and the bounded backtracker.

use regex::RegexBuilder;
use regex_automata::{
    Input, PatternID,
    meta::{Config, Regex as Meta},
    nfa::thompson::{
        self, Compiler, WhichCaptures,
        backtrack::{self, BoundedBacktracker},
        pikevm::PikeVM,
    },
    util::{primitives::NonMaxUsize, syntax},
};
use std::cell::RefCell;
use std::sync::LazyLock;

/// Lazy DFA cache per fast regex and thread.
const HYBRID_CACHE_BYTES: usize = 64 << 10;

/// Non-ASCII word characters, as inclusive code point ranges.
const WORD_RANGES: &[(u32, u32)] = &[
    (0x00AA, 0x00AA),
    (0x00B5, 0x00B5),
    (0x00BA, 0x00BA),
    (0x00C0, 0x02AF),
    (0x0300, 0x036F),
    (0x0370, 0x052F),
    (0x0531, 0x0587),
    (0x0591, 0x05EA),
    (0x0620, 0x06FF),
    (0x0900, 0x0E5B),
    (0x10A0, 0x11FF),
    (0x1E00, 0x1FFF),
    // Ω, K (Kelvin sign) and Å: the Kelvin sign case-folds to "k", so leaving it out of the word
    // class would make `\W` match "k" in case-insensitive rules.
    (0x2126, 0x2126),
    (0x212A, 0x212B),
    (0x2160, 0x2188),
    (0x3041, 0x30FA),
    (0x30FC, 0x30FF),
    (0x3105, 0x318E),
    (0x3400, 0x9FFF),
    (0xAC00, 0xD7A3),
    (0xF900, 0xFAFF),
    (0xFF10, 0xFF19),
    (0xFF21, 0xFF3A),
    (0xFF41, 0xFF5A),
    (0xFF66, 0xFFDC),
    (0x20000, 0x2FFFF),
];

fn range_text(ranges: &[(u32, u32)]) -> String {
    ranges
        .iter()
        .map(|(a, b)| format!(r"\x{{{a:X}}}-\x{{{b:X}}}"))
        .collect()
}

/// Body of a class that matches `\w` (ASCII word characters plus [`WORD_RANGES`]).
static WORD_BODY: LazyLock<String> =
    LazyLock::new(|| format!("0-9A-Za-z_{}", range_text(WORD_RANGES)));

/// Body of a class that matches `\W`: the complement of [`WORD_BODY`].
static NOT_WORD_BODY: LazyLock<String> = LazyLock::new(|| {
    let mut out: Vec<(u32, u32)> = vec![
        (0x00, 0x2F),
        (0x3A, 0x40),
        (0x5B, 0x5E),
        (0x60, 0x60),
        (0x7B, 0x7F),
    ];
    let mut from = 0x80;
    for &(a, b) in WORD_RANGES {
        if a > from {
            out.push((from, a - 1));
        }
        from = b + 1;
    }
    out.push((from, 0x10FFFF));
    range_text(&out)
});

/// Rewrite `\w`, `\W`, `\d` and `\D` (inside or outside classes) into compact classes.
pub(crate) fn compact(pat: &str) -> String {
    let mut out = String::with_capacity(pat.len() * 2);
    let mut in_class = false;
    let mut chars = pat.chars();
    while let Some(c) = chars.next() {
        match (c, in_class) {
            ('\\', _) => match chars.next() {
                Some(n @ ('w' | 'W' | 'd' | 'D')) => push_shorthand(&mut out, n, in_class),
                Some(n) => {
                    out.push('\\');
                    out.push(n);
                }
                None => out.push('\\'),
            },
            ('[', false) => {
                in_class = true;
                out.push(c);
            }
            (']', true) => {
                in_class = false;
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

fn push_shorthand(out: &mut String, which: char, in_class: bool) {
    let body = match which {
        'w' => WORD_BODY.as_str(),
        'W' => NOT_WORD_BODY.as_str(),
        'd' => "0-9",
        _ => r"\x00-/:-\x{10FFFF}",
    };
    if in_class {
        out.push_str(body);
        return;
    }
    out.push('[');
    out.push_str(body);
    out.push(']');
}

/// A compiled rule pattern in one of two forms.
pub(crate) enum Re {
    /// Just the NFA, searched by the bounded backtracker (Pike VM when the text is too long).
    /// No cache is kept between searches: a few KB per pattern, microseconds per search.
    Nfa(Box<Nfa>),
    /// The meta regex with a lazy DFA: about ten times faster per search, several times larger.
    Meta(Box<Meta>),
}

pub(crate) struct Nfa {
    backtrack: BoundedBacktracker,
    pike: PikeVM,
    slots: usize,
    groups: usize,
}

type Slots = Vec<Option<NonMaxUsize>>;

/// Patterns with at most this many capture slots (8 groups) need no heap allocation per search.
const STACK_SLOTS: usize = 16;

thread_local! {
    /// One backtracker cache per thread, reset for whichever pattern is searched next.
    static SCRATCH: RefCell<Option<backtrack::Cache>> = const { RefCell::new(None) };
}

impl Nfa {
    fn new(pat: &str, ci: bool) -> Result<Nfa, String> {
        let nfa = Compiler::new()
            .configure(thompson::Config::new().which_captures(WhichCaptures::All))
            .syntax(syntax::Config::new().case_insensitive(ci))
            .build(&compact(pat))
            .map_err(|e| e.to_string())?;
        let info = nfa.group_info().clone();
        Ok(Nfa {
            backtrack: BoundedBacktracker::new_from_nfa(nfa.clone()).map_err(|e| e.to_string())?,
            pike: PikeVM::new_from_nfa(nfa).map_err(|e| e.to_string())?,
            slots: info.slot_len(),
            groups: info.group_len(PatternID::ZERO),
        })
    }

    fn search(&self, input: &Input<'_>, slots: &mut [Option<NonMaxUsize>]) -> bool {
        let tried = SCRATCH.with(|scratch| {
            let mut scratch = scratch.borrow_mut();
            let cache = match scratch.as_mut() {
                Some(c) => {
                    c.reset(&self.backtrack);
                    c
                }
                None => scratch.insert(self.backtrack.create_cache()),
            };
            self.backtrack.try_search_slots(cache, input, slots)
        });
        match tried {
            Ok(found) => found.is_some(),
            Err(_) => {
                slots.iter_mut().for_each(|s| *s = None);
                let mut cache = self.pike.create_cache();
                self.pike.search_slots(&mut cache, input, slots).is_some()
            }
        }
    }

    fn is_match(&self, input: &Input<'_>) -> bool {
        self.search(&input.clone().earliest(true), &mut [])
    }

    fn captures(&self, input: &Input<'_>) -> Option<Groups> {
        let mut stack = [None; STACK_SLOTS];
        let mut heap: Slots;
        let slots = if self.slots <= STACK_SLOTS {
            &mut stack[..self.slots]
        } else {
            heap = vec![None; self.slots];
            &mut heap[..]
        };
        if !self.search(input, slots) {
            return None;
        }
        let spans = (0..self.groups)
            .map(|i| match (slots[2 * i], slots[2 * i + 1]) {
                (Some(a), Some(b)) => Some((a.get(), b.get())),
                _ => None,
            })
            .collect();
        Some(Groups(spans))
    }
}

impl Re {
    /// The small form (see [`Re::Nfa`]).
    pub fn new(pat: &str, ci: bool) -> Re {
        match Nfa::new(pat, ci) {
            Ok(n) => Re::Nfa(Box::new(n)),
            Err(e) => panic!("invalid relparse pattern {pat:?}: {e}"),
        }
    }

    /// The form with a lazy DFA, for rules that run on many titles.
    pub fn fast(pat: &str, ci: bool) -> Re {
        let cfg = Config::new()
            .hybrid_cache_capacity(HYBRID_CACHE_BYTES)
            .dfa(false)
            .onepass(false)
            .auto_prefilter(false);
        let built = Meta::builder()
            .configure(cfg)
            .syntax(syntax::Config::new().case_insensitive(ci))
            .build(&compact(pat));
        match built {
            Ok(re) => Re::Meta(Box::new(re)),
            Err(e) => panic!("invalid relparse pattern {pat:?}: {e}"),
        }
    }

    pub fn is_match(&self, text: &str) -> bool {
        match self {
            Re::Nfa(n) => n.is_match(&Input::new(text)),
            Re::Meta(m) => m.is_match(text),
        }
    }

    /// Leftmost match starting at or after `from`, with the span of every group. Look-behind
    /// assertions (`\b`, `^`) still see the text before `from`.
    pub fn captures_at(&self, text: &str, from: usize) -> Option<Groups> {
        let input = Input::new(text).span(from..text.len());
        match self {
            Re::Nfa(n) => n.captures(&input),
            Re::Meta(m) => {
                m.search(&input)?;
                let mut caps = m.create_captures();
                m.search_captures(&input, &mut caps);
                if !caps.is_match() {
                    return None;
                }
                let spans = (0..caps.group_len())
                    .map(|i| caps.get_group(i).map(|s| (s.start, s.end)))
                    .collect();
                Some(Groups(spans))
            }
        }
    }
}

/// Spans of the capture groups of one match (group 0 is the whole match).
pub(crate) struct Groups(Vec<Option<(usize, usize)>>);

impl Groups {
    pub fn span(&self, i: usize) -> Option<(usize, usize)> {
        self.0.get(i).copied().flatten()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn text<'t>(&self, text: &'t str, i: usize) -> Option<&'t str> {
        self.span(i).map(|(a, b)| &text[a..b])
    }
}

/// A `regex` crate pattern with compact classes, for the few fixed patterns outside the rule table.
pub(crate) fn std_regex(pat: &str, ci: bool) -> regex::Regex {
    match RegexBuilder::new(&compact(pat))
        .case_insensitive(ci)
        .build()
    {
        Ok(re) => re,
        Err(e) => panic!("invalid relparse pattern {pat:?}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_classes_keep_word_semantics() {
        let w = std_regex(r"^\w+$", false);
        assert!(
            w.is_match("Game_1") && w.is_match("Игра") && w.is_match("映画") && w.is_match("café")
        );
        assert!(!w.is_match("a b") && !w.is_match("¿qué") && !w.is_match("【x】"));
        let nw = std_regex(r"^\W+$", false);
        assert!(nw.is_match(" .-/【】«»") && !nw.is_match("a"));
        let mixed = std_regex(r"[\W\d]x", false);
        assert!(mixed.is_match("5x") && mixed.is_match(" x") && !mixed.is_match("ax"));
        assert!(std_regex(r"[^\w-]x", false).is_match(" x"));
    }

    #[test]
    fn case_folding_does_not_make_k_a_non_word() {
        // U+212A KELVIN SIGN folds to "k"; it must stay out of the `\W` class.
        for ci in [false, true] {
            assert!(!std_regex(r"^\W$", ci).is_match("k"));
            assert!(!std_regex(r"^\W$", ci).is_match("K"));
            assert!(!Re::new(r"^\W$", ci).is_match("k"));
            assert!(!Re::fast(r"^\W$", ci).is_match("K"));
        }
    }

    #[test]
    fn both_engines_agree() {
        let pats = [
            r"\b(\d{1,2})x(\d{2})\b",
            r"(?:[\W\d]|^)e[ .]?(\d+)(?:\W|$)",
            r"[^SE](19\d\d)",
        ];
        let hay = [
            "Show 1x05 HD",
            "Show.E12.720p",
            "xx 1999 end",
            "no match here",
        ];
        for p in pats {
            let (lean, fast) = (Re::new(p, true), Re::fast(p, true));
            for h in hay {
                let a = lean
                    .captures_at(h, 0)
                    .map(|g| (0..g.len()).map(|i| g.span(i)).collect::<Vec<_>>());
                let b = fast
                    .captures_at(h, 0)
                    .map(|g| (0..g.len()).map(|i| g.span(i)).collect::<Vec<_>>());
                assert_eq!(a, b, "{p} on {h}");
            }
        }
    }

    #[test]
    fn lean_regex_finds_with_context() {
        let re = Re::new(r"\bHD\b", true);
        let g = re.captures_at("x HD y", 0).unwrap();
        assert_eq!(g.span(0), Some((2, 4)));
        assert!(re.captures_at("xHD", 1).is_none());
    }
}
