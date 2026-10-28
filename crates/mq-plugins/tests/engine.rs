//! Phase D (plans/plugins-and-marketplace.md 9): the engine's limits, its
//! message boundary, and the HTML caps.

use mq_plugins::dom::{self, Caps, Stop};
use mq_plugins::engine::{HostCall, HostReply, HostRequest, Limits, MAX_REQUESTS, Outcome, call};
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

/// Runs `script` with a host that answers every fetch with `url` reversed,
/// and says how many requests reached the host.
fn run_counted(script: &str, limits: Limits) -> (Outcome, usize) {
    let (tx, mut rx) = mpsc::unbounded_channel::<HostRequest>();
    let host = std::thread::spawn(move || {
        let mut seen = 0;
        while let Some(HostRequest { call, reply }) = rx.blocking_recv() {
            let HostCall::Fetch { url } = call;
            let _ = reply.send(HostReply::Body(url.chars().rev().collect()));
            seen += 1;
        }
        seen
    });
    let outcome = call(script.into(), limits, Arc::default(), tx)
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

/// Every kept node's depth is its parent's plus one, and within the cap.
fn depths_hold(doc: &dom::Doc) {
    for (i, node) in doc.nodes.iter().enumerate().skip(2) {
        if let Some(parent) = node.parent {
            assert_eq!(node.depth, doc.nodes[parent].depth + 1, "node {i}");
            assert!(node.depth as usize <= Caps::default().depth, "node {i}");
        }
    }
}

#[test]
fn a_plugin_answers_through_get_streams() {
    let out = run(
        r#"function getStreams() { return [{ url: "https://a.example/x.mp4" }]; }"#,
        quick(),
    );
    assert_eq!(
        out,
        Outcome::Done(r#"[{"url":"https://a.example/x.mp4"}]"#.into())
    );
}

#[test]
fn fetch_goes_to_the_host_as_a_message() {
    let out = run(
        r#"async function getStreams() { const a = await fetch("abc"); return [a, await fetch("xy")]; }"#,
        quick(),
    );
    assert_eq!(out, Outcome::Done(r#"["cba","yx"]"#.into()));
}

#[test]
fn a_call_makes_at_most_its_requests() {
    let script = r#"async function getStreams() {
        for (let i = 0; i < 40; i++) {
            try { await fetch("u" + i); } catch (e) { return [i, e.message]; }
        }
    }"#;
    let (out, seen) = run_counted(script, quick());
    assert_eq!(
        out,
        Outcome::Done(format!(r#"[{MAX_REQUESTS},"rate limited"]"#))
    );
    assert_eq!(seen, MAX_REQUESTS as usize);
}

#[test]
fn a_throw_or_a_missing_entry_is_said() {
    assert!(
        matches!(run("function getStreams() { throw new Error('nope'); }", quick()), Outcome::Threw(w) if w.contains("nope"))
    );
    assert!(matches!(run("var x = 1;", quick()), Outcome::Threw(_)));
}

#[test]
fn a_memory_bomb_stops_at_the_heap_limit() {
    let objects = "function getStreams() { const a = []; for (;;) a.push({ n: a.length, s: 'x' + a.length }); }";
    assert_eq!(run(objects, quick()), Outcome::Memory);
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
        run("function getStreams() { return 1; }", quick()),
        Outcome::Done("1".into())
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
    assert!(matches!(out, Outcome::Threw(_)), "{out:?}");
}

#[test]
fn no_shared_memory_webassembly_or_modules() {
    let probe = r#"async function getStreams() {
        let modules;
        try { await import("os"); modules = "loaded"; } catch (e) { modules = "refused"; }
        return [typeof SharedArrayBuffer, typeof Atomics, typeof WebAssembly, modules];
    }"#;
    assert_eq!(
        run(probe, quick()),
        Outcome::Done(r#"["undefined","undefined","undefined","refused"]"#.into())
    );
}

/// Cancel during a loop and during a fetch the host never answers: the
/// thread itself ends soon after.
#[test]
fn cancel_ends_the_thread() {
    for script in [
        "function getStreams() { for (;;) {} }",
        "async function getStreams() { await fetch('x'); }",
    ] {
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, _held) = mpsc::unbounded_channel::<HostRequest>();
        let limits = Limits {
            cpu: Duration::from_secs(10),
            wall: Duration::from_secs(10),
            ..Limits::default()
        };
        let thread = call(script.into(), limits, cancel.clone(), tx);
        std::thread::sleep(Duration::from_millis(100));
        let asked = Instant::now();
        cancel.store(true, Ordering::Relaxed);
        assert_eq!(thread.join().unwrap(), Outcome::Cancelled, "{script}");
        assert!(
            asked.elapsed() < Duration::from_millis(100),
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
    let deepest = doc.nodes.iter().map(|n| n.depth as usize).max();
    assert_eq!(deepest, Some(Caps::default().depth));
    assert!(doc.fed <= 4096, "fed {}", doc.fed);
    depths_hold(&doc);
    assert!(
        started.elapsed() < Duration::from_secs(1),
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
    ] {
        let html = attribute_bomb(prefix, name);
        let started = Instant::now();
        let doc = parse(&html);
        assert_eq!(doc.stopped, Some(Stop::Attrs), "{prefix}");
        assert_eq!(doc.fed, prefix.len(), "{prefix}");
        assert!(doc.nodes.iter().any(|n| n.text == "before"));
        assert!(
            started.elapsed() < Duration::from_millis(500),
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
