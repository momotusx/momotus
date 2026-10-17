//! Pre-filter: a rule can only match a title that contains one of its needles.
//!
//! Needles are required literals read from the rule's pattern (see `needle.rs`). Checking them
//! against the lowercased title costs a few nanoseconds per rule, so the regex of a rule runs
//! only when its literal is present. Rules without a provable literal always run. A 128-bit
//! bigram bloom of the title rejects most needles before the substring search.

use crate::engine::Handler;
use crate::needle::needles;

struct Needle {
    text: String,
    mask: u128,
}

/// Which bigrams a byte string contains, hashed into 128 bits.
fn bigrams(bytes: &[u8]) -> u128 {
    bytes.windows(2).fold(0, |m, w| {
        let h = (u32::from(w[0]) * 31 + u32::from(w[1])) % 128;
        m | (1u128 << h)
    })
}

/// The current title as the gate sees it.
pub(crate) struct View {
    lower: String,
    mask: u128,
}

impl View {
    pub fn new(title: &str) -> View {
        let mut view = View {
            lower: String::with_capacity(title.len() + 8),
            mask: 0,
        };
        view.reset(title);
        view
    }

    /// Point the view at the new title text, reusing the buffer.
    pub fn reset(&mut self, title: &str) {
        self.lower.clear();
        if title.is_ascii() {
            self.lower.push_str(title);
            self.lower.make_ascii_lowercase();
        } else {
            self.lower.push_str(&title.to_lowercase());
        }
        self.mask = bigrams(self.lower.as_bytes());
    }
}

pub(crate) struct Gate {
    /// Per handler: `None` means "always run".
    sets: Vec<Option<Vec<Needle>>>,
}

impl Gate {
    pub fn new(handlers: &[Handler]) -> Gate {
        let sets = handlers
            .iter()
            .map(|h| h.needles().map(|set| set.into_iter().map(make).collect()))
            .collect();
        Gate { sets }
    }

    pub fn allows(&self, view: &View, handler: usize) -> bool {
        match &self.sets[handler] {
            None => true,
            Some(set) => set
                .iter()
                .any(|n| n.mask & !view.mask == 0 && view.lower.contains(n.text.as_str())),
        }
    }
}

fn make(text: String) -> Needle {
    let mask = bigrams(text.as_bytes());
    Needle { text, mask }
}

/// Needles of a rule: the union over its alternatives, or `None` if one has none.
pub(crate) fn rule_needles(patterns: &[&str]) -> Option<Vec<String>> {
    let mut all = Vec::new();
    for p in patterns {
        all.extend(needles(p)?);
    }
    all.sort();
    all.dedup();
    Some(all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{run_all, run_ungated};
    use crate::{convert, rules::TABLE};
    use proptest::prelude::*;

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
            for line in text.lines() {
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

    fn same(input: &str) {
        let (a, ea) = run_all(&TABLE, input.to_string());
        let (b, eb) = run_ungated(&TABLE, input.to_string());
        assert_eq!(
            convert::finish(&a, ea),
            convert::finish(&b, eb),
            "input {input:?}"
        );
    }

    #[test]
    fn gate_never_changes_a_result() {
        inputs().iter().for_each(|i| same(i));
    }

    #[test]
    fn bloom_is_a_necessary_condition() {
        let v = View::new("The.Matrix.1999.1080p.BluRay.x264");
        let n = make("bluray".into());
        assert!(n.mask & !v.mask == 0 && v.lower.contains("bluray"));
        let miss = make("remux".into());
        assert!(miss.mask & !v.mask != 0);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(400))]
        #[test]
        fn gate_is_exact_on_token_soup(
            tokens in proptest::collection::vec(
                prop_oneof![
                    Just("S01E02"), Just("1080p"), Just("x265"), Just("2160p"), Just("[GRP]"),
                    Just("(2019)"), Just("DTS-HD.MA"), Just("5.1"), Just("-"), Just("."), Just(" "),
                    Just("720p"), Just("Season"), Just("Complete"), Just("1-12"), Just("Vol.2"),
                    Just("mkv"), Just("WEB-DL"), Just("BluRay"), Just("REMUX"), Just("HDR"),
                    Just("DV"), Just("Dual"), Just("Audio"), Just("ITA"), Just("ENG"), Just("PL"),
                    Just("Игра"), Just("é"), Just("PROPER"), Just("3D"), Just("Extended"),
                    Just("One Piece"), Just("AAC"), Just("2.0"), Just("EP05"), Just("[1080p]"),
                ],
                0..40,
            )
        ) {
            same(&tokens.concat());
        }
    }
}
