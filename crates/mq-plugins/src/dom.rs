//! HTML for a plugin's cheerio (plans/plugins-and-marketplace.md 6.2): a tree
//! of our own under html5ever that counts nodes, depth and attributes and
//! stops at its caps. `scraper` parses to the end before anything can be
//! counted; here the input goes in by chunks, a node past a cap is never
//! kept, and feeding stops at the first chunk that met one, so what a page
//! can cost is the caps plus one chunk.
//!
//! Some of html5ever's work grows faster than its input, before the sink sees
//! anything: its tokenizer checks each attribute name against every earlier
//! one of the same tag, so one 4 MiB tag of attributes costs minutes
//! (bench/plugins/d-spike.jsonl). A byte scan before the tokenizer finds the
//! first tag over the attribute or tag-length cap, and the parser is never
//! given it. The call's budget (CPU, wall, cancel) is read between chunks too,
//! for whatever the scan does not foresee.

use html5ever::interface::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::{Attribute, QualName, parse_document};
use std::borrow::Cow;
use std::cell::{Cell, Ref, RefCell};

/// What one document may hold.
#[derive(Debug, Clone, Copy)]
pub struct Caps {
    pub bytes: usize,
    pub nodes: usize,
    pub depth: usize,
    pub attrs: usize,
    /// One tag's bytes, `<` to `>`.
    pub tag: usize,
}

impl Default for Caps {
    fn default() -> Self {
        Self {
            bytes: 4 * 1024 * 1024,
            nodes: 150_000,
            depth: 256,
            attrs: 64,
            tag: 256 * 1024,
        }
    }
}

/// The cap a document met first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Bytes,
    Nodes,
    Depth,
    Attrs,
    Tag,
    /// The call's budget ran out, or the call was cancelled.
    Budget,
}

#[derive(Debug)]
pub struct Node {
    pub parent: Option<usize>,
    pub depth: u16,
    /// None for the document, text and comments.
    pub name: Option<QualName>,
    pub attrs: Vec<Attribute>,
    pub text: String,
    pub children: Vec<usize>,
}

/// A parsed document: its nodes (0 is the document), the cap it met, if any,
/// and how many bytes of the input the parser was given.
#[derive(Debug)]
pub struct Doc {
    pub nodes: Vec<Node>,
    pub stopped: Option<Stop>,
    pub fed: usize,
}

/// Input goes to the parser in pieces of this size, and the caps and the
/// budget are read after each.
const CHUNK: usize = 4 * 1024;

/// The handle every node past a cap gets: an element nothing is ever added to.
const DROPPED: usize = 1;

/// Parses `html` within `caps`; `spent` says when the call's budget is gone.
pub fn parse(html: &str, caps: Caps, spent: impl Fn() -> bool) -> Doc {
    let sink = Sink::new(caps);
    let stopped = sink.stopped.clone();
    let mut parser = parse_document(sink, Default::default());
    // Past the byte cap the rest is never read; what came before is kept.
    let cut = html.len() > caps.bytes;
    let html = &html[..floor_char(html, html.len().min(caps.bytes))];
    // The tag the scan found starts at a `<`, so the cut is a char boundary.
    let over = first_over(html.as_bytes(), &caps);
    let html = &html[..over.map_or(html.len(), |(at, _)| at)];
    let mut at = 0;
    while at < html.len() && stopped.get().is_none() {
        if spent() {
            stopped.set(Some(Stop::Budget));
            break;
        }
        let end = floor_char(html, (at + CHUNK).min(html.len())).max(ceil_char(html, at + 1));
        parser.process(StrTendril::from_slice(&html[at..end]));
        at = end;
    }
    // `finish` runs on a stopped parse too: it only closes what is open (the
    // depth cap plus one chunk of elements at most) and hands over the tree.
    let mut doc = parser.finish();
    doc.stopped = stopped
        .get()
        .or(over.map(|(_, why)| why))
        .or(cut.then_some(Stop::Bytes));
    doc.fed = at;
    doc
}

