//! Phase D (plans/plugins-and-marketplace.md 9): the engine's limits, its
//! message boundary, and the HTML caps.

use mq_plugins::dom::{self, Caps, Kind, Stop};
use mq_plugins::engine::{
    Call, FetchResponse, HostCall, HostReply, HostRequest, Info, Limits, MAX_REQUESTS, MAX_URL,
    Media, Outcome, PluginStream, call,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

fn quick() -> Limits {
    Limits {
        cpu: Duration::from_millis(300),
        // Far past the CPU limit, so a loaded machine still ends a busy
        // call by CPU time, never by the wall.
        wall: Duration::from_secs(5),
        ..Limits::default()
    }
}

/// A call of `script` as written, for The Matrix.
fn raw(script: &str) -> Call {
    Call {
        script: script.into(),
        plugin_id: "test".into(),
        tmdb_id: "603".into(),
        media: Media::Movie,
        season: None,
        episode: None,
        info: Info {
            imdb_id: Some("tt0133093".into()),
            title: "The Matrix".into(),
            year: Some(1999),
        },
    }
}

/// `script` declares `getStreams` and may answer `say(value)`: one stream
/// whose name is the value as JSON, read back by `said`.
fn plugin(script: &str) -> Call {
    raw(&format!(
        "const say = (v) => [{{ url: 'u', name: JSON.stringify(v) }}];\n{script}\nmodule.exports = {{ getStreams }};"
    ))
}

fn said(out: &Outcome) -> String {
    match out {
        Outcome::Done(streams) => streams[0].name.clone().unwrap(),
        other => panic!("{other:?}"),
    }
}

/// Runs `script` with a host that answers every fetch with its URL
/// reversed, and says how many requests reached the host.
fn run_counted(script: &str, limits: Limits) -> (Outcome, usize) {
    let (tx, mut rx) = mpsc::unbounded_channel::<HostRequest>();
    let host = std::thread::spawn(move || {
        let mut seen = 0;
        while let Some(HostRequest { call, reply }) = rx.blocking_recv() {
            if let HostCall::Fetch(request) = call {
                let _ = reply.send(HostReply::Response(FetchResponse {
                    status: 200,
                    status_text: "OK".into(),
                    url: request.url.clone(),
                    headers: Vec::new(),
                    body: request.url.chars().rev().collect::<String>().into_bytes(),
                }));
                seen += 1;
            }
        }
        seen
    });
    let outcome = call(plugin(script), limits, Arc::default(), tx)
        .unwrap()
        .join()
        .unwrap();
    (outcome, host.join().unwrap())
}

fn run(script: &str, limits: Limits) -> Outcome {
    run_counted(script, limits).0
}

fn parse(html: &str) -> dom::Doc {
    dom::parse(html, Caps::default(), || false)
}

/// The nodes reachable from the document, which is the tree.
fn reachable(doc: &dom::Doc) -> Vec<usize> {
    let (mut seen, mut below) = (Vec::new(), vec![0]);
    while let Some(node) = below.pop() {
        seen.push(node);
        below.extend_from_slice(&doc.nodes[node].children);
    }
    seen
}

/// In the tree, every node's parent lists it, and its depth is its parent's
/// plus one and within the cap.
fn depths_hold(doc: &dom::Doc) {
    for i in reachable(doc).into_iter().skip(1) {
        let node = &doc.nodes[i];
        let parent = node.parent.expect("a parent in the tree");
        assert!(doc.nodes[parent].children.contains(&i), "node {i}");
        assert_eq!(node.depth, doc.nodes[parent].depth + 1, "node {i}");
        assert!(node.depth as usize <= Caps::default().depth, "node {i}");
    }
}

/// The body as `name[children]` and `"text"`, comments as `!`.
fn shape(html: &str) -> String {
    fn put(doc: &dom::Doc, node: usize, out: &mut String) {
        let node = &doc.nodes[node];
        match node.kind {
            Kind::Text => out.push_str(&format!("{:?}", node.text)),
            Kind::Comment => out.push('!'),
            _ => {
                out.push_str(&node.name.as_ref().map_or("", |q| &*q.local).to_string());
                out.push('[');
                for (i, &child) in node.children.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    put(doc, child, out);
                }
                out.push(']');
            }
        }
    }
    let doc = parse(html);
    let body = doc
        .nodes
        .iter()
        .position(|n| n.name.as_ref().is_some_and(|q| &*q.local == "body"))
        .unwrap();
    let mut out = String::new();
    put(&doc, body, &mut out);
    out
}

