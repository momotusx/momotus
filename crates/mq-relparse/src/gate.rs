//! Pre-filter: one `RegexSet` over relaxed copies of every rule pattern.
//!
//! A relaxed pattern matches a superset of what the real pattern matches (no word boundaries,
//! ASCII-sized stand-ins for the Unicode classes). A rule whose relaxed pattern does not match the
//! current title cannot match it, so the rule is skipped without running its own regex.

use regex::{RegexSet, RegexSetBuilder, SetMatches};

use crate::engine::Handler;

const WORD_SUPER: &str = r"0-9A-Za-z_\x{80}-\x{10FFFF}";
const WORD_SUB: &str = "0-9A-Za-z_";
const NOT_WORD_SUPER: &str = r"\x00-\x2F\x3A-\x40\x5B-\x5E\x60\x7B-\x{10FFFF}";
const NOT_WORD_SUB: &str = r"\x00-\x2F\x3A-\x40\x5B-\x5E\x60\x7B-\x7F";
const DIGIT_SUPER: &str = r"0-9\x{80}-\x{10FFFF}";
const DIGIT_SUB: &str = "0-9";
const NOT_DIGIT_SUPER: &str = r"\x00-\x2F\x3A-\x{10FFFF}";
const NOT_DIGIT_SUB: &str = r"\x00-\x2F\x3A-\x7F";
const SPACE_SUPER: &str = r"\t-\r \x{80}-\x{10FFFF}";
const SPACE_SUB: &str = r"\t-\r ";
const NOT_SPACE_SUPER: &str = r"\x00-\x08\x0E-\x1F\x21-\x{10FFFF}";
const NOT_SPACE_SUB: &str = r"\x00-\x08\x0E-\x1F\x21-\x7F";

/// Class body standing in for a shorthand: a superset normally, a subset inside `[^...]`.
fn shorthand(c: char, in_negated: bool) -> &'static str {
    match (c, in_negated) {
        ('w', false) => WORD_SUPER,
        ('w', true) => WORD_SUB,
        ('W', false) => NOT_WORD_SUPER,
        ('W', true) => NOT_WORD_SUB,
        ('d', false) => DIGIT_SUPER,
        ('d', true) => DIGIT_SUB,
        ('D', false) => NOT_DIGIT_SUPER,
        ('D', true) => NOT_DIGIT_SUB,
        ('s', false) => SPACE_SUPER,
        ('s', true) => SPACE_SUB,
        ('S', false) => NOT_SPACE_SUPER,
        _ => NOT_SPACE_SUB,
    }
}

/// Relax a pattern (see module docs). Patterns here use no nested classes.
pub(crate) fn relax(pat: &str) -> String {
    let mut out = String::with_capacity(pat.len() * 2);
    let (mut in_class, mut negated) = (false, false);
    let mut chars = pat.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let Some(n) = chars.next() else { break };
                match n {
                    'b' | 'B' if !in_class => {}
                    'w' | 'W' | 'd' | 'D' | 's' | 'S' if in_class => {
                        out.push_str(shorthand(n, negated))
                    }
                    'w' | 'W' | 'd' | 'D' | 's' | 'S' => {
                        out.push('[');
                        out.push_str(shorthand(n, false));
                        out.push(']');
                    }
                    _ => {
                        out.push('\\');
                        out.push(n);
                    }
                }
            }
            '[' if !in_class => {
                in_class = true;
                negated = chars.next_if_eq(&'^').is_some();
                out.push('[');
                if negated {
                    out.push('^');
                }
            }
            ']' if in_class => {
                in_class = false;
                out.push(']');
            }
            _ => out.push(c),
        }
    }
    out
}

/// Rules per set. One huge set thrashes the lazy DFA cache, many tiny ones cost a call each.
const SHARD: usize = 64;
/// Lazy DFA cache per set (and per thread). Lower is smaller but slower, see the README.
const GATE_DFA_MB: usize = 4;

pub(crate) struct Gate {
    shards: Vec<RegexSet>,
    /// Handler index to (shard, index in shard); `None` means "always run".
    slot: Vec<Option<(usize, usize)>>,
    /// Handler index to the first shard holding a gated handler at or after it.
    resume: Vec<usize>,
}

