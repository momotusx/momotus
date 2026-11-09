use super::*;
use proptest::prelude::*;

#[derive(Deserialize)]
struct Vectors {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    rule: String,
    #[serde(rename = "in")]
    input: String,
    out: String,
}

fn vectors() -> Vec<Case> {
    let text = include_str!("../../../apps/shared/report-scrub-vectors.json");
    serde_json::from_str::<Vectors>(text)
        .expect("the vector file")
        .cases
}

#[test]
fn every_vector_scrubs_as_written() {
    let failed: Vec<String> = vectors()
        .iter()
        .filter_map(|c| {
            let got = scrub(&c.input);
            (got != c.out)
                .then(|| format!("{}: {:?} gave {:?}, want {:?}", c.rule, c.input, got, c.out))
        })
        .collect();
    assert!(failed.is_empty(), "{failed:#?}");
}

#[test]
fn every_vector_is_a_fixed_point() {
    for c in vectors() {
        assert_eq!(scrub(&c.out), c.out, "{}", c.rule);
    }
}

fn secret() -> impl Strategy<Value = String> {
    // A key shaped as the debrid stores' are: letters and digits.
    "[A-Z]{8}[0-9]{4}[a-z]{8}[0-9]{4}"
}

proptest! {
    #[test]
    fn scrub_is_idempotent(text in "[ -~\n“”‘’]{0,200}") {
        let once = scrub(&text);
        prop_assert_eq!(scrub(&once), once);
    }

    /// Secrets planted in the shapes real error lines carry them never come
    /// out of the scrubber.
    #[test]
    fn planted_secrets_never_survive(
        key in secret(),
        user in "[a-z]{4,10}",
        title in "[A-Z][a-z]{3,8} [A-Z][a-z]{3,8}",
        year in 1950u32..2030,
        id in 1000u32..9_999_999,
        octets in proptest::array::uniform4(1u8..255),
        hash in "[0-9a-f]{40}",
        which in 0usize..12,
    ) {
        let line = match which {
            0 => format!("request failed: https://torrentio.strem.fun/realdebrid={key}/manifest.json"),
            1 => format!("request failed: torrentio.strem.fun/alldebrid={key}/stream/movie/tt{id}.json"),
            2 => format!("open /Users/{user}/Movies/{title} ({year})/{title}.mkv"),
            3 => format!("no row for \"{title}\""),
            4 => format!("no row for {title} ({year})"),
            5 => format!("peer {}.{}.{}.{} refused", octets[0], octets[1], octets[2], octets[3]),
            6 => format!("torrent {hash} stalled"),
            7 => format!("mail to {user}@example.com bounced"),
            8 => format!("apikey={key} refused"),
            9 => format!("debrid {key} refused"),
            10 => format!("C:\\Users\\{user}\\AppData\\x.txt locked"),
            _ => format!("{}.{year}.1080p.WEB-DL missing", title.replace(' ', ".")),
        };
        let out = scrub(&line);
        prop_assert!(!out.contains(&key), "{} kept the key: {}", line, out);
        prop_assert!(!out.contains(&format!("/{user}")), "{} kept the user: {}", line, out);
        prop_assert!(!out.contains(&format!("{user}@")), "{} kept the mailbox: {}", line, out);
        prop_assert!(!out.contains(&title), "{} kept the title: {}", line, out);
        prop_assert!(!out.contains(&format!("tt{id}")), "{} kept the title id: {}", line, out);
        prop_assert!(!out.contains(&hash), "{} kept the hash: {}", line, out);
        let ip = format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3]);
        prop_assert!(!out.contains(&ip), "{} kept the address: {}", line, out);
    }
}

#[test]
fn where_takes_compile_time_locations_only() {
    for ok in [
        "crates/mq-runtime/src/views/playback.rs:1340:21",
        "~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tokio-1.47.1/src/runtime/task/harness.rs:12:5",
        "/rustc/abc123/library/core/src/option.rs:2009:5",
    ] {
        assert!(where_ok(ok), "{ok}");
    }
    for bad in [
        "/Users/robin/.cargo/registry/src/x.rs:1:2",
        "/home/robin/x.rs:1",
        "crates/x.rs",
        "C:\\Users\\robin\\x.rs:1",
        "crates/x.rs:1 extra",
    ] {
        assert!(!where_ok(bad), "{bad}");
    }
}

#[test]
fn log_lines_are_callsites_only() {
    assert!(log_ok(
        "warn mq_server::ahead crates/mq-server/src/ahead.rs:212"
    ));
    assert!(log_ok("error mq_runtime crates/mq-runtime/src/core.rs:1"));
    assert!(!log_ok("info mq_server crates/mq-server/src/ahead.rs:212"));
    assert!(!log_ok("warn mq_server read stalled 4000 ms"));
    assert!(!log_ok("warn mq_server /Users/robin/x.rs:1"));
}

fn device() -> Device {
    Device {
        app: App::Android,
        channel: Channel::Release,
        version: "0.14.2".into(),
        build: "d8b973d8".into(),
        core: "d8b973d8".into(),
        os: "Android 15".into(),
        model: "Google Pixel 9 Pro".into(),
        abi: "arm64-v8a".into(),
        form: Form::Phone,
        core_build: CoreBuild::Own,
    }
}