/// Elements whose text html5ever reads raw up to their end tag, outside SVG
/// and MathML (scripting on, as `parse_document` has it).
const RAW: [&[u8]; 10] = [
    b"script",
    b"style",
    b"textarea",
    b"title",
    b"xmp",
    b"iframe",
    b"noembed",
    b"noframes",
    b"noscript",
    b"plaintext",
];

/// Where the first tag over the attribute or the tag-length cap starts.
///
/// It follows the tokenizer's tag states (WHATWG 13.2.5.8 to 13.2.5.40), its
/// comments, and the raw text elements above, which it skips as text. Where it
/// cannot know what the tree builder will do it reads markup as tags, which
/// can only count more: inside `<svg>` or `<math>` nothing is skipped (the
/// tree builder reads `<style>` there as markup), and a script's escaped
/// `<!--` sections end at the first `</script`.
fn first_over(html: &[u8], caps: &Caps) -> Option<(usize, Stop)> {
    let n = html.len();
    let at_ci = |at: usize, word: &[u8]| {
        html.get(at..at + word.len())
            .is_some_and(|b| b.eq_ignore_ascii_case(word))
    };
    let find = |from: usize, word: &[u8]| {
        (from..n.saturating_sub(word.len() - 1)).find(|&at| &html[at..at + word.len()] == word)
    };
    let space = |b: u8| matches!(b, b'\t' | b'\n' | b'\x0c' | b'\r' | b' ');
    let (mut i, mut svg, mut math) = (0, 0usize, 0usize);
    while let Some(lt) = html[i..].iter().position(|&b| b == b'<') {
        let start = i + lt;
        if at_ci(start, b"<!--") {
            // `<!-->` and `<!--->` end at once; others at `-->` or `--!>`.
            let body = start + 4;
            i = if at_ci(body, b">") {
                body + 1
            } else if at_ci(body, b"->") {
                body + 2
            } else {
                // One pass to whichever comes first, so the scan stays linear.
                (body..n)
                    .find_map(|at| {
                        (at_ci(at, b"-->").then_some(at + 3))
                            .or(at_ci(at, b"--!>").then_some(at + 4))
                    })
                    .unwrap_or(n)
            };
            continue;
        }
        let end_tag = html.get(start + 1) == Some(&b'/');
        let name_at = start + 1 + end_tag as usize;
        if !html.get(name_at).is_some_and(u8::is_ascii_alphabetic) {
            // `<!`, `<?` and `</` before anything else read as a bogus
            // comment to the next `>`; `<` before anything else is text.
            i = match html.get(start + 1) {
                Some(b'!' | b'?' | b'/') => find(start + 1, b">").map_or(n, |at| at + 1),
                _ => start + 1,
            };
            continue;
        }
        let mut j = name_at;
        while j < n && !space(html[j]) && html[j] != b'/' && html[j] != b'>' {
            j += 1;
        }
        let name = &html[name_at..j];
        #[derive(Clone, Copy)]
        enum S {
            Before,
            Name,
            After,
            BeforeValue,
            Double,
            Single,
            Unquoted,
            AfterQuoted,
            SelfClosing,
        }
        let (mut state, mut attrs, mut closed, mut end) = (S::Before, 0, false, None);
        while j < n {
            if j - start > caps.tag {
                return Some((start, Stop::Tag));
            }
            let b = html[j];
            match state {
                S::Before | S::After if space(b) => {}
                S::Before | S::After | S::Name if b == b'/' => state = S::SelfClosing,
                S::Before | S::After | S::Name | S::BeforeValue | S::Unquoted if b == b'>' => {
                    end = Some(j);
                    break;
                }
                S::After | S::Name if b == b'=' => state = S::BeforeValue,
                S::Before | S::After => {
                    attrs += 1;
                    if attrs > caps.attrs {
                        return Some((start, Stop::Attrs));
                    }
                    state = S::Name;
                }
                S::Name if space(b) => state = S::After,
                S::Name => {}
                S::BeforeValue if space(b) => {}
                S::BeforeValue if b == b'"' => state = S::Double,
                S::BeforeValue if b == b'\'' => state = S::Single,
                S::BeforeValue => state = S::Unquoted,
                S::Double if b == b'"' => state = S::AfterQuoted,
                S::Single if b == b'\'' => state = S::AfterQuoted,
                S::Double | S::Single => {}
                S::Unquoted if space(b) => state = S::Before,
                S::Unquoted => {}
                S::AfterQuoted | S::SelfClosing if b == b'>' => {
                    closed = matches!(state, S::SelfClosing);
                    end = Some(j);
                    break;
                }
                // Anything else is read again as if before an attribute name.
                S::AfterQuoted | S::SelfClosing => {
                    state = S::Before;
                    continue;
                }
            }
            j += 1;
        }
        // A tag still open at the end of the input never reaches the tree.
        let end = end?;
        i = end + 1;
        let is = |word: &[u8]| name.eq_ignore_ascii_case(word);
        if end_tag {
            if is(b"svg") {
                svg = svg.saturating_sub(1);
            } else if is(b"math") {
                math = math.saturating_sub(1);
            }
        } else if is(b"svg") {
            svg += !closed as usize;
        } else if is(b"math") {
            math += !closed as usize;
        } else if svg == 0 && math == 0 && RAW.iter().any(|raw| is(raw)) {
            if is(b"plaintext") {
                return None;
            }
            // The text runs to `</name` and a space, `/` or `>`.
            i = (i..n).find(|&at| {
                at_ci(at, b"</")
                    && at_ci(at + 2, name)
                    && html
                        .get(at + 2 + name.len())
                        .is_none_or(|&b| space(b) || b == b'/' || b == b'>')
            })?;
        }
    }
    None
}

