//! Handler engine: a table of regex rules applied in order to a shrinking title.
//!
//! Semantics follow PTT's `Parser.parse` (MIT, see README NOTICE). Look-arounds are not
//! available in linear-time regex engines, so rules carry explicit [`Look`] guards that
//! inspect the text before or after a match (or capture group) instead.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use crate::gate::{Gate, View, rule_needles};
use crate::lean::{Groups, Re};
use crate::transform::{Tf, Val};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(usize)]
pub(crate) enum F {
    Title,
    Year,
    Container,
    Torrent,
    Adult,
    Scene,
    Extras,
    Ppv,
    Site,
    Resolution,
    EpisodeCode,
    Trash,
    Date,
    Complete,
    Bitrate,
    Edition,
    Upscaled,
    AudioDescription,
    Convert,
    Hardcoded,
    Proper,
    Repack,
    Retail,
    Remastered,
    Documentary,
    Unrated,
    Uncensored,
    Commentary,
    Region,
    Quality,
    BitDepth,
    Hdr,
    Codec,
    Channels,
    Audio,
    Group,
    Volumes,
    Languages,
    Seasons,
    Episodes,
    Country,
    Subbed,
    Dubbed,
    ThreeD,
    Size,
    Network,
    Extension,
}

pub(crate) const NF: usize = F::Extension as usize + 1;

#[derive(Clone, Debug)]
pub(crate) struct Matched {
    pub raw: String,
    pub idx: usize,
}

pub(crate) struct Ctx {
    pub title: String,
    pub vals: [Option<Val>; NF],
    pub matched: [Option<Matched>; NF],
}

impl Ctx {
    pub fn new(title: String) -> Self {
        Ctx {
            title,
            vals: std::array::from_fn(|_| None),
            matched: std::array::from_fn(|_| None),
        }
    }

    pub fn has(&self, f: F) -> bool {
        self.vals[f as usize].is_some()
    }

    pub fn get(&self, f: F) -> Option<&Val> {
        self.vals[f as usize].as_ref()
    }

    pub fn set(&mut self, f: F, v: Val) {
        self.vals[f as usize] = Some(v);
    }

    pub fn unset(&mut self, f: F) {
        self.vals[f as usize] = None;
    }

    /// PTT `matched[name] = matched.get(name, ...)`: the first match wins.
    pub fn remember(&mut self, f: F, raw: &str, idx: usize) {
        let slot = &mut self.matched[f as usize];
        if slot.is_none() {
            *slot = Some(Matched {
                raw: raw.to_string(),
                idx,
            });
        }
    }

    pub fn match_idx(&self, f: F) -> Option<usize> {
        self.matched[f as usize].as_ref().map(|m| m.idx)
    }

    fn all_other_matches_after(&self, f: F, start: usize) -> bool {
        let mut any = false;
        for (i, m) in self.matched.iter().enumerate() {
            let Some(m) = m else { continue };
            if i == f as usize {
                continue;
            }
            any = true;
            if start >= m.idx {
                return false;
            }
        }
        any
    }

    fn push_uniq(&mut self, f: F, v: &str) {
        if let Some(Val::Strs(list)) = &mut self.vals[f as usize]
            && !list.iter().any(|s| s == v)
        {
            list.push(v.to_string());
        }
    }
}

/// What a handler reports back to the main loop.
pub(crate) struct Hit {
    pub raw_len: usize,
    pub idx: usize,
    pub remove: bool,
    pub skip_from_title: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum Pos {
    Start(usize),
    End(usize),
    /// The char right before the start of a group.
    Before(usize),
}

/// A replacement for a look-around: a regex tested against the text before or after a position.
pub(crate) struct Look {
    behind: bool,
    neg: bool,
    pos: Pos,
    pat: &'static str,
    re: OnceLock<Re>,
}

fn look(behind: bool, neg: bool, pat: &'static str) -> Look {
    Look {
        behind,
        neg,
        pos: if behind { Pos::Start(0) } else { Pos::End(0) },
        pat,
        re: OnceLock::new(),
    }
}

/// `(?<!pat)`, to be placed with [`Look::at`] and [`Rule::look`].
pub(crate) fn nb(pat: &'static str) -> Look {
    look(true, true, pat)
}
/// `(?!pat)`
pub(crate) fn na(pat: &'static str) -> Look {
    look(false, true, pat)
}

impl Look {
    pub fn at(mut self, pos: Pos) -> Self {
        self.pos = pos;
        self
    }

