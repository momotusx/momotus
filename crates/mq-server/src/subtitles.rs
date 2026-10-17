//! Subtitles for players: whatever an addon serves (SRT, WebVTT or ASS, in
//! any common charset, gzipped or not) becomes UTF-8 WebVTT, the one format
//! every player here reads with a known MIME type.

use encoding_rs::Encoding;
use std::io::Read;

/// Largest subtitle after unpacking; real ones are well under 1 MB.
const MAX_TEXT: u64 = 16 * 1024 * 1024;

/// UTF-8 WebVTT for `bytes`, or `None` when they are not a subtitle format
/// we know. `lang` (ISO 639-2, as addons give it) picks the legacy charset
/// for files that are neither UTF-8 nor marked with a BOM.
pub fn to_vtt(bytes: &[u8], lang: &str) -> Option<String> {
    let text = decode(&unpack(bytes)?, lang);
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = text.trim_start_matches('\u{feff}').trim_start();
    if text.starts_with("WEBVTT") {
        Some(text.to_owned())
    } else if text.contains("[Events]") || text.contains("[events]") {
        ass(text)
    } else if text.contains("-->") {
        srt(text)
    } else {
        None
    }
}

fn unpack(bytes: &[u8]) -> Option<Vec<u8>> {
    if !bytes.starts_with(&[0x1f, 0x8b]) {
        return Some(bytes.to_vec());
    }
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .take(MAX_TEXT)
        .read_to_end(&mut out)
        .ok()?;
    Some(out)
}

fn decode(bytes: &[u8], lang: &str) -> String {
    if let Some((encoding, bom)) = Encoding::for_bom(bytes) {
        return encoding
            .decode_without_bom_handling(&bytes[bom..])
            .0
            .into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => legacy(lang)
            .decode_without_bom_handling(bytes)
            .0
            .into_owned(),
    }
}

/// The charset a subtitle in `lang` most likely uses when it is not UTF-8:
/// the Windows code page of that script, as subtitle tools write them.
fn legacy(lang: &str) -> &'static Encoding {
    use encoding_rs::*;
    match lang.to_ascii_lowercase().as_str() {
        "tur" | "tr" => WINDOWS_1254,
        "gre" | "ell" | "el" => WINDOWS_1253,
        "rus" | "ukr" | "bul" | "bel" | "mac" | "mkd" | "scc" | "ru" | "uk" | "bg" | "be"
        | "mk" => WINDOWS_1251,
        "pol" | "cze" | "ces" | "slo" | "slk" | "hun" | "hrv" | "slv" | "rum" | "ron" | "bos"
        | "srp" | "alb" | "sqi" | "pl" | "cs" | "sk" | "hu" | "hr" | "sl" | "ro" | "bs" | "sr"
        | "sq" => WINDOWS_1250,
        "ara" | "per" | "fas" | "urd" | "ar" | "fa" | "ur" => WINDOWS_1256,
        "heb" | "he" => WINDOWS_1255,
        "est" | "lav" | "lit" | "et" | "lv" | "lt" => WINDOWS_1257,
        "vie" | "vi" => WINDOWS_1258,
        "tha" | "th" => WINDOWS_874,
        "chi" | "zho" | "zh" => GB18030,
        "zht" | "zhtw" => BIG5,
        "jpn" | "ja" => SHIFT_JIS,
        "kor" | "ko" => EUC_KR,
        _ => WINDOWS_1252,
    }
}

fn srt(text: &str) -> Option<String> {
    let mut out = String::from("WEBVTT\n\n");
    let mut cues = 0;
    for line in text.lines() {
        if let Some((start, end)) = line.split_once("-->") {
            // Coordinates after the end time are dropped.
            let end = end.split_whitespace().next().unwrap_or_default();
            let (Some(start), Some(end)) = (stamp(start), stamp(end)) else {
                continue;
            };
            out.push_str(&format!("{start} --> {end}\n"));
            cues += 1;
        } else {
            out.push_str(&without_ass_tags(line));
            out.push('\n');
        }
    }
    (cues > 0).then_some(out)
}

fn ass(text: &str) -> Option<String> {
    let mut format: Option<Vec<String>> = None;
    let mut events = false;
    let mut cues: Vec<(u64, u64, String)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            events = line.eq_ignore_ascii_case("[events]");
            continue;
        }
        if !events {
            continue;
        }
        if let Some(fields) = line.strip_prefix("Format:") {
            format = Some(
                fields
                    .split(',')
                    .map(|f| f.trim().to_ascii_lowercase())
                    .collect(),
            );
            continue;
        }
        let (Some(dialogue), Some(format)) = (line.strip_prefix("Dialogue:"), &format) else {
            continue;
        };
        // The text is the last field and may itself hold commas.
        let parts: Vec<&str> = dialogue.splitn(format.len(), ',').collect();
        let field = |name: &str| {
            format
                .iter()
                .position(|f| f == name)
                .and_then(|i| parts.get(i))
                .map(|p| p.trim())
        };
        let (Some(start), Some(end), Some(body)) = (field("start"), field("end"), field("text"))
        else {
            continue;
        };
        let (Some(start), Some(end)) = (millis(start), millis(end)) else {
            continue;
        };
        let body = without_ass_tags(body)
            .replace("\\N", "\n")
            .replace("\\n", "\n")
            .replace("\\h", " ");
        if !body.trim().is_empty() && end > start {
            cues.push((start, end, body.trim().to_owned()));
        }
    }
    if cues.is_empty() {
        return None;
    }
    cues.sort_by_key(|(start, _, _)| *start);
    let mut out = String::from("WEBVTT\n");
    for (start, end, body) in cues {
        // A blank line inside a cue would end it early.
        let body = body
            .lines()
            .filter(|l| !l.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        out.push_str(&format!(
            "\n{} --> {}\n{body}\n",
            format_ms(start),
            format_ms(end)
        ));
    }
    Some(out)
}

