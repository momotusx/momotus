//! Required-literal analysis of rule patterns, for the pre-filter in `gate.rs`.
//!
//! [`needles`] reads a pattern and returns lowercase ASCII strings such that every match of the
//! pattern contains at least one of them (compared case-insensitively). `None` means no useful
//! literal could be proven, so the rule has to run. The analysis is conservative: anything it
//! does not understand counts as "unknown", never as a requirement.

/// Needles shorter than this select too little to be worth a check.
const MIN_LEN: usize = 2;
/// Alternations with more literals than this are not gated.
const MAX_SET: usize = 48;

#[derive(Debug)]
enum Node {
    Lit(char),
    /// Anything that consumes one character without being a known literal.
    Class,
    /// Zero-width assertion.
    Assert,
    Group(Vec<Vec<Node>>),
    Rep(Box<Node>, u32),
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        self.pos += 1;
        c
    }

    fn alternatives(&mut self) -> Vec<Vec<Node>> {
        let mut alts = vec![self.sequence()];
        while self.peek() == Some('|') {
            self.pos += 1;
            alts.push(self.sequence());
        }
        alts
    }

    fn sequence(&mut self) -> Vec<Node> {
        let mut seq = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            let atom = self.atom();
            seq.push(self.quantified(atom));
        }
        seq
    }

    fn quantified(&mut self, mut atom: Node) -> Node {
        loop {
            let min = match self.peek() {
                Some('?' | '*') => {
                    self.pos += 1;
                    0
                }
                Some('+') => {
                    self.pos += 1;
                    1
                }
                Some('{') => self.counted(),
                _ => return atom,
            };
            if self.peek() == Some('?') {
                self.pos += 1;
            }
            atom = Node::Rep(Box::new(atom), min);
        }
    }

    /// Parse `{m}`, `{m,}` or `{m,n}` and return `m`; `pos` ends after the `}`.
    fn counted(&mut self) -> u32 {
        self.pos += 1;
        let mut digits = String::new();
        while let Some(c) = self.bump() {
            match c {
                '}' => break,
                ',' => {
                    while self.bump().is_some_and(|c| c != '}') {}
                    break;
                }
                d if d.is_ascii_digit() => digits.push(d),
                _ => break,
            }
        }
        digits.parse().unwrap_or(0)
    }

    fn atom(&mut self) -> Node {
        match self.bump() {
            Some('(') => self.group(),
            Some('[') => {
                self.class();
                Node::Class
            }
            Some('.') => Node::Class,
            Some('^' | '$') => Node::Assert,
            Some('\\') => self.escape(),
            Some(c) => literal(c),
            None => Node::Assert,
        }
    }

    fn group(&mut self) -> Node {
        if self.peek() == Some('?') {
            self.pos += 1;
            match self.peek() {
                Some(':') => self.pos += 1,
                Some('<' | 'P') => {
                    while self.peek().is_some_and(|c| c != '>') {
                        self.pos += 1;
                    }
                    self.pos += 1;
                }
                _ => {
                    // flag group such as `(?i)` or `(?i:`
                    while let Some(c) = self.bump() {
                        if c == ')' {
                            return Node::Assert;
                        }
                        if c == ':' {
                            break;
                        }
                    }
                }
            }
        }
        let alts = self.alternatives();
        if self.peek() == Some(')') {
            self.pos += 1;
        }
        Node::Group(alts)
    }

    /// Skip a character class (the opening `[` is consumed).
    fn class(&mut self) {
        if self.peek() == Some('^') {
            self.pos += 1;
        }
        if self.peek() == Some(']') {
            self.pos += 1;
        }
        while let Some(c) = self.bump() {
            match c {
                '\\' => self.pos += 1,
                ']' => return,
                _ => {}
            }
        }
    }

    fn escape(&mut self) -> Node {
        match self.bump() {
            Some('b' | 'B' | 'A' | 'z') => Node::Assert,
            Some('d' | 'D' | 'w' | 'W' | 's' | 'S') => Node::Class,
            Some('p' | 'P') => {
                if self.peek() == Some('{') {
                    while self.peek().is_some_and(|c| c != '}') {
                        self.pos += 1;
                    }
                    self.pos += 1;
                } else {
                    self.pos += 1;
                }
                Node::Class
            }
            Some('x' | 'u' | 'U') => {
                if self.peek() == Some('{') {
                    while self.peek().is_some_and(|c| c != '}') {
                        self.pos += 1;
                    }
                    self.pos += 1;
                } else {
                    self.pos += 2;
                }
                Node::Class
            }
            Some(c) if c.is_ascii_alphanumeric() => Node::Class,
            Some(c) => literal(c),
            None => Node::Assert,
        }
    }
}