    fn regex(&self, ci: bool) -> &Re {
        self.re.get_or_init(|| {
            let wrapped = if self.behind {
                format!("(?:{})$", self.pat)
            } else {
                format!("^(?:{})", self.pat)
            };
            Re::new(&wrapped, ci)
        })
    }

    fn holds(&self, ci: bool, text: &str, caps: &Groups) -> bool {
        let Some(at) = self.offset(text, caps) else {
            return true;
        };
        let re = self.regex(ci);
        let hit = if self.behind {
            re.is_match(&text[..at])
        } else {
            re.is_match(&text[at..])
        };
        hit != self.neg
    }

    fn offset(&self, text: &str, caps: &Groups) -> Option<usize> {
        match self.pos {
            Pos::Start(g) => caps.span(g).map(|s| s.0),
            Pos::End(g) => caps.span(g).map(|s| s.1),
            Pos::Before(g) => caps.span(g).map(|s| prev_char(text, s.0)),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Opts {
    again: bool,
    skip_from_title: bool,
    skip_if_first: bool,
    remove: bool,
}

pub(crate) type Check = fn(&str, &Groups) -> bool;

/// A match plus the span PTT would report (a trailing look-ahead group is not part of it).
pub(crate) struct Found {
    pub groups: Groups,
    pub start: usize,
    pub end: usize,
    /// Number of groups the pattern has in PTT's sense (without the look-ahead group).
    ngroups: usize,
}

impl Found {
    pub fn raw<'t>(&self, text: &'t str) -> &'t str {
        &text[self.start..self.end]
    }

    pub fn group<'t>(&self, text: &'t str, i: usize) -> Option<&'t str> {
        self.groups.text(text, i)
    }

    /// PTT passes group 1 to the transformer when the pattern has groups and it is non-empty.
    fn input<'t>(&self, text: &'t str) -> &'t str {
        if self.ngroups == 0 {
            return self.raw(text);
        }
        match self.group(text, 1) {
            Some(g) if !g.is_empty() => g,
            _ => self.raw(text),
        }
    }
}

/// One alternative of a rule: a pattern plus the guards that stand in for its look-arounds.
struct Branch {
    pat: &'static str,
    /// Trailing `(?=tail)` realised as a consumed last group, so backtracking stays exact.
    tail: Option<&'static str>,
    looks: Vec<Look>,
    checks: Vec<Check>,
    /// Small engine, used while the branch is searched rarely; dropped once `fast` exists.
    lean: Mutex<Option<Arc<Re>>>,
    /// Engine with a lazy DFA, built when the branch is searched on a fair share of titles.
    fast: OnceLock<Re>,
    runs: AtomicU32,
}

/// Titles parsed so far, to tell how often a branch runs.
static PARSES: AtomicU64 = AtomicU64::new(0);

/// A branch searched at least this often, and on at least one title in `HOT_SHARE`, gets the
/// fast engine: it saves about 2 us per search and costs about 150 us and 30 KB once.
const HOT_RUNS: u32 = 48;
const HOT_SHARE: u64 = 4;

enum Engine<'a> {
    Fast(&'a Re),
    Lean(Arc<Re>),
}

impl Engine<'_> {
    fn re(&self) -> &Re {
        match self {
            Engine::Fast(r) => r,
            Engine::Lean(r) => r,
        }
    }
}