/// `{...}` override blocks (ASS styling, also found in SRT files as `{\an8}`).
fn without_ass_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0;
    for c in text.chars() {
        match c {
            '{' => depth += 1,
            '}' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// "0:00:01,5", "00:01.250" or "1:02:03.45" as milliseconds.
fn millis(text: &str) -> Option<u64> {
    let text = text.trim();
    let (clock, fraction) = match text.rfind([',', '.']) {
        Some(i) => (&text[..i], &text[i + 1..]),
        None => (text, ""),
    };
    let mut seconds = 0u64;
    for part in clock.split(':') {
        let n: u64 = part.trim().parse().ok()?;
        seconds = seconds.checked_mul(60)?.checked_add(n)?;
    }
    if fraction.is_empty() {
        return seconds.checked_mul(1000);
    }
    if !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // Centiseconds in ASS, milliseconds in SRT: read as a decimal fraction.
    let digits: String = fraction.chars().chain("000".chars()).take(3).collect();
    seconds.checked_mul(1000)?.checked_add(digits.parse().ok()?)
}

fn stamp(text: &str) -> Option<String> {
    millis(text).map(format_ms)
}

fn format_ms(ms: u64) -> String {
    let (h, m, s, f) = (ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000);
    format!("{h:02}:{m:02}:{s:02}.{f:03}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn srt_in_a_turkish_code_page_becomes_utf8_webvtt() {
        let srt = "1\r\n0:00:01,500 --> 0:00:03,000 X1:10\r\n{\\an8}<i>Çağrı geldi</i>\r\n\r\n2\r\n00:00:04,000 --> 00:00:05,250\r\nŞimdi\r\n";
        let (bytes, _, _) = encoding_rs::WINDOWS_1254.encode(srt);
        let vtt = to_vtt(&bytes, "tur").unwrap();
        assert!(
            vtt.starts_with("WEBVTT\n\n1\n00:00:01.500 --> 00:00:03.000\n"),
            "{vtt}"
        );
        assert!(vtt.contains("<i>Çağrı geldi</i>\n"), "{vtt}");
        assert!(
            vtt.contains("00:00:04.000 --> 00:00:05.250\nŞimdi"),
            "{vtt}"
        );
    }

    #[test]
    fn utf8_is_kept_whatever_the_language_says() {
        let vtt = to_vtt(
            "1\n00:00:01,000 --> 00:00:02,000\nПривет\n".as_bytes(),
            "tur",
        )
        .unwrap();
        assert!(vtt.contains("Привет"));
        let (bytes, _, _) =
            encoding_rs::WINDOWS_1251.encode("1\n00:00:01,000 --> 00:00:02,000\nПривет\n");
        assert!(to_vtt(&bytes, "rus").unwrap().contains("Привет"));
    }

    #[test]
    fn utf16_with_a_bom_and_gzip_are_unpacked() {
        let srt = "1\n00:00:01,000 --> 00:00:02,000\nhello\n";
        let mut utf16 = vec![0xff, 0xfe];
        utf16.extend(srt.encode_utf16().flat_map(u16::to_le_bytes));
        assert!(to_vtt(&utf16, "eng").unwrap().contains("hello"));
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(srt.as_bytes()).unwrap();
        assert!(
            to_vtt(&gz.finish().unwrap(), "eng")
                .unwrap()
                .contains("hello")
        );
    }

    #[test]
    fn ass_dialogue_becomes_cues_in_time_order() {
        let ass = "[Script Info]\nTitle: x\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:05.20,0:00:07.00,Default,,0,0,0,,Second, with a comma\nDialogue: 0,0:00:01.00,0:00:02.50,Default,,0,0,0,,{\\i1}First{\\i0}\\Nline two\n";
        let vtt = to_vtt(ass.as_bytes(), "eng").unwrap();
        assert_eq!(
            vtt,
            "WEBVTT\n\n00:00:01.000 --> 00:00:02.500\nFirst\nline two\n\n00:00:05.200 --> 00:00:07.000\nSecond, with a comma\n"
        );
    }

    #[test]
    fn webvtt_passes_and_unknown_formats_are_refused() {
        let vtt = "\u{feff}WEBVTT\n\n00:01.000 --> 00:02.000\nhi\n";
        assert_eq!(
            to_vtt(vtt.as_bytes(), "eng").unwrap(),
            vtt.trim_start_matches('\u{feff}')
        );
        assert_eq!(to_vtt(b"{0}{25}microdvd", "eng"), None);
        assert_eq!(to_vtt(b"", "eng"), None);
        assert_eq!(millis("1:02:03.45"), Some(3_723_450));
        assert_eq!(millis("bad"), None);
    }
}