fn literal(c: char) -> Node {
    if c.is_ascii() && !c.is_ascii_control() {
        Node::Lit(c.to_ascii_lowercase())
    } else {
        Node::Class
    }
}

/// Needles of a node sequence, the most selective option found.
fn sequence_needles(seq: &[Node]) -> Option<Vec<String>> {
    let mut best: Option<Vec<String>> = None;
    let mut run = String::new();
    let mut offer = |cand: Vec<String>, best: &mut Option<Vec<String>>| {
        if score(&cand) >= MIN_LEN && best.as_ref().is_none_or(|b| better(&cand, b)) {
            *best = Some(cand);
        }
    };
    for node in seq {
        match node {
            Node::Lit(c) => run.push(*c),
            Node::Assert => {}
            Node::Class | Node::Rep(_, 0) => flush(&mut run, &mut best, &mut offer),
            Node::Rep(inner, _) => {
                if let Node::Lit(c) = **inner {
                    run.push(c);
                    flush(&mut run, &mut best, &mut offer);
                } else {
                    flush(&mut run, &mut best, &mut offer);
                    if let Some(set) = node_needles(inner) {
                        offer(set, &mut best);
                    }
                }
            }
            Node::Group(_) => {
                flush(&mut run, &mut best, &mut offer);
                if let Some(set) = node_needles(node) {
                    offer(set, &mut best);
                }
            }
        }
    }
    flush(&mut run, &mut best, &mut offer);
    best
}

fn flush(
    run: &mut String,
    best: &mut Option<Vec<String>>,
    offer: &mut impl FnMut(Vec<String>, &mut Option<Vec<String>>),
) {
    if !run.is_empty() {
        offer(vec![std::mem::take(run)], best);
    }
}

fn node_needles(node: &Node) -> Option<Vec<String>> {
    let Node::Group(alts) = node else { return None };
    let mut all: Vec<String> = Vec::new();
    for alt in alts {
        all.extend(sequence_needles(alt)?);
        if all.len() > MAX_SET {
            return None;
        }
    }
    Some(all)
}

fn score(set: &[String]) -> usize {
    set.iter().map(String::len).min().unwrap_or(0)
}

fn better(a: &[String], b: &[String]) -> bool {
    (score(a), usize::MAX - a.len()) > (score(b), usize::MAX - b.len())
}

/// Lowercase literals of which every match contains at least one, or `None`.
pub(crate) fn needles(pat: &str) -> Option<Vec<String>> {
    let mut p = Parser {
        chars: pat.chars().collect(),
        pos: 0,
    };
    let alts = p.alternatives();
    let mut set = node_needles(&Node::Group(alts))?;
    set.sort();
    set.dedup();
    Some(set)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(p: &str) -> Option<Vec<String>> {
        needles(p)
    }

    #[test]
    fn literals_and_alternations() {
        assert_eq!(n(r"\bBlu[ .-]*Ray\b"), Some(vec!["blu".into()]));
        assert_eq!(
            n(r"\b(?:JP|JAP|JPN)\b"),
            Some(vec!["jap".into(), "jp".into(), "jpn".into()])
        );
        assert_eq!(n(r"\bHD(.?TV)?\b"), Some(vec!["hd".into()]));
        assert_eq!(n(r"[hx][\. \-]?264"), Some(vec!["264".into()]));
    }

    #[test]
    fn optional_and_unknown_parts_are_not_required() {
        assert_eq!(n(r"(?:foo)?"), None);
        assert_eq!(n(r"\d{1,2}[xх]\d{1,3}"), None);
        assert_eq!(n(r"(\d+)(?:a|b)?"), None);
        assert_eq!(n(r"a|\d+"), None);
    }

    #[test]
    fn picks_the_most_selective() {
        assert_eq!(n(r"\bs(\d{1,3})(?:[\Wex]|$)"), None);
        assert_eq!(n(r"\.torrent$"), Some(vec![".torrent".into()]));
        assert_eq!(n(r"Season[. ]?(\d+)"), Some(vec!["season".into()]));
    }
}
