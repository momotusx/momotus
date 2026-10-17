//! Robustness: arbitrary and adversarial text never panics and never takes long.

use mq_relparse::{parse, parse_addon_stream};
use proptest::prelude::*;
use std::time::{Duration, Instant};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn parse_never_panics_on_any_string(s in any::<String>()) {
        let _ = parse(&s);
    }

    #[test]
    fn parse_never_panics_on_release_like_text(
        s in r"[ -~\u{80}-\u{2FF}\u{400}-\u{4FF}\u{3040}-\u{30FF}\u{1F1E6}-\u{1F1FF}\u{1F4BE}\u{1F464}\u{2699}]{0,300}"
    ) {
        let _ = parse(&s);
    }

    #[test]
    fn addon_parse_never_panics(
        name in any::<String>(),
        description in any::<String>(),
        hint in proptest::option::of(any::<String>()),
    ) {
        let _ = parse_addon_stream(&name, &description, hint.as_deref());
    }

    #[test]
    fn token_soup_never_panics(
        tokens in proptest::collection::vec(
            prop_oneof![
                Just("S01E02"), Just("1080p"), Just("x265"), Just("2160p"), Just("[GRP]"), Just("(2019)"),
                Just("DTS-HD.MA"), Just("5.1"), Just("-"), Just("."), Just("_"), Just(" "), Just("720p"),
                Just("Season"), Just("Complete"), Just("1-12"), Just("vol"), Just("2.0"), Just("mkv"),
                Just("👤 12"), Just("💾 4.2 GB"), Just("[RD+]"), Just("🇬🇧"), Just("é"), Just("Игра"),
            ],
            0..80,
        )
    ) {
        let s = tokens.concat();
        let _ = parse(&s);
        let _ = parse_addon_stream(&s, &s, Some(&s));
    }
}

#[test]
fn hostile_inputs_finish_quickly() {
    let long = "a".repeat(200_000);
    let cases = [
        long.clone(),
        "1-".repeat(5_000),
        "S01E01 ".repeat(2_000),
        "(".repeat(4_000),
        "[".repeat(4_000) + &"]".repeat(4_000),
        "2010 ".repeat(1_500),
        "x264 ".repeat(1_500) + "3D",
        "Игра ".repeat(1_500),
        "- ".repeat(3_000),
        "\u{1F1EC}\u{1F1E7}".repeat(3_000),
    ];
    mq_relparse::warm_up();
    for case in &cases {
        let start = Instant::now();
        let _ = parse(case);
        let _ = parse_addon_stream(case, case, Some(case));
        let took = start.elapsed();
        let head: String = case.chars().take(12).collect();
        println!("{took:>10.2?}  {head:?}");
        assert!(
            took < Duration::from_secs(5),
            "took {took:?} on input starting {head:?}"
        );
    }
}