#[test]
fn clean_scrubs_drops_and_groups() {
    let mut r = Report::new(Kind::Panic, 1);
    r.message = Some("no row for \"Dune\" at tt0111161".into());
    r.location = Some("/Users/robin/x.rs:1:2".into());
    r.thread = Some("marquee-core".into());
    r.stack = vec!["open /Users/robin/a".into()];
    r.log = vec![
        "warn mq_server::ahead crates/mq-server/src/ahead.rs:212".into(),
        "warn mq_server read stalled".into(),
    ];
    r.context = Some(Context {
        op: Some("subscribe".into()),
        view: Some("no such view here".into()),
        failure_kind: None,
        class: Some("Library".into()),
    });
    r.attempts = vec![Attempt {
        source: Some("debrid".into()),
        codec: Some("Movie Name 2019".into()),
        ..Attempt::default()
    }];
    r.clean(App::Android);
    assert_eq!(r.message.as_deref(), Some("no row for <text> at <title>"));
    assert_eq!(r.location, None);
    assert_eq!(r.stack, vec!["open <path>".to_owned()]);
    assert_eq!(r.log.len(), 1);
    let c = r.context.as_ref().unwrap();
    assert_eq!(
        (c.op.as_deref(), c.view.as_deref()),
        (Some("subscribe"), None)
    );
    assert_eq!(r.attempts[0].codec, None);
    assert_eq!(r.group.len(), 16);
    let group = r.group.clone();
    r.clean(App::Android);
    assert_eq!(r.group, group, "cleaning twice changes nothing");
}

#[test]
fn incognito_keeps_where_the_fault_is_only() {
    let mut r = Report::new(Kind::Crash, 1);
    r.message = Some("x".into());
    r.location = Some("crates/a.rs:1".into());
    r.stack = vec!["a.b(C.kt:1)".into()];
    r.context = Some(Context::default());
    r.attempts = vec![Attempt::default()];
    r.log = vec!["warn a crates/a.rs:2".into()];
    r.strip_incognito();
    assert_eq!(r.message, None);
    assert_eq!(r.context, None);
    assert!(r.attempts.is_empty() && r.log.is_empty());
    assert_eq!(r.location.as_deref(), Some("crates/a.rs:1"));
    assert_eq!(r.stack.len(), 1);
}

#[test]
fn repeats_differing_in_digits_merge() {
    let mut a = Report::new(Kind::Core, 1);
    a.location = Some("crates/a.rs:1".into());
    a.message = Some("read stalled 4000 ms".into());
    let mut b = a.clone();
    b.message = Some("read stalled 4012 ms".into());
    assert_eq!(a.merge_key(), b.merge_key());
    b.message = Some("write stalled 4012 ms".into());
    assert_ne!(a.merge_key(), b.merge_key());
}

#[test]
fn a_batch_round_trips_through_the_relay_reading() {
    let mut r = Report::new(Kind::Panic, 1);
    r.message = Some("x at https://a.example/b".into());
    let (batch, taken) = Batch::build(device(), &[r]);
    assert_eq!(taken, 1);
    let body = serde_json::to_vec(&batch).unwrap();
    let read = parse_batch(&body).unwrap();
    assert_eq!(read, batch, "the relay's second clean changes nothing");
    assert_eq!(read.reports[0].message.as_deref(), Some("x at <url>"));
}

#[test]
fn the_relay_refuses_what_never_passes_and_waits_on_versions() {
    let (batch, _) = Batch::build(device(), &[Report::new(Kind::Js, 1)]);
    let mut value = serde_json::to_value(&batch).unwrap();
    value["v"] = 2.into();
    assert_eq!(
        parse_batch(&serde_json::to_vec(&value).unwrap()),
        Err(Refusal::BadVersion)
    );
    let mut value = serde_json::to_value(&batch).unwrap();
    value["install"] = "00ff".into();
    assert_eq!(
        parse_batch(&serde_json::to_vec(&value).unwrap()),
        Err(Refusal::BadReport("shape")),
        "no identifier field is taken"
    );
    let mut value = serde_json::to_value(&batch).unwrap();
    value["device"]["model"] = "Robin's iPhone".into();
    assert_eq!(
        parse_batch(&serde_json::to_vec(&value).unwrap()),
        Err(Refusal::BadReport("model"))
    );
    let many: Vec<Report> = (0..MAX_REPORTS + 1)
        .map(|i| Report::new(Kind::Js, i as u64))
        .collect();
    let mut value = serde_json::to_value(&batch).unwrap();
    value["reports"] = serde_json::to_value(many).unwrap();
    assert_eq!(
        parse_batch(&serde_json::to_vec(&value).unwrap()),
        Err(Refusal::BadReport("too many reports"))
    );
}

#[test]
fn a_report_never_passes_its_size() {
    let mut r = Report::new(Kind::Crash, 1);
    r.stack = (0..MAX_STACK)
        .map(|i| format!("frame{i} {}", "x".repeat(400)))
        .collect();
    r.message = Some("y".repeat(5000));
    r.clean(App::Android);
    assert!(serde_json::to_vec(&r).unwrap().len() <= MAX_REPORT_BYTES);
    assert_eq!(r.message.as_ref().unwrap().chars().count(), MAX_MESSAGE);
}