/// Joins the call's thread, or None after `secs`: a thread stuck in a
/// native loop fails the test instead of hanging it.
fn joined_within(thread: std::thread::JoinHandle<Outcome>, secs: u64) -> Option<Outcome> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || tx.send(thread.join().unwrap()));
    rx.recv_timeout(Duration::from_secs(secs)).ok()
}

#[test]
fn a_plugin_answers_through_get_streams() {
    let out = run(
        r#"function getStreams() { return [{ url: "https://a.example/x.mp4" }]; }"#,
        quick(),
    );
    let stream = PluginStream {
        url: "https://a.example/x.mp4".into(),
        ..Default::default()
    };
    assert_eq!(out, Outcome::Done(vec![stream]));
}

#[test]
fn fetch_goes_to_the_host_as_a_message() {
    let out = run(
        r#"async function getStreams() { const a = await (await fetch("abc")).text(); return say([a, await (await fetch("xy")).text()]); }"#,
        quick(),
    );
    assert_eq!(said(&out), r#"["cba","yx"]"#);
}

#[test]
fn a_call_makes_at_most_its_requests() {
    let script = r#"async function getStreams() {
        for (let i = 0; i < 40; i++) {
            try { await fetch("u" + i); } catch (e) { return say([i, e.message]); }
        }
    }"#;
    let (out, seen) = run_counted(script, quick());
    assert_eq!(said(&out), format!(r#"[{MAX_REQUESTS},"rate limited"]"#));
    assert_eq!(seen, MAX_REQUESTS as usize);
}

#[test]
fn a_throw_or_a_missing_entry_is_said() {
    assert!(
        matches!(run("function getStreams() { throw new Error('nope'); }", quick()), Outcome::Threw(w) if w.contains("nope"))
    );
    let (tx, _rx) = mpsc::unbounded_channel::<HostRequest>();
    let missing = call(raw("var x = 1;"), quick(), Arc::default(), tx).unwrap();
    assert_eq!(
        missing.join().unwrap(),
        Outcome::Threw("no getStreams".into())
    );
}

#[test]
fn a_memory_bomb_stops_at_the_heap_limit() {
    // Two seconds of CPU, so a slow machine still meets the heap first.
    let roomy = Limits {
        cpu: Duration::from_secs(2),
        ..quick()
    };
    let objects = "function getStreams() { const a = []; for (;;) a.push({ n: a.length, s: 'x' + a.length }); }";
    assert_eq!(run(objects, roomy), Outcome::Memory);
    // Inside a promise job too.
    let later = "async function getStreams() { await null; const a = []; for (;;) a.push({ s: 'x' + a.length }); }";
    assert_eq!(run(later, roomy), Outcome::Memory);
    // A doubling string fails too: QuickJS-NG caps a string's length before
    // the heap limit is reached. Seen 2026-10-27 under a 24 MiB limit: the
    // call threw "string too long". The heap limit would also do; finishing never.
    let doubling = run(
        "function getStreams() { let s = 'x'; for (;;) s += s; }",
        quick(),
    );
    assert!(
        doubling == Outcome::Threw("string too long".into()) || doubling == Outcome::Memory,
        "{doubling:?}"
    );
    // The engine is fine afterwards: a call right after answers.
    assert_eq!(
        said(&run("function getStreams() { return say(1); }", quick())),
        "1"
    );
}

#[test]
fn a_busy_loop_stops_at_the_cpu_limit() {
    let started = Instant::now();
    assert_eq!(
        run("function getStreams() { for (;;) {} }", quick()),
        Outcome::Cpu
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    // In a promise and in a regular expression too.
    assert_eq!(
        run(
            "async function getStreams() { await null; for (;;) {} }",
            quick()
        ),
        Outcome::Cpu
    );
    let regex = "function getStreams() { return /(a+)+b/.test('a'.repeat(40)); }";
    assert_eq!(run(regex, quick()), Outcome::Cpu);
    // The interrupt cannot be caught.
    let caught = "function getStreams() { for (;;) { try { for (;;) {} } catch (e) {} } }";
    assert_eq!(run(caught, quick()), Outcome::Cpu);
}

#[test]
fn a_promise_that_never_settles_ends_at_the_wall() {
    let started = Instant::now();
    assert_eq!(
        run(
            "function getStreams() { return new Promise(() => {}); }",
            Limits {
                wall: Duration::from_secs(1),
                ..quick()
            }
        ),
        Outcome::Wall
    );
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn deep_recursion_throws_instead_of_crashing() {
    let out = run(
        "function f() { return f(); } function getStreams() { return f(); }",
        quick(),
    );
    assert!(
        matches!(&out, Outcome::Threw(w) if w.contains("stack")),
        "{out:?}"
    );
}

/// A loop of calls past the cap is refused before anything is sent, so
/// nothing piles up on the host side, and a long URL never leaves the heap.
#[test]
fn fetch_refuses_at_once() {
    let (out, seen) = run_counted("function getStreams() { for (;;) fetch('x'); }", quick());
    assert!(out == Outcome::Cpu || out == Outcome::Memory, "{out:?}");
    assert_eq!(seen, MAX_REQUESTS as usize);
    let all = "async function getStreams() { return Promise.all(Array.from({ length: 40 }, (_, i) => fetch('u' + i))); }";
    let (out, seen) = run_counted(all, quick());
    assert_eq!(out, Outcome::Threw("rate limited".into()));
    assert_eq!(seen, MAX_REQUESTS as usize);
    let long = format!(
        "function getStreams() {{ return fetch('x'.repeat({})); }}",
        MAX_URL + 1
    );
    let (out, seen) = run_counted(&long, quick());
    assert_eq!(out, Outcome::Threw("URL too long".into()));
    assert_eq!(seen, 0);
}

/// QuickJS-NG walks a generic array-like in C loops that never read the
/// interrupt; every `Array.prototype` method refuses a long one, and the
/// call ends.
#[test]
fn long_array_likes_are_refused() {
    let script = r#"function getStreams() {
        const out = {};
        const sparse = []; sparse.length = 2 ** 32 - 1;
        for (const name of Object.getOwnPropertyNames(Array.prototype)) {
            const method = Array.prototype[name];
            if (typeof method !== "function" || name === "constructor") continue;
            for (const that of [{ length: 2 ** 53 - 1 }, sparse]) {
                try { method.call(that, () => false, 0); out[name] = "ran"; }
                catch (e) { out[name] = e.name; if (e.name !== "RangeError") break; }
            }
        }
        return say(out);
    }"#;
    let (tx, _rx) = mpsc::unbounded_channel::<HostRequest>();
    let thread = call(plugin(script), quick(), Arc::default(), tx).unwrap();
    let out = joined_within(thread, 20).expect("the call ends");
    let json = said(&out);
    let names: std::collections::BTreeMap<String, String> = serde_json::from_str(&json).unwrap();
    assert!(names.len() > 30, "{json}");
    assert!(names.values().all(|e| e == "RangeError"), "{json}");
}

#[test]
fn no_shared_memory_webassembly_or_modules() {
    let probe = r#"async function getStreams() {
        let modules;
        try { await import("os"); modules = "loaded"; } catch (e) { modules = "refused"; }
        return say([typeof SharedArrayBuffer, typeof Atomics, typeof WebAssembly, modules]);
    }"#;
    assert_eq!(
        said(&run(probe, quick())),
        r#"["undefined","undefined","undefined","refused"]"#
    );
}

/// Cancel during a loop and during a fetch the host holds: the thread itself
/// ends long before its 10 s limits.
#[test]
fn cancel_ends_the_thread() {
    for script in [
        "function getStreams() { for (;;) {} }",
        "async function getStreams() { await fetch('x'); }",
    ] {
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, mut rx) = mpsc::unbounded_channel::<HostRequest>();
        let limits = Limits {
            cpu: Duration::from_secs(10),
            wall: Duration::from_secs(10),
            ..Limits::default()
        };
        let thread = call(plugin(script), limits, cancel.clone(), tx).unwrap();
        // The fetch is held once it reached the host; the loop is running.
        let _held = if script.contains("fetch") {
            Some(rx.blocking_recv().unwrap())
        } else {
            std::thread::sleep(Duration::from_millis(100));
            None
        };
        let asked = Instant::now();
        cancel.store(true, Ordering::Relaxed);
        assert_eq!(
            joined_within(thread, 20),
            Some(Outcome::Cancelled),
            "{script}"
        );
        assert!(
            asked.elapsed() < Duration::from_secs(2),
            "{script}: {:?}",
            asked.elapsed()
        );
    }
}

#[test]
fn a_page_parses_whole_within_its_caps() {
    let html = r#"<html><body><ul><li><a href="/x">X</a></li><li>Y</li></ul></body></html>"#;
    let doc = parse(html);
    assert_eq!(doc.stopped, None);
    assert_eq!(doc.fed, html.len());
    let link = doc
        .nodes
        .iter()
        .find(|n| n.name.as_ref().is_some_and(|q| &*q.local == "a"))
        .unwrap();
    assert_eq!(&*link.attrs[0].value, "/x");
    assert_eq!(doc.nodes[link.children[0]].text, "X");
}

/// The sink's counter met the node cap exactly, and the parser was given
/// only the input up to the chunk where it did.
#[test]
fn four_mib_of_links_stops_at_the_node_cap() {
    let html = "<a>".repeat(4 * 1024 * 1024 / 3);
    let doc = parse(&html);
    assert_eq!(doc.stopped, Some(Stop::Nodes));
    assert_eq!(doc.nodes.len(), Caps::default().nodes);
    // 150,000 `<a>` are 450,000 bytes; one chunk past them at most.
    assert!(doc.fed < 460_000, "fed {} of {}", doc.fed, html.len());
    depths_hold(&doc);
}

/// The deepest kept node sits at the depth cap, and the parser was given
/// one chunk of the 5 MB.
#[test]
fn a_million_nested_divs_stop_at_the_depth_cap() {
    let html = "<div>".repeat(1_000_000);
    let started = Instant::now();
    let doc = parse(&html);
    assert_eq!(doc.stopped, Some(Stop::Depth));
    let deepest = reachable(&doc)
        .iter()
        .map(|&n| doc.nodes[n].depth as usize)
        .max();
    assert_eq!(deepest, Some(Caps::default().depth));
    assert!(doc.fed <= 4096, "fed {}", doc.fed);
    depths_hold(&doc);
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );
}