fn floor_char(s: &str, mut at: usize) -> usize {
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    at
}

fn ceil_char(s: &str, mut at: usize) -> usize {
    while at < s.len() && !s.is_char_boundary(at) {
        at += 1;
    }
    at.min(s.len())
}

struct Sink {
    caps: Caps,
    nodes: RefCell<Vec<Node>>,
    stopped: std::rc::Rc<Cell<Option<Stop>>>,
}

impl Sink {
    fn new(caps: Caps) -> Self {
        let node = |name: Option<QualName>| Node {
            parent: None,
            depth: 0,
            name,
            attrs: Vec::new(),
            text: String::new(),
            children: Vec::new(),
        };
        let dropped = QualName::new(None, html5ever::ns!(html), html5ever::local_name!("div"));
        Self {
            caps,
            nodes: RefCell::new(vec![node(None), node(Some(dropped))]),
            stopped: Default::default(),
        }
    }

    fn stop(&self, why: Stop) {
        if self.stopped.get().is_none() {
            self.stopped.set(Some(why));
        }
    }

    /// A new node, or `DROPPED` once a cap is met.
    fn add(&self, name: Option<QualName>, attrs: Vec<Attribute>, text: String) -> usize {
        if self.stopped.get().is_some() {
            return DROPPED;
        }
        let mut nodes = self.nodes.borrow_mut();
        if nodes.len() >= self.caps.nodes {
            self.stop(Stop::Nodes);
            return DROPPED;
        }
        if attrs.len() > self.caps.attrs {
            self.stop(Stop::Attrs);
            return DROPPED;
        }
        nodes.push(Node {
            parent: None,
            depth: 0,
            name,
            attrs,
            text,
            children: Vec::new(),
        });
        nodes.len() - 1
    }

    fn attach(&self, parent: usize, child: usize) {
        if parent == DROPPED || child == DROPPED {
            return;
        }
        let mut nodes = self.nodes.borrow_mut();
        let depth = nodes[parent].depth as usize + 1;
        if depth > self.caps.depth {
            self.stop(Stop::Depth);
            return;
        }
        nodes[child].parent = Some(parent);
        nodes[child].depth = depth as u16;
        nodes[parent].children.push(child);
        // A moved subtree (the adoption agency, foster parenting) takes its new
        // depth all the way down, so the cap holds for every node. A walk, not
        // recursion: the tree's depth is what is in question.
        let mut below = nodes[child].children.clone();
        while let Some(node) = below.pop() {
            let depth = nodes[nodes[node].parent.expect("a child's parent")].depth as usize + 1;
            if depth > self.caps.depth {
                self.stop(Stop::Depth);
            }
            nodes[node].depth = depth.min(u16::MAX as usize) as u16;
            below.extend_from_slice(&nodes[node].children);
        }
    }