impl Branch {
    fn new(pat: &'static str) -> Self {
        Branch {
            pat,
            tail: None,
            looks: Vec::new(),
            checks: Vec::new(),
            lean: Mutex::new(None),
            fast: OnceLock::new(),
            runs: AtomicU32::new(0),
        }
    }

    fn source(&self) -> String {
        match self.tail {
            Some(t) => format!("(?:{})({t})", self.pat),
            None => self.pat.to_string(),
        }
    }

    fn engine(&self, ci: bool) -> Engine<'_> {
        if let Some(fast) = self.fast.get() {
            return Engine::Fast(fast);
        }
        let runs = self.runs.fetch_add(1, Ordering::Relaxed).saturating_add(1);
        if runs >= HOT_RUNS && u64::from(runs) * HOT_SHARE >= PARSES.load(Ordering::Relaxed) {
            let fast = self.fast.get_or_init(|| Re::fast(&self.source(), ci));
            *self.lean.lock().unwrap_or_else(PoisonError::into_inner) = None;
            return Engine::Fast(fast);
        }
        let mut slot = self.lean.lock().unwrap_or_else(PoisonError::into_inner);
        let re = slot.get_or_insert_with(|| Arc::new(Re::new(&self.source(), ci)));
        Engine::Lean(Arc::clone(re))
    }

    pub fn find(&self, ci: bool, text: &str, mut from: usize) -> Option<Found> {
        let engine = self.engine(ci);
        let re = engine.re();
        while from <= text.len() {
            let groups = re.captures_at(text, from)?;
            let (start, whole_end) = groups.span(0)?;
            if self.accepts(ci, text, &groups) {
                let last = groups.len() - 1;
                let (end, ngroups) = match self.tail {
                    Some(_) => (groups.span(last).map_or(whole_end, |g| g.0), last - 1),
                    None => (whole_end, last),
                };
                return Some(Found {
                    groups,
                    start,
                    end,
                    ngroups,
                });
            }
            from = next_char(text, start);
        }
        None
    }

    fn accepts(&self, ci: bool, text: &str, caps: &Groups) -> bool {
        self.checks.iter().all(|c| c(text, caps))
            && self.looks.iter().all(|l| l.holds(ci, text, caps))
    }

    #[cfg(test)]
    fn warm(&self, ci: bool) {
        self.engine(ci);
        self.looks.iter().for_each(|l| {
            l.regex(ci);
        });
    }
}

pub(crate) struct Rule {
    pub field: F,
    ci: bool,
    tf: Tf,
    opts: Opts,
    branches: Vec<Branch>,
}

pub(crate) fn rule(field: F, pat: &'static str, tf: Tf) -> Rule {
    Rule {
        field,
        ci: false,
        tf,
        opts: Opts::default(),
        branches: vec![Branch::new(pat)],
    }
}

impl Rule {
    /// Case-insensitive (`regex.IGNORECASE`).
    pub fn ci(mut self) -> Self {
        self.ci = true;
        self
    }
    /// `remove: True`
    pub fn rm(mut self) -> Self {
        self.opts.remove = true;
        self
    }
    /// `skipFromTitle: True`
    pub fn st(mut self) -> Self {
        self.opts.skip_from_title = true;
        self
    }
    /// `skipIfFirst: True`
    pub fn sif(mut self) -> Self {
        self.opts.skip_if_first = true;
        self
    }
    /// `skipIfAlreadyFound: False`
    pub fn again(mut self) -> Self {
        self.opts.again = true;
        self
    }
    /// Start another alternative (for patterns whose `|` branches carry different look-arounds).
    pub fn alt(mut self, pat: &'static str) -> Self {
        self.branches.push(Branch::new(pat));
        self
    }
    fn last(&mut self) -> &mut Branch {
        self.branches.last_mut().expect("rule has a branch")
    }
    /// `(?<=pat)` at the match start of the current alternative.
    pub fn pb(mut self, pat: &'static str) -> Self {
        self.last().looks.push(look(true, false, pat));
        self
    }
    /// `(?<!pat)` at the match start.
    pub fn nb(mut self, pat: &'static str) -> Self {
        self.last().looks.push(look(true, true, pat));
        self
    }
    /// `(?!pat)` at the match end.
    pub fn na(mut self, pat: &'static str) -> Self {
        self.last().looks.push(look(false, true, pat));
        self
    }
    /// A look-around at an explicit position.
    pub fn look(mut self, l: Look) -> Self {
        self.last().looks.push(l);
        self
    }
    pub fn check(mut self, c: Check) -> Self {
        self.last().checks.push(c);
        self
    }
    /// Trailing `(?=pat)`: matched as a last group and cut off the reported span.
    pub fn la(mut self, pat: &'static str) -> Self {
        self.last().tail = Some(pat);
        self
    }