/// `<` + `name`, then attributes to 4 MiB in all.
fn attribute_bomb(prefix: &str, name: &str) -> String {
    let mut html = format!("{prefix}<{name}");
    let mut i = 0;
    while html.len() < 4 * 1024 * 1024 - 16 {
        html.push_str(&format!(" a{i}"));
        i += 1;
    }
    html + ">"
}

/// One 4 MiB tag of attributes: html5ever checks each name against the
/// earlier ones, minutes of work before the sink sees the tag
/// (bench/plugins/d-spike.jsonl). The scan cuts the input before that tag,
/// in a start tag, an end tag, and inside SVG, where `<style>` is markup.
#[test]
fn an_attribute_bomb_never_reaches_the_tokenizer() {
    for (prefix, name) in [
        ("<p>before</p>", "a"),
        ("<p>before</p>", "/a"),
        ("<p>before</p><svg><style>", "a"),
        // The frameset modes ignore raw-text start tags, so the tokenizer
        // reads what follows them as markup.
        ("<p>before</p><frameset><textarea>", "a"),
        ("<p>before</p><frameset><plaintext>", "a"),
        ("<p>before</p><frameset><script>", "a"),
    ] {
        let html = attribute_bomb(prefix, name);
        let started = Instant::now();
        let doc = parse(&html);
        assert_eq!(doc.stopped, Some(Stop::Attrs), "{prefix}");
        assert_eq!(doc.fed, prefix.len(), "{prefix}");
        assert!(doc.nodes.iter().any(|n| n.text == "before"));
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "{prefix}: {:?}",
            started.elapsed()
        );
    }
}