/// Which gated handlers can match the current title.
pub(crate) struct Scan(Vec<SetMatches>);

impl Gate {
    pub fn new(handlers: &[Handler]) -> Gate {
        let mut groups: Vec<Vec<String>> = vec![Vec::new()];
        let slot: Vec<Option<(usize, usize)>> = handlers
            .iter()
            .map(|h| {
                h.gate_pattern().map(|p| {
                    if groups.last().is_some_and(|g| g.len() >= SHARD) {
                        groups.push(Vec::new());
                    }
                    let shard = groups.len() - 1;
                    let group = &mut groups[shard];
                    group.push(p);
                    (shard, group.len() - 1)
                })
            })
            .collect();
        let mut resume = vec![groups.len(); handlers.len()];
        let mut next = groups.len();
        for (i, s) in slot.iter().enumerate().rev() {
            if let Some((shard, _)) = s {
                next = *shard;
            }
            resume[i] = next;
        }
        let shards = groups.iter().map(|g| compile(g)).collect();
        Gate {
            shards,
            slot,
            resume,
        }
    }

    pub fn scan(&self, text: &str) -> Scan {
        Scan(self.shards.iter().map(|s| s.matches(text)).collect())
    }

    /// The title changed: refresh what handlers from `handler` on will read.
    pub fn rescan(&self, found: &mut Scan, handler: usize, text: &str) {
        let from = self.resume[handler];
        found.0.truncate(from);
        found
            .0
            .extend(self.shards[from..].iter().map(|s| s.matches(text)));
    }

    pub fn allows(&self, found: &Scan, handler: usize) -> bool {
        self.slot[handler].is_none_or(|(s, i)| found.0.get(s).is_none_or(|m| m.matched(i)))
    }
}

fn compile(patterns: &[String]) -> RegexSet {
    match RegexSetBuilder::new(patterns)
        .size_limit(32 << 20)
        .dfa_size_limit(GATE_DFA_MB << 20)
        .build()
    {
        Ok(s) => s,
        Err(e) => panic!("relparse gate failed to compile: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{run_all, run_ungated};
    use crate::{convert, rules::TABLE};
    use regex::Regex;

    fn inputs() -> Vec<String> {
        let golden: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../testdata/ptt_cases.json")).unwrap();
        let mut out: Vec<String> = golden
            .iter()
            .filter_map(|c| c["input"].as_str().map(String::from))
            .collect();
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/harvested");
        for entry in std::fs::read_dir(dir).unwrap() {
            let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
            for line in text.lines().step_by(7) {
                let v: serde_json::Value = serde_json::from_str(line).unwrap();
                out.extend(v["file"].as_str().map(String::from));
                out.extend(
                    v["text"]
                        .as_str()
                        .and_then(|t| t.lines().next())
                        .map(String::from),
                );
            }
        }
        out
    }

    #[test]
    fn gate_never_changes_a_result() {
        for input in inputs() {
            let (a, ea) = run_all(&TABLE, input.clone());
            let (b, eb) = run_ungated(&TABLE, input.clone());
            assert_eq!(
                convert::finish(&a, ea),
                convert::finish(&b, eb),
                "input {input:?}"
            );
        }
    }

    #[test]
    fn relaxed_pattern_is_a_superset() {
        let cases = [
            (r"\bHD\b", "xHDx"),
            (r"[^\w-](\d{1,2})-\d{2}", " 1-23"),
            (r"\d+x\W", "12x!"),
            (r"[\.\-\s\W]\d", "-5"),
        ];
        for (pat, hay) in cases {
            assert!(Regex::new(pat).is_ok());
            let relaxed = Regex::new(&relax(pat)).unwrap();
            let strict = Regex::new(pat).unwrap();
            if strict.is_match(hay) {
                assert!(relaxed.is_match(hay), "{pat} on {hay}");
            }
        }
        assert!(Regex::new(&relax(r"\bHD\b")).unwrap().is_match("xHDx"));
    }
}
