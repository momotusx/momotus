//! Strict date validation matching what PTT gets from `arrow.get(text, fmt)`.

const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

#[derive(Clone, Copy)]
enum Tok {
    Year4,
    Year2,
    Two,
    Mon,
    MonFull,
    Ordinal,
}

/// ISO date for the first format that accepts the text, `None` when no format does.
pub(crate) fn date_value(fmts: &[&str], input: &str) -> Option<String> {
    let text = sanitize(input);
    fmts.iter().find_map(|f| parse(f, &text))
}

fn sanitize(s: &str) -> String {
    let spaced: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                ' '
            }
        })
        .collect();
    spaced
        .split_whitespace()
        .map(shorten_month)
        .collect::<Vec<_>>()
        .join(" ")
}

fn shorten_month(w: &str) -> &str {
    match w.to_ascii_lowercase().as_str() {
        "june" => "Jun",
        "july" => "Jul",
        "sept" => "Sep",
        _ => w,
    }
}

fn parse(fmt: &str, text: &str) -> Option<String> {
    let toks: Vec<&str> = text.split(' ').collect();
    if fmt == "YYYYMMDD" {
        let [t] = toks.as_slice() else { return None };
        if t.len() != 8 {
            return None;
        }
        return ymd(digits(t, 4, 0)?, digits(t, 2, 4)?, digits(t, 2, 6)?);
    }
    let tags: Vec<&str> = fmt.split(' ').collect();
    if tags.len() != toks.len() {
        return None;
    }
    let (mut y, mut m, mut d) = (None, None, None);
    for (tag, tok) in tags.iter().zip(&toks) {
        let v = field(kind(tag)?, tok)?;
        match *tag {
            "YYYY" | "YY" => y = Some(v),
            "MM" | "MMM" | "MMMM" => m = Some(v),
            _ => d = Some(v),
        }
    }
    ymd(y?, m?, d?)
}

fn kind(tag: &str) -> Option<Tok> {
    match tag {
        "YYYY" => Some(Tok::Year4),
        "YY" => Some(Tok::Year2),
        "MM" | "DD" => Some(Tok::Two),
        "MMM" => Some(Tok::Mon),
        "MMMM" => Some(Tok::MonFull),
        "Do" => Some(Tok::Ordinal),
        _ => None,
    }
}

fn field(kind: Tok, tok: &str) -> Option<u32> {
    match kind {
        Tok::Year4 if tok.len() == 4 => digits(tok, 4, 0),
        Tok::Year2 if tok.len() == 2 => {
            digits(tok, 2, 0).map(|v| if v > 68 { 1900 + v } else { 2000 + v })
        }
        Tok::Two if tok.len() == 2 => digits(tok, 2, 0),
        Tok::Mon => month(tok, true),
        Tok::MonFull => month(tok, false),
        Tok::Ordinal => ordinal(tok),
        _ => None,
    }
}

fn digits(s: &str, len: usize, at: usize) -> Option<u32> {
    let part = s.get(at..at + len)?;
    if !part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    part.parse().ok()
}

fn month(tok: &str, short: bool) -> Option<u32> {
    let l = tok.to_ascii_lowercase();
    let pos = MONTHS
        .iter()
        .position(|m| if short { m[..3] == l } else { *m == l })?;
    Some(pos as u32 + 1)
}

fn ordinal(tok: &str) -> Option<u32> {
    let l = tok.to_ascii_lowercase();
    let cut = l.find(|c: char| !c.is_ascii_digit())?;
    let (num, suffix) = l.split_at(cut);
    let n: u32 = num.parse().ok()?;
    let want = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    (suffix == want && (1..=31).contains(&n)).then_some(n)
}

fn ymd(y: u32, m: u32, d: u32) -> Option<String> {
    let leap = (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400);
    let dim = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    ((1..=dim).contains(&d) && y >= 1).then(|| format!("{y:04}-{m:02}-{d:02}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_like_arrow() {
        assert_eq!(
            date_value(&["YYYY MM DD"], "2015.12.31").as_deref(),
            Some("2015-12-31")
        );
        assert_eq!(date_value(&["YYYY MM DD"], "2015.02.30"), None);
        assert_eq!(
            date_value(&["DD MMM YYYY"], "05 Feb 2015").as_deref(),
            Some("2015-02-05")
        );
        assert_eq!(date_value(&["DD MMM YYYY"], "5 Feb 2015"), None);
        assert_eq!(
            date_value(&["Do MMMM YYYY"], "5th February 2015").as_deref(),
            Some("2015-02-05")
        );
        assert_eq!(
            date_value(&["YY MM DD"], "99 12 31").as_deref(),
            Some("1999-12-31")
        );
    }
}