#[test]
fn a_tag_over_the_length_cap_never_reaches_the_tokenizer() {
    let prefix = "<p>before</p>";
    let html = format!(r#"{prefix}<a title="{}">x</a>"#, "t".repeat(300 * 1024));
    let doc = parse(&html);
    assert_eq!(doc.stopped, Some(Stop::Tag));
    assert_eq!(doc.fed, prefix.len());
}

/// Text html5ever reads raw (scripts, styles, comments, quoted values) is
/// not taken for tags, so ordinary pages parse whole.
#[test]
fn raw_text_is_not_read_as_tags() {
    let words = "x ".repeat(200);
    for html in [
        format!("<script>for (i = 0; i<n {words}; i++) {{}}</script><p>after</p>"),
        format!("<SCRIPT>if (a<b {words}) {{}}</script ><p>after</p>"),
        format!("<style>a<b {words}</style><p>after</p>"),
        format!("<textarea><b {words}</textarea><p>after</p>"),
        format!("<!-- <b {words} --><p>after</p>"),
        format!(r#"<p title="<b {words}">after</p>"#),
        format!("<svg/><script>a<b {words}</script><p>after</p>"),
        format!("<svg></svg><script>a<b {words}</script><p>after</p>"),
    ] {
        let doc = parse(&html);
        assert_eq!(doc.stopped, None, "{html}");
        assert_eq!(doc.fed, html.len(), "{html}");
    }
}

/// Whatever the scan does not foresee still ends at the call's budget,
/// read between chunks.
#[test]
fn a_spent_budget_stops_the_parse() {
    let html = "<p>x</p>".repeat(10_000);
    let doc = dom::parse(&html, Caps::default(), || true);
    assert_eq!(doc.stopped, Some(Stop::Budget));
    assert_eq!(doc.fed, 0);
}

/// Misnested formatting elements make html5ever move whole subtrees (the
/// adoption agency): the moved nodes take their new depth, so the cap holds.
#[test]
fn moved_subtrees_keep_true_depths() {
    for html in [
        format!("{}{}", "<b><div>".repeat(200), "</b>x".repeat(200)),
        // `</b>` drops the `<span>` between it and the `<div>`, which moves up
        // two levels with everything under it.
        "<b><span><div><p><q>x</q></p></b>".into(),
        format!(
            "{}{}",
            "<b><span><div><em><u>x".repeat(60),
            "</b>".repeat(60)
        ),
        format!("{}{}", "<a><p>".repeat(300), "</a>".repeat(300)),
        format!("<table>{}</table>", "<b><tr><td><i>".repeat(150)),
        format!(
            "{}{}",
            "<em><span><div>".repeat(120),
            "</em><div>y".repeat(120)
        ),
    ] {
        depths_hold(&parse(&html));
    }
}

#[test]
fn a_page_over_the_byte_cap_keeps_its_start() {
    let caps = Caps {
        bytes: 64,
        ..Caps::default()
    };
    let doc = dom::parse(
        &format!("<p>first</p>{}", "<p>x</p>".repeat(100)),
        caps,
        || false,
    );
    assert_eq!(doc.stopped, Some(Stop::Bytes));
    assert!(doc.nodes.iter().any(|n| n.text == "first"));
}

/// The tree is the parse: foster-parented content stays before its table,
/// text after a comment is its own node, and misnested formatting splits.
#[test]
fn the_tree_keeps_document_order() {
    assert_eq!(
        shape("<table>X<tr><td>1</td></tr></table>Y"),
        r#"body["X",table[tbody[tr[td["1"]]]],"Y"]"#
    );
    assert_eq!(
        shape("<table><a href=x>l</a><tr><td>1</td></tr></table>"),
        r#"body[a["l"],table[tbody[tr[td["1"]]]]]"#
    );
    assert_eq!(
        shape("<p><!--ad-->Watch here</p>"),
        r#"body[p[!,"Watch here"]]"#
    );
    assert_eq!(shape("<b>1<p>2</b>3</p>"), r#"body[b["1"],p[b["2"],"3"]]"#);
}

/// Attributes count against the node cap, so reconstructed formatting
/// elements cannot copy their attributes past it.
#[test]
fn attributes_count_against_the_node_cap() {
    let doc = parse(&"<p a=1 b=2 c=3>".repeat(100_000));
    assert_eq!(doc.stopped, Some(Stop::Nodes));
    let units: usize = doc.nodes.iter().map(|n| 1 + n.attrs.len()).sum();
    assert!(units <= Caps::default().nodes, "{units}");
    assert!(
        doc.nodes.len() <= Caps::default().nodes / 4 + 2,
        "{}",
        doc.nodes.len()
    );
}