    fn text(&self, parent: usize, text: &str) {
        if parent == DROPPED || self.stopped.get().is_some() {
            return;
        }
        // Text next to text joins it, as the tree builder expects.
        let last = self.nodes.borrow()[parent].children.last().copied();
        if let Some(last) = last
            && self.nodes.borrow()[last].name.is_none()
        {
            self.nodes.borrow_mut()[last].text.push_str(text);
            return;
        }
        let node = self.add(None, Vec::new(), text.to_owned());
        self.attach(parent, node);
    }
}

impl TreeSink for Sink {
    type Handle = usize;
    type Output = Doc;
    type ElemName<'a> = Ref<'a, QualName>;

    fn finish(self) -> Doc {
        Doc {
            nodes: self.nodes.into_inner(),
            stopped: self.stopped.get(),
            fed: 0,
        }
    }

    fn parse_error(&self, _msg: Cow<'static, str>) {}

    fn get_document(&self) -> usize {
        0
    }

    fn elem_name<'a>(&'a self, target: &'a usize) -> Ref<'a, QualName> {
        Ref::map(self.nodes.borrow(), |nodes| {
            nodes[*target].name.as_ref().unwrap_or_else(|| {
                nodes[DROPPED]
                    .name
                    .as_ref()
                    .expect("the dropped element's name")
            })
        })
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, _flags: ElementFlags) -> usize {
        self.add(Some(name), attrs, String::new())
    }

    fn create_comment(&self, _text: StrTendril) -> usize {
        self.add(None, Vec::new(), String::new())
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> usize {
        self.add(None, Vec::new(), String::new())
    }

    fn append(&self, parent: &usize, child: NodeOrText<usize>) {
        match child {
            NodeOrText::AppendNode(node) => self.attach(*parent, node),
            NodeOrText::AppendText(text) => self.text(*parent, &text),
        }
    }

    fn append_based_on_parent_node(&self, element: &usize, prev: &usize, child: NodeOrText<usize>) {
        let parent = self.nodes.borrow()[*element].parent;
        match parent {
            Some(_) => self.append_before_sibling(element, child),
            None => self.append(prev, child),
        }
    }

    fn append_doctype_to_document(
        &self,
        _name: StrTendril,
        _public: StrTendril,
        _system: StrTendril,
    ) {
    }

    fn get_template_contents(&self, target: &usize) -> usize {
        *target
    }

    fn same_node(&self, x: &usize, y: &usize) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, _mode: QuirksMode) {}

    fn append_before_sibling(&self, sibling: &usize, child: NodeOrText<usize>) {
        // ponytail: misnested markup lands at its parent's end, not before the
        // sibling; order among siblings is not something a scraper reads here.
        let parent = self.nodes.borrow()[*sibling].parent;
        if let Some(parent) = parent {
            self.append(&parent, child);
        }
    }

    fn add_attrs_if_missing(&self, target: &usize, attrs: Vec<Attribute>) {
        if *target == DROPPED {
            return;
        }
        let mut nodes = self.nodes.borrow_mut();
        let node = &mut nodes[*target];
        for attr in attrs {
            if node.attrs.len() >= self.caps.attrs {
                break;
            }
            if !node.attrs.iter().any(|a| a.name == attr.name) {
                node.attrs.push(attr);
            }
        }
    }

    fn remove_from_parent(&self, target: &usize) {
        if *target == DROPPED {
            return;
        }
        let mut nodes = self.nodes.borrow_mut();
        if let Some(parent) = nodes[*target].parent.take() {
            nodes[parent].children.retain(|c| c != target);
        }
    }

    fn reparent_children(&self, node: &usize, new_parent: &usize) {
        if *node == DROPPED || *new_parent == DROPPED {
            return;
        }
        let children = std::mem::take(&mut self.nodes.borrow_mut()[*node].children);
        for child in children {
            self.attach(*new_parent, child);
        }
    }
}
