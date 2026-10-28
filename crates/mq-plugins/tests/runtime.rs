//! Phase E1 (plans/plugins-and-marketplace.md 6.2 to 6.4, 13.12): Nuvio's
//! wrapper and entry, the web APIs of the prelude, cheerio and crypto-js,
//! and the field-by-field read of what a plugin returns.

use mq_plugins::engine::{
    Call, FetchRequest, FetchResponse, HostCall, HostReply, HostRequest, Info, Limits, LogLevel,
    Media, Method, Outcome, PluginStream, PluginSubtitle, call,
};
use mq_plugins::natives::{CRYPTO_JS, HOST_BYTES, MAX_BODY, MAX_LOG};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

fn limits() -> Limits {
    Limits {
        cpu: Duration::from_secs(2),
        wall: Duration::from_secs(10),
        ..Limits::default()
    }
}

fn movie(script: &str) -> Call {
    Call {
        script: script.into(),
        plugin_id: "test.plugin".into(),
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

/// `body` runs inside `getStreams` (async), with `say(value)` answering one
/// stream whose name is the value as JSON.
fn plugin(body: &str) -> Call {
    movie(&format!(
        "const say = (v) => [{{ url: 'u', name: JSON.stringify(v) }}];\n\
         async function getStreams() {{\n{body}\n}}\n\
         module.exports = {{ getStreams }};"
    ))
}

fn said(out: &Outcome) -> serde_json::Value {
    match out {
        Outcome::Done(streams) => {
            serde_json::from_str(streams[0].name.as_deref().unwrap()).unwrap()
        }
        other => panic!("{other:?}"),
    }
}

/// What the host saw.
#[derive(Default)]
struct Seen {
    fetches: Vec<FetchRequest>,
    logs: Vec<(LogLevel, String)>,
}

/// Runs `call` with `answer` as the host; every log and fetch is kept.
fn run_with(
    call_: Call,
    answer: impl Fn(&FetchRequest) -> HostReply + Send + 'static,
) -> (Outcome, Seen) {
    let (tx, mut rx) = mpsc::unbounded_channel::<HostRequest>();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let kept = seen.clone();
    let host = std::thread::spawn(move || {
        while let Some(HostRequest { call, reply }) = rx.blocking_recv() {
            match call {
                HostCall::Fetch(request) => {
                    let _ = reply.send(answer(&request));
                    kept.lock().unwrap().fetches.push(request);
                }
                HostCall::Log { level, text } => kept.lock().unwrap().logs.push((level, text)),
            }
        }
    });
    let out = call(call_, limits(), Arc::default(), tx)
        .unwrap()
        .join()
        .unwrap();
    host.join().unwrap();
    let seen = std::mem::take(&mut *seen.lock().unwrap());
    (out, seen)
}

fn ok(body: &str) -> impl Fn(&FetchRequest) -> HostReply + Send + 'static {
    let body = body.to_owned();
    move |request| {
        HostReply::Response(FetchResponse {
            status: 200,
            status_text: "OK".into(),
            url: request.url.clone(),
            headers: vec![("content-type".into(), "text/html".into())],
            body: body.clone().into_bytes(),
        })
    }
}

fn run(call_: Call) -> Outcome {
    run_with(call_, ok("")).0
}

/// Nuvio's wrapper keeps the script's declarations to itself; the entry is
/// `module.exports.getStreams`, else a global one, called with Nuvio's four
/// arguments and our `info`.
#[test]
fn the_entry_and_its_arguments() {
    let echo =
        "async function getStreams(...args) { return [{ url: 'u', name: JSON.stringify(args) }]; }";
    let exported = movie(&format!("{echo}\nmodule.exports = {{ getStreams }};"));
    assert_eq!(
        said(&run(exported)),
        serde_json::json!(["603", "movie", null, null, {"imdbId": "tt0133093", "title": "The Matrix", "year": 1999}])
    );
    let mut episode = movie(&format!("{echo}\nglobalThis.getStreams = getStreams;"));
    (episode.media, episode.season, episode.episode) = (Media::Tv, Some(1), Some(2));
    assert_eq!(
        said(&run(episode))[0..4],
        serde_json::json!(["603", "tv", 1, 2]).as_array().unwrap()[..]
    );
    // Declared but not exported: not found, as on Nuvio.
    assert_eq!(run(movie(echo)), Outcome::Threw("no getStreams".into()));
    let globals = plugin("return say([SCRAPER_ID, TMDB_API_KEY, typeof window, typeof require]);");
    assert_eq!(
        said(&run(globals)),
        serde_json::json!(["test.plugin", "", "object", "function"])
    );
}

/// The request as the host receives it, and the response as the plugin reads it.
#[test]
fn fetch_round_trip() {
    let body = r#"
        const r = await fetch("https://a.example/p?q=1", {
            method: "post",
            headers: { "X-Thing": "1", Referer: "https://b.example/" },
            body: new URLSearchParams({ a: "b c" }),
            redirect: "manual",
        });
        const again = r.clone();
        return say([r.ok, r.status, r.statusText, r.url, r.headers.get("Content-Type"), await r.text(), await again.json(), r.bodyUsed]);
    "#;
    let (out, seen) = run_with(plugin(body), ok("{\"x\":1}"));
    assert_eq!(
        said(&out),
        serde_json::json!([true, 200, "OK", "https://a.example/p?q=1", "text/html", "{\"x\":1}", {"x": 1}, true])
    );
    let request = &seen.fetches[0];
    assert_eq!(request.method, Method::Post);
    assert_eq!(request.url, "https://a.example/p?q=1");
    assert_eq!(
        request.headers,
        vec![
            ("x-thing".into(), "1".into()),
            ("referer".into(), "https://b.example/".into()),
            (
                "content-type".into(),
                "application/x-www-form-urlencoded;charset=UTF-8".into()
            ),
        ]
    );
    assert_eq!(request.body.as_deref(), Some(&b"a=b+c"[..]));
    assert!(!request.follow);
    assert_eq!(request.max_bytes, MAX_BODY);
}

/// A non-2xx answer resolves with `ok` false; a refusal rejects with the
/// host's words; a broken JSON body reads as null, as on Nuvio.
#[test]
fn fetch_failures() {
    let missing = |r: &FetchRequest| {
        if r.url.contains("refused") {
            return HostReply::Failed("not allowed: refused.example".into());
        }
        HostReply::Response(FetchResponse {
            status: 404,
            status_text: "Not Found".into(),
            url: r.url.clone(),
            headers: Vec::new(),
            body: b"<html>".to_vec(),
        })
    };
    let body = r#"
        const r = await fetch("https://a.example/x");
        let refused;
        try { await fetch("https://refused.example/"); } catch (e) { refused = [e.name, e.message]; }
        let method;
        try { await fetch("https://a.example/", { method: "PUT" }); } catch (e) { method = e.message; }
        return say([r.ok, r.status, await r.json(), refused, method]);
    "#;
    let (out, seen) = run_with(plugin(body), missing);
    assert_eq!(
        said(&out),
        serde_json::json!([
            false,
            404,
            null,
            ["TypeError", "not allowed: refused.example"],
            "method not allowed: PUT"
        ])
    );
    assert_eq!(seen.fetches.len(), 2);
}

/// Decoded bodies draw from the call's 16 MiB: each request's cap shrinks
/// with what is left, and a spent call refuses before asking the host.
#[test]
fn bodies_draw_from_the_host_bytes() {
    let full = |r: &FetchRequest| {
        HostReply::Response(FetchResponse {
            status: 200,
            status_text: "OK".into(),
            url: r.url.clone(),
            headers: Vec::new(),
            body: vec![b'x'; r.max_bytes],
        })
    };
    let body = r#"
        const sizes = [];
        for (let i = 0; i < 6; i++) {
            try { sizes.push((await (await fetch("https://a.example/" + i)).arrayBuffer()).byteLength); }
            catch (e) { sizes.push(e.message); }
        }
        return say(sizes);
    "#;
    let (out, seen) = run_with(plugin(body), full);
    let per = HOST_BYTES / MAX_BODY;
    let mut expected: Vec<serde_json::Value> = vec![serde_json::json!(MAX_BODY); per];
    expected.resize(6, serde_json::json!("the call's 16 MiB are spent"));
    // The bodies stay in the JS heap too; a heap of 24 MiB may meet its own
    // limit first, which also ends the call.
    match out {
        Outcome::Memory => {}
        out => assert_eq!(said(&out), serde_json::Value::Array(expected)),
    }
    assert!(seen.fetches.len() <= per, "{}", seen.fetches.len());
    assert!(seen.fetches.iter().all(|r| r.max_bytes == MAX_BODY));
}

#[test]
fn console_goes_to_the_host_within_its_budget() {
    let body = r#"
        console.log("one", 2, { three: 3 });
        console.error(new TypeError("four"));
        for (let i = 0; i < 1000; i++) console.debug("x".repeat(100));
        return say(1);
    "#;
    let (_, seen) = run_with(plugin(body), ok(""));
    assert_eq!(
        seen.logs[0],
        (LogLevel::Info, r#"one 2 {"three":3}"#.into())
    );
    assert_eq!(seen.logs[1], (LogLevel::Error, "TypeError: four".into()));
    let said: usize = seen.logs.iter().map(|(_, t)| t.len()).sum();
    assert!(said <= MAX_LOG, "{said}");
}

#[test]
fn timers() {
    let body = r#"
        const order = [];
        await new Promise((done) => {
            setTimeout(() => order.push("b"), 20);
            setTimeout(() => order.push("a"), 5);
            const never = setTimeout(() => order.push("never"), 10);
            clearTimeout(never);
            let ticks = 0;
            const every = setInterval(() => { if (++ticks === 3) { clearInterval(every); order.push("i3"); } }, 10);
            setTimeout(done, 80);
        });
        let refused;
        try { for (let i = 0; i < 2000; i++) setTimeout(() => {}, 1); } catch (e) { refused = e.message; }
        return say([order, refused]);
    "#;
    assert_eq!(
        said(&run(plugin(body))),
        serde_json::json!([["a", "b", "i3"], "too many timers"])
    );
}

#[test]
fn url_and_search_params() {
    let body = r#"
        const u = new URL("/search?q=a%20b&x=1#top", "https://Example.com/base/");
        const before = [u.href, u.host, u.pathname, u.search, u.hash, u.origin, u.searchParams.get("q")];
        u.searchParams.set("q", "c&d");
        u.searchParams.append("e", "é");
        const after = u.href;
        u.search = "?z=9";
        const reset = u.searchParams.get("z");
        let bad;
        try { new URL("not a url"); } catch (e) { bad = e.name; }
        const p = new URLSearchParams([["a", "1"], ["a", "2"]]);
        p.delete("a", "1");
        return say([before, after, reset, bad, p.toString(), URL.canParse("x:"), String(new URLSearchParams("?k=v+w"))]);
    "#;
    assert_eq!(
        said(&run(plugin(body))),
        serde_json::json!([
            [
                "https://example.com/search?q=a%20b&x=1#top",
                "example.com",
                "/search",
                "?q=a%20b&x=1",
                "#top",
                "https://example.com",
                "a b"
            ],
            "https://example.com/search?q=c%26d&x=1&e=%C3%A9#top",
            "9",
            "TypeError",
            "a=2",
            true,
            "k=v+w"
        ])
    );
}

#[test]
fn bytes_base64_and_random() {
    let body = r#"
        const bytes = new TextEncoder().encode("é€");
        const text = new TextDecoder().decode(bytes);
        const latin = btoa("hiÿ");
        let wide;
        try { btoa("€"); } catch (e) { wide = "refused"; }
        const r = crypto.getRandomValues(new Uint8Array(16));
        const uuid = crypto.randomUUID();
        return say([Array.from(bytes), text, latin, atob(" aGn/ "), wide, r.length, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(uuid)]);
    "#;
    assert_eq!(
        said(&run(plugin(body))),
        serde_json::json!([
            [195, 169, 226, 130, 172],
            "é€",
            "aGn/",
            "hi\u{ff}",
            "refused",
            16,
            true
        ])
    );
}

const PAGE: &str = r#"<html><body>
    <ul id="list">
        <li class="item hd"><a href="/one" data-q="1080">One</a> <span>1.2 GB</span></li>
        <li class="item"><a href="/two">Two</a><!-- ad --> <span>700 MB</span></li>
        <li class="other"><b>Three</b></li>
    </ul>
    <table>X<tr><td>cell</td></tr></table>
</body></html>"#;

#[test]
fn cheerio_subset() {
    let body = format!(
        r##"
        const cheerio = require("cheerio");
        const $ = cheerio.load({PAGE:?});
        const links = [];
        $("li.item a").each((i, el) => links.push([i, $(el).attr("href"), $(el).text()]));
        const rows = $("#list li").map((i, el) => $(el).find("span").text()).get();
        const first = $("li").first();
        return say([
            links,
            rows,
            $("li").length,
            $("li").eq(-1).text(),
            first.next().find("a").attr("href"),
            first.next().prev().hasClass("hd"),
            $("a").first().parent().is("li"),
            $("span").first().closest("ul").attr("id"),
            $("#list").children().length,
            $("#list").children(".item").length,
            $("li").filter(".other").text(),
            $("li").filter((i) => i === 1).find("a").text(),
            $("a[data-q]").data("q"),
            $("a").first().attr(),
            $("li.hd").html(),
            $.html($("b")),
            $("td", "table").text(),
            require("react-native-cheerio") === cheerio,
        ]);
    "##
    );
    assert_eq!(
        said(&run(plugin(&body))),
        serde_json::json!([
            [[0, "/one", "One"], [1, "/two", "Two"]],
            ["1.2 GB", "700 MB", ""],
            3,
            "Three",
            "/two",
            true,
            true,
            "list",
            3,
            2,
            "Three",
            "Two",
            "1080",
            {"href": "/one", "data-q": "1080"},
            "<a href=\"/one\" data-q=\"1080\">One</a> <span>1.2 GB</span>",
            "<b>Three</b>",
            "cell",
            true
        ])
    );
}

#[test]
fn cheerio_refuses_bad_selectors_and_too_much_work() {
    let many = "<p>x</p>".repeat(30_000);
    let body = format!(
        r#"
        const $ = require("cheerio").load({many:?});
        let bad, work;
        try {{ $("a[[["); }} catch (e) {{ bad = e.name; }}
        try {{ for (let i = 0; i < 10; i++) $("p"); }} catch (e) {{ work = [e.name, e.message]; }}
        return say([bad, work]);
    "#
    );
    assert_eq!(
        said(&run(plugin(&body))),
        serde_json::json!(["SyntaxError", ["RangeError", "too much selector work"]])
    );
}

#[test]
fn crypto_js_is_the_pinned_release() {
    use sha2::Digest;
    assert_eq!(
        hex::encode(sha2::Sha256::digest(CRYPTO_JS.as_bytes())),
        "ee02257ffbaf0a9b481c7039b0f3bb20c360c9674fe4be8b38ae709b2ea59bbe"
    );
    let body = r#"
        const C = require("crypto-js");
        const key = C.enc.Utf8.parse("0123456789abcdef");
        const sealed = C.AES.encrypt("secret", key, { iv: key }).toString();
        const opened = C.AES.decrypt(sealed, key, { iv: key }).toString(C.enc.Utf8);
        return say([C.MD5("abc").toString(), CryptoJS.SHA256("abc").toString(C.enc.Base64), opened, C === CryptoJS]);
    "#;
    assert_eq!(
        said(&run(plugin(body))),
        serde_json::json!([
            "900150983cd24fb0d6963f7d28e17f72",
            "ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=",
            "secret",
            true
        ])
    );
}

/// What a plugin returns is read field by field: only the listed fields
/// cross, getters that throw drop their stream, and the caps hold.
#[test]
fn streams_are_read_field_by_field() {
    let body = r#"
        const bad = {};
        Object.defineProperty(bad, "url", { get() { throw new Error("no"); } });
        const proxied = new Proxy({ url: "https://p.example/v.m3u8" }, { get(t, k) { return k === "title" ? 42 : t[k]; } });
        return [
            { url: "https://a.example/1.mp4", name: "A", title: "T\nline", quality: 1080, size: "1.2 GB", language: "en",
              headers: { Referer: "https://a.example/", "X-N": 5 }, subtitles: [{ url: "https://a.example/s.vtt", language: "en" }, { lang: "x" }],
              infoHash: "abc", seeders: 9, behaviorHints: { notWebReady: true }, provider: "p" },
            bad,
            { url: { url: "https://b.example/2.mp4" }, name: { toString() { return "object"; } } },
            { url: "x".repeat(9000) },
            { url: "" },
            { name: "no url" },
            proxied,
            "not an object",
        ];
    "#;
    let out = run(movie(&format!(
        "async function getStreams() {{ {body} }}\nmodule.exports = {{ getStreams }};"
    )));
    assert_eq!(
        out,
        Outcome::Done(vec![
            PluginStream {
                url: "https://a.example/1.mp4".into(),
                name: Some("A".into()),
                title: Some("T\nline".into()),
                quality: Some("1080".into()),
                size: Some("1.2 GB".into()),
                language: Some("en".into()),
                headers: vec![("Referer".into(), "https://a.example/".into())],
                subtitles: vec![PluginSubtitle {
                    url: "https://a.example/s.vtt".into(),
                    lang: Some("en".into()),
                }],
            },
            PluginStream {
                url: "https://b.example/2.mp4".into(),
                ..Default::default()
            },
            PluginStream {
                url: "https://p.example/v.m3u8".into(),
                title: Some("42".into()),
                ..Default::default()
            },
        ])
    );
}

#[test]
fn stream_caps() {
    let many = run(plugin(
        "return Array.from({ length: 150 }, (_, i) => ({ url: 'https://a.example/' + i }));",
    ));
    let Outcome::Done(streams) = many else {
        panic!("{many:?}")
    };
    assert_eq!(streams.len(), 100);
    // 1 KiB names: 256 KiB in all holds fewer than the 100.
    let wide = run(plugin(
        "return Array.from({ length: 100 }, (_, i) => ({ url: 'https://a.example/' + i, name: 'n'.repeat(1000), title: 'n'.repeat(1000), size: 'n'.repeat(1000) }));",
    ));
    let Outcome::Done(streams) = wide else {
        panic!("{wide:?}")
    };
    let total: usize = streams.iter().map(|s| s.url.len() + 3000).sum();
    assert!(
        streams.len() < 100 && total <= 256 * 1024,
        "{} {total}",
        streams.len()
    );
    // A name over 1 KiB drops its stream; null and a non-array read as none.
    let long = run(plugin(
        "return [{ url: 'https://a.example/', name: 'n'.repeat(1025) }, { url: 'https://b.example/' }];",
    ));
    assert_eq!(
        long,
        Outcome::Done(vec![PluginStream {
            url: "https://b.example/".into(),
            ..Default::default()
        }])
    );
    assert_eq!(run(plugin("return null;")), Outcome::Done(Vec::new()));
    assert_eq!(
        run(plugin("return { url: 'x' };")),
        Outcome::Done(Vec::new())
    );
}

/// A cancelled call drops the replies it was waiting on, so the host sees
/// them closed and can abort those requests.
#[test]
fn cancel_closes_pending_replies() {
    let (tx, mut rx) = mpsc::unbounded_channel::<HostRequest>();
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let thread = call(
        plugin("await Promise.all([fetch('https://a.example/1'), fetch('https://a.example/2')]);"),
        limits(),
        cancel.clone(),
        tx,
    )
    .unwrap();
    let held = [rx.blocking_recv().unwrap(), rx.blocking_recv().unwrap()];
    assert!(held.iter().all(|r| !r.reply.is_closed()));
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(thread.join().unwrap(), Outcome::Cancelled);
    assert!(held.iter().all(|r| r.reply.is_closed()));
}

/// The e2e's fixture provider (tests/fixtures/plugin-repo, plan 13.14),
/// with `{{BASE}}` as the e2e serves it: one lookup on its own host, then
/// one row of the suite's test film.
#[test]
fn the_fixture_provider_runs() {
    let base = "https://plugins.example:8443";
    let script = include_str!("fixtures/plugin-repo/test-film.js").replace("{{BASE}}", base);
    let (out, seen) = run_with(movie(&script), ok(r#"{"title": "Test"}"#));
    assert_eq!(seen.fetches.len(), 1);
    assert_eq!(seen.fetches[0].method, Method::Get);
    assert_eq!(seen.fetches[0].url, format!("{base}/lookup/movie/603"));
    assert_eq!(
        out,
        Outcome::Done(vec![PluginStream {
            url: format!("{base}/media/test-film.webm"),
            name: Some("Test film".into()),
            title: Some("Test (test film)".into()),
            quality: Some("240p".into()),
            ..Default::default()
        }])
    );
    // A lookup the host answers 404: no rows.
    let missing = |r: &FetchRequest| {
        HostReply::Response(FetchResponse {
            status: 404,
            status_text: "Not Found".into(),
            url: r.url.clone(),
            headers: Vec::new(),
            body: Vec::new(),
        })
    };
    assert_eq!(
        run_with(movie(&script), missing).0,
        Outcome::Done(Vec::new())
    );
}

/// The acceptance walk's provider (tests/fixtures/walk-repo): the walks play
/// a real legal stream, Big Buck Bunny (Blender Foundation, CC BY 3.0) from
/// archive.org. Here archive.org's metadata answer is recorded, so the test
/// stays off the network: one row for Big Buck Bunny, and no request for a
/// title it does not know.
#[test]
fn the_walk_provider_finds_its_film() {
    let script = include_str!("fixtures/walk-repo/open-films.js");
    let meta = include_str!("fixtures/walk-repo/archive-metadata.json");
    let mut bunny = movie(script);
    bunny.info = Info {
        imdb_id: Some("tt1254207".into()),
        title: "Big Buck Bunny".into(),
        year: Some(2008),
    };
    let (out, seen) = run_with(bunny, ok(meta));
    assert_eq!(
        seen.fetches[0].url,
        "https://archive.org/metadata/BigBuckBunny_124"
    );
    assert_eq!(
        out,
        Outcome::Done(vec![PluginStream {
            url: "https://archive.org/download/BigBuckBunny_124/Content/big_buck_bunny_720p_surround.mp4".into(),
            name: Some("Open films".into()),
            title: Some("Big Buck Bunny (360p)".into()),
            quality: Some("360p".into()),
            size: Some("59 MB".into()),
            ..Default::default()
        }])
    );
    let (out, seen) = run_with(movie(script), ok(meta));
    assert_eq!(out, Outcome::Done(Vec::new()));
    assert!(seen.fetches.is_empty());
}
