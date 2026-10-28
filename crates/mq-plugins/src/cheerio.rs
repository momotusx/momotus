//! The natives under the prelude's cheerio (plans/plugins-and-marketplace.md
//! 6.2 and 6.4): a page parsed within the caps of `dom`, then handed to
//! `scraper` for its selectors and serialization. Documents and their nodes
//! reach JS as numbers; 0 is a document's root. Selector work is counted in
//! node visits against the call's budget.

use crate::dom::{self, Caps, Kind};
use crate::engine::spent;
use crate::natives::charge;
use ego_tree::NodeId;
use rquickjs::prelude::Func;
use rquickjs::{Ctx, Exception, Object, Result};
use scraper::node::{Comment, Element, Text};
use scraper::{ElementRef, Html, Node, Selector};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

/// Node visits per call (6.2).
pub const MAX_VISITS: u32 = 100_000;

/// What a parsed node is charged against the call's host bytes, beyond the
/// page's own bytes: scraper's node with its links, our id maps, and slack
/// (bench/plugins/d-spike.jsonl measures our own tree's node).
pub const NODE_BYTES: usize = 256;

struct Page {
    html: Html,
    /// Node ids in document order; a node's number is its index here.
    ids: Vec<NodeId>,
    numbers: HashMap<NodeId, u32>,
}

thread_local! {
    static PAGES: RefCell<Vec<Page>> = const { RefCell::new(Vec::new()) };
    static VISITS: Cell<u32> = const { Cell::new(0) };
}

pub fn install(n: &Object<'_>) -> Result<()> {
    n.set("load", Func::from(load))?;
    n.set("select", Func::from(select))?;
    n.set("matching", Func::from(matching))?;
    n.set("children", Func::from(children))?;
    n.set("parent", Func::from(parent))?;
    n.set("closest", Func::from(closest))?;
    n.set("sibling", Func::from(sibling))?;
    n.set("text", Func::from(text))?;
    n.set("html", Func::from(html))?;
    n.set("attr", Func::from(attr))?;
    n.set("attrs", Func::from(attrs))?;
    Ok(())
}

fn type_error<T>(ctx: &Ctx<'_>, words: &str) -> Result<T> {
    Err(Exception::throw_type(ctx, words))
}

/// Counts one visit, or throws when the call's visits are spent.
fn visit(ctx: &Ctx<'_>) -> Result<()> {
    let visits = VISITS.get() + 1;
    if visits > MAX_VISITS {
        return Err(Exception::throw_range(ctx, "too much selector work"));
    }
    VISITS.set(visits);
    Ok(())
}

/// Parses `source` within the caps and the call's budget, charges it to the
/// call's host bytes, and keeps it for the call: its number.
fn load(ctx: Ctx<'_>, source: String) -> Result<u32> {
    let parsed = dom::parse(&source, Caps::default(), spent);
    if parsed.stopped == Some(dom::Stop::Budget) {
        return type_error(&ctx, "out of time while parsing");
    }
    let cost = source.len().min(Caps::default().bytes) + parsed.nodes.len() * NODE_BYTES;
    if charge(cost).is_err() {
        return type_error(&ctx, "the call's 16 MiB are spent");
    }
    let page = convert(parsed);
    PAGES.with_borrow_mut(|pages| {
        pages.push(page);
        Ok(pages.len() as u32 - 1)
    })
}

/// Our tree as scraper's, in document order; only nodes in the tree come over.
fn convert(doc: dom::Doc) -> Page {
    let dom::Doc { nodes, .. } = doc;
    let mut nodes: Vec<Option<dom::Node>> = nodes.into_iter().map(Some).collect();
    let mut html = Html::new_document();
    let root = html.tree.root().id();
    let (mut ids, mut numbers) = (vec![root], HashMap::new());
    numbers.insert(root, 0);
    let children =
        |node: &Option<dom::Node>| node.as_ref().map_or(Vec::new(), |n| n.children.clone());
    // (our index, scraper's parent), last first, so the pops go in order.
    let mut below: Vec<(usize, NodeId)> = children(&nodes[0])
        .into_iter()
        .rev()
        .map(|c| (c, root))
        .collect();
    while let Some((index, parent)) = below.pop() {
        let Some(node) = nodes[index].take() else {
            continue;
        };
        let value = match node.kind {
            Kind::Element => Node::Element(Element::new(
                node.name.expect("an element's name"),
                node.attrs,
            )),
            Kind::Text => Node::Text(Text {
                text: node.text.into(),
            }),
            Kind::Comment | Kind::Document => Node::Comment(Comment {
                comment: Default::default(),
            }),
        };
        let id = html
            .tree
            .get_mut(parent)
            .expect("a parent already made")
            .append(value)
            .id();
        numbers.insert(id, ids.len() as u32);
        ids.push(id);
        below.extend(node.children.iter().rev().map(|&c| (c, id)));
    }
    Page { html, ids, numbers }
}

/// Runs `f` on page `doc`.
fn with_page<T>(ctx: &Ctx<'_>, doc: u32, f: impl FnOnce(&Page) -> Result<T>) -> Result<T> {
    PAGES.with_borrow(|pages| match pages.get(doc as usize) {
        Some(page) => f(page),
        None => type_error(ctx, "no such document"),
    })
}

fn node_id(ctx: &Ctx<'_>, page: &Page, number: u32) -> Result<NodeId> {
    match page.ids.get(number as usize) {
        Some(&id) => Ok(id),
        None => type_error(ctx, "no such node"),
    }
}

fn element<'a>(page: &'a Page, id: NodeId) -> Option<ElementRef<'a>> {
    ElementRef::wrap(page.html.tree.get(id)?)
}