    /// Literals of which every match contains one (see `needle.rs`), `None` if unknown.
    pub fn needles(&self) -> Option<Vec<String>> {
        let pats: Vec<&str> = self.branches.iter().map(|b| b.pat).collect();
        rule_needles(&pats)
    }

    /// Compile every pattern now (panics on an invalid one).
    #[cfg(test)]
    pub fn warm(&self) {
        self.branches.iter().for_each(|b| b.warm(self.ci));
    }

    /// Leftmost accepted match over all alternatives (earlier alternative wins ties).
    pub fn find(&self, text: &str) -> Option<Found> {
        self.find_from(text, 0)
    }

    pub fn find_from(&self, text: &str, from: usize) -> Option<Found> {
        let mut best: Option<Found> = None;
        for b in &self.branches {
            if let Some(c) = b.find(self.ci, text, from)
                && best.as_ref().is_none_or(|cur| c.start < cur.start)
            {
                best = Some(c);
            }
        }
        best
    }

    pub fn settled(&self, ctx: &Ctx) -> bool {
        !self.opts.again && ctx.has(self.field)
    }

    pub fn run(&self, ctx: &mut Ctx) -> Option<Hit> {
        let f = self.field;
        if self.settled(ctx) {
            return None;
        }
        let (start, raw, input) = {
            let found = self.find(&ctx.title)?;
            let raw = found.raw(&ctx.title).to_string();
            (found.start, raw, found.input(&ctx.title).to_string())
        };
        let label = self.tf.uniq_label(&raw);
        let existed = label.is_some() && ctx.has(f);
        if let (Some(l), true) = (label, existed) {
            ctx.push_uniq(f, l);
        }
        let out = match label {
            Some(l) => Val::Strs(vec![l.to_string()]),
            None => self.tf.apply(&input)?,
        };
        let before_title = in_leading_bracket(&ctx.title, &raw);
        if self.opts.skip_if_first && ctx.all_other_matches_after(f, start) {
            return None;
        }
        ctx.remember(f, &raw, start);
        if !existed {
            ctx.set(f, out);
        }
        Some(Hit {
            raw_len: raw.len(),
            idx: start,
            remove: self.opts.remove,
            skip_from_title: before_title || self.opts.skip_from_title,
        })
    }
}

/// PTT: the match lies inside a leading `[group]` block, so it must not end the title.
pub(crate) fn in_leading_bracket(title: &str, raw: &str) -> bool {
    let Some(rest) = title.strip_prefix('[') else {
        return false;
    };
    match rest.find(['[', ']']) {
        Some(end) if end > 0 && rest.as_bytes()[end] == b']' => rest[..end].contains(raw),
        _ => false,
    }
}

pub(crate) fn next_char(text: &str, i: usize) -> usize {
    match text.get(i..).and_then(|t| t.chars().next()) {
        Some(c) => i + c.len_utf8(),
        None => text.len() + 1,
    }
}

fn prev_char(text: &str, i: usize) -> usize {
    text[..i]
        .chars()
        .next_back()
        .map_or(0, |c| i - c.len_utf8())
}

pub(crate) type HandlerFn = fn(&mut Ctx) -> Option<Hit>;

pub(crate) enum Handler {
    Re(Box<Rule>),
    /// A function handler that always runs.
    Func(HandlerFn),
    /// A function handler that can only act when the title contains one of these (lowercase)
    /// literals.
    Gated(HandlerFn, &'static [&'static str]),
}

impl Handler {
    pub fn run(&self, ctx: &mut Ctx) -> Option<Hit> {
        match self {
            Handler::Re(r) => r.run(ctx),
            Handler::Func(f) | Handler::Gated(f, _) => f(ctx),
        }
    }

    /// True when the handler would return at once because its field is already found.
    pub fn settled(&self, ctx: &Ctx) -> bool {
        matches!(self, Handler::Re(r) if r.settled(ctx))
    }

    /// Literals the title must contain for this handler to act, `None` if it always runs.
    pub fn needles(&self) -> Option<Vec<String>> {
        match self {
            Handler::Re(r) => r.needles(),
            Handler::Func(_) => None,
            Handler::Gated(_, set) => Some(set.iter().map(|s| (*s).to_string()).collect()),
        }
    }
}

impl From<Rule> for Handler {
    fn from(r: Rule) -> Self {
        Handler::Re(Box::new(r))
    }
}

/// The ordered handlers plus the pre-filter built from them.
pub(crate) struct Table {
    pub handlers: Vec<Handler>,
    gate: Gate,
}

impl Table {
    pub fn new(handlers: Vec<Handler>) -> Self {
        let gate = Gate::new(&handlers);
        Table { handlers, gate }
    }
}

/// The main loop of PTT's `Parser.parse`: raw per-field results plus where the title ends.
///
/// The gate view is rebuilt whenever the title changes, so skipping a rule is always exact.
pub(crate) fn run_all(table: &Table, title: String) -> (Ctx, usize) {
    PARSES.fetch_add(1, Ordering::Relaxed);
    let mut ctx = Ctx::new(title);
    let mut end = ctx.title.len();
    let mut view = View::new(&ctx.title);
    let mut stale = false;
    for (i, h) in table.handlers.iter().enumerate() {
        if h.settled(&ctx) {
            continue;
        }
        if stale {
            view.reset(&ctx.title);
            stale = false;
        }
        if !table.gate.allows(&view, i) {
            continue;
        }
        let Some(hit) = h.run(&mut ctx) else { continue };
        let before = ctx.title.len();
        apply_hit(&mut ctx, &hit, &mut end);
        stale = ctx.title.len() != before;
    }
    (ctx, end)
}

/// Reference loop without the gate, to test that the gate never changes a result.
#[cfg(test)]
pub(crate) fn run_ungated(table: &Table, title: String) -> (Ctx, usize) {
    let mut ctx = Ctx::new(title);
    let mut end = ctx.title.len();
    for h in &table.handlers {
        if let Some(hit) = h.run(&mut ctx) {
            apply_hit(&mut ctx, &hit, &mut end);
        }
    }
    (ctx, end)
}

fn apply_hit(ctx: &mut Ctx, hit: &Hit, end: &mut usize) {
    let title = &ctx.title;
    let stop = hit.idx + hit.raw_len;
    if stop > title.len() || !title.is_char_boundary(hit.idx) || !title.is_char_boundary(stop) {
        return;
    }
    if hit.remove {
        ctx.title.replace_range(hit.idx..hit.idx + hit.raw_len, "");
    }
    if !hit.skip_from_title && hit.idx < *end && ctx.title[..hit.idx].chars().nth(1).is_some() {
        *end = hit.idx;
    }
    if hit.remove && hit.skip_from_title && hit.idx < *end {
        *end = end.saturating_sub(hit.raw_len);
    }
}