fn parse_selector(ctx: &Ctx<'_>, selector: &str) -> Result<Selector> {
    match Selector::parse(selector) {
        Ok(selector) => Ok(selector),
        Err(_) => Err(Exception::throw_syntax(
            ctx,
            &format!("invalid selector: {selector}"),
        )),
    }
}

/// The elements under `scope` (the whole page when None) that match, in
/// document order, each once.
fn select(ctx: Ctx<'_>, doc: u32, selector: String, scope: Option<Vec<u32>>) -> Result<Vec<u32>> {
    let selector = parse_selector(&ctx, &selector)?;
    with_page(&ctx, doc, |page| {
        let scope = scope.unwrap_or_else(|| vec![0]);
        let mut out = Vec::new();
        for number in scope {
            let root = node_id(&ctx, page, number)?;
            let Some(root) = page.html.tree.get(root) else {
                continue;
            };
            for node in root.descendants().skip(1) {
                visit(&ctx)?;
                if let Some(el) = ElementRef::wrap(node)
                    && selector.matches(&el)
                {
                    out.push(page.numbers[&node.id()]);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        Ok(out)
    })
}

/// Those of `numbers` that match.
fn matching(ctx: Ctx<'_>, doc: u32, numbers: Vec<u32>, selector: String) -> Result<Vec<u32>> {
    let selector = parse_selector(&ctx, &selector)?;
    with_page(&ctx, doc, |page| {
        let mut out = Vec::new();
        for number in numbers {
            visit(&ctx)?;
            if element(page, node_id(&ctx, page, number)?).is_some_and(|el| selector.matches(&el)) {
                out.push(number);
            }
        }
        Ok(out)
    })
}

fn children(ctx: Ctx<'_>, doc: u32, number: u32) -> Result<Vec<u32>> {
    with_page(&ctx, doc, |page| {
        let Some(node) = page.html.tree.get(node_id(&ctx, page, number)?) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for child in node.children() {
            visit(&ctx)?;
            if child.value().is_element() {
                out.push(page.numbers[&child.id()]);
            }
        }
        Ok(out)
    })
}

/// The parent element's number, or 0 when there is none.
fn parent(ctx: Ctx<'_>, doc: u32, number: u32) -> Result<u32> {
    with_page(&ctx, doc, |page| {
        let node = page.html.tree.get(node_id(&ctx, page, number)?);
        Ok(node
            .and_then(|n| n.parent())
            .filter(|p| p.value().is_element())
            .map_or(0, |p| page.numbers[&p.id()]))
    })
}

/// The element itself or its nearest ancestor that matches; 0 when none.
fn closest(ctx: Ctx<'_>, doc: u32, number: u32, selector: String) -> Result<u32> {
    let selector = parse_selector(&ctx, &selector)?;
    with_page(&ctx, doc, |page| {
        let mut node = page.html.tree.get(node_id(&ctx, page, number)?);
        while let Some(n) = node {
            visit(&ctx)?;
            if ElementRef::wrap(n).is_some_and(|el| selector.matches(&el)) {
                return Ok(page.numbers[&n.id()]);
            }
            node = n.parent();
        }
        Ok(0)
    })
}

/// The next (`step` 1) or previous (-1) element sibling; 0 when none.
fn sibling(ctx: Ctx<'_>, doc: u32, number: u32, step: i32) -> Result<u32> {
    with_page(&ctx, doc, |page| {
        let mut node = page.html.tree.get(node_id(&ctx, page, number)?);
        loop {
            node = node.and_then(|n| {
                if step > 0 {
                    n.next_sibling()
                } else {
                    n.prev_sibling()
                }
            });
            let Some(n) = node else {
                return Ok(0);
            };
            visit(&ctx)?;
            if n.value().is_element() {
                return Ok(page.numbers[&n.id()]);
            }
        }
    })
}

/// The text of every node in `numbers`, joined.
fn text(ctx: Ctx<'_>, doc: u32, numbers: Vec<u32>) -> Result<String> {
    with_page(&ctx, doc, |page| {
        let mut out = String::new();
        for number in numbers {
            let Some(node) = page.html.tree.get(node_id(&ctx, page, number)?) else {
                continue;
            };
            for n in node.descendants() {
                visit(&ctx)?;
                if let Some(t) = n.value().as_text() {
                    out.push_str(t);
                }
            }
        }
        Ok(out)
    })
}

/// A node's markup: outer or inner; node 0 is the whole page.
fn html(ctx: Ctx<'_>, doc: u32, number: u32, outer: bool) -> Result<String> {
    with_page(&ctx, doc, |page| {
        if number == 0 {
            return Ok(page.html.html());
        }
        let Some(el) = element(page, node_id(&ctx, page, number)?) else {
            return Ok(String::new());
        };
        Ok(if outer { el.html() } else { el.inner_html() })
    })
}

fn attr(ctx: Ctx<'_>, doc: u32, number: u32, name: String) -> Result<Option<String>> {
    with_page(&ctx, doc, |page| {
        Ok(element(page, node_id(&ctx, page, number)?)
            .and_then(|el| el.attr(&name).map(str::to_owned)))
    })
}

fn attrs<'js>(ctx: Ctx<'js>, doc: u32, number: u32) -> Result<Object<'js>> {
    let out = Object::new(ctx.clone())?;
    with_page(&ctx, doc, |page| {
        if let Some(el) = element(page, node_id(&ctx, page, number)?) {
            for (name, value) in el.value().attrs() {
                out.set(name, value)?;
            }
        }
        Ok(())
    })?;
    Ok(out)
}
