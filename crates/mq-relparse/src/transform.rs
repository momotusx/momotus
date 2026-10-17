//! Value transformers applied to a rule's matched text (PTT `transformers.py`).

use crate::date::date_value;

/// Largest numeric range a pattern may expand into (bounds work on hostile input).
const MAX_RANGE: u64 = 10_000;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Val {
    Bool,
    Str(String),
    Int(u32),
    Strs(Vec<String>),
    Ints(Vec<u32>),
}

pub(crate) enum Tf {
    /// Matched text as is.
    Raw,
    /// A fixed value; `$1` is replaced by the matched text.
    Val(&'static str),
    Lower,
    Upper,
    True,
    Int,
    FirstInt,
    Range,
    RangeXofY,
    Resolution,
    IntArr,
    /// Append a fixed value to a list when not already present.
    Uniq(&'static str),
    /// Like `Uniq`, with the value chosen from the raw matched text.
    UniqFn(fn(&str) -> &'static str),
    Date(&'static [&'static str]),
}

impl Tf {
    /// The list element a `Uniq` transformer contributes for this raw match.
    pub fn uniq_label(&self, raw: &str) -> Option<&'static str> {
        match self {
            Tf::Uniq(v) => Some(v),
            Tf::UniqFn(f) => Some(f(raw)),
            _ => None,
        }
    }

    pub fn apply(&self, input: &str) -> Option<Val> {
        match self {
            Tf::Raw => Some(Val::Str(input.trim().to_string())),
            Tf::Val(v) => Some(Val::Str(v.replace("$1", input).trim().to_string())),
            Tf::Lower => Some(Val::Str(input.to_lowercase().trim().to_string())),
            Tf::Upper => Some(Val::Str(input.to_uppercase().trim().to_string())),
            Tf::True => Some(Val::Bool),
            Tf::Int => integer(input).map(Val::Int),
            Tf::FirstInt => numbers(input)
                .first()
                .copied()
                .and_then(to_u32)
                .map(Val::Int),
            Tf::Range => range(input).map(Val::Ints),
            Tf::RangeXofY => range_x_of_y(input).map(Val::Ints),
            Tf::Resolution => Some(Val::Str(resolution(input))),
            Tf::IntArr => integer(input).map(|n| Val::Ints(vec![n])),
            Tf::Uniq(_) | Tf::UniqFn(_) => None,
            Tf::Date(fmts) => date_value(fmts, input).map(Val::Str),
        }
    }
}

fn to_u32(n: u64) -> Option<u32> {
    u32::try_from(n).ok()
}

/// All ASCII digit runs, as numbers (runs too long for u64 are skipped as 0 would mislead).
fn numbers(s: &str) -> Vec<u64> {
    s.split(|c: char| !c.is_ascii_digit())
        .filter(|t| !t.is_empty())
        .map(|t| t.parse::<u64>().unwrap_or(u64::MAX))
        .collect()
}

fn integer(s: &str) -> Option<u32> {
    let digits: String = s.chars().filter(char::is_ascii_digit).collect();
    digits.parse::<u32>().ok()
}

fn span(a: u64, b: u64) -> Option<Vec<u32>> {
    if b < a || b - a >= MAX_RANGE || b > u64::from(u32::MAX) {
        return None;
    }
    Some((a..=b).map(|n| n as u32).collect())
}

fn range(s: &str) -> Option<Vec<u32>> {
    let n = numbers(s);
    match n.as_slice() {
        [a, b] if a < b => span(*a, *b),
        [a] => to_u32(*a).map(|v| vec![v]),
        all if all.len() > 2 && all.windows(2).all(|w| w[0].checked_add(1) == Some(w[1])) => {
            all.iter().map(|v| to_u32(*v)).collect()
        }
        _ => None,
    }
}

fn range_x_of_y(s: &str) -> Option<Vec<u32>> {
    match numbers(s).as_slice() {
        [n] => span(1, *n),
        _ => None,
    }
}

fn resolution(s: &str) -> String {
    let l = s.to_lowercase();
    let has = |t: &str| l.contains(t);
    let tag = if has("2160") || has("4k") {
        "2160p"
    } else if has("1440") || has("2k") {
        "1440p"
    } else if has("1080") {
        "1080p"
    } else if has("720") {
        "720p"
    } else if has("480") {
        "480p"
    } else if has("360") {
        "360p"
    } else if has("240") {
        "240p"
    } else {
        return l;
    };
    tag.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_follow_ptt() {
        assert_eq!(range("1-3"), Some(vec![1, 2, 3]));
        assert_eq!(range("5"), Some(vec![5]));
        assert_eq!(range("1 2 3"), Some(vec![1, 2, 3]));
        assert_eq!(range("1 3 4"), None);
        assert_eq!(range("3-1"), None);
        assert_eq!(range("1-99999999999"), None);
    }

    #[test]
    fn x_of_y_counts_from_one() {
        assert_eq!(range_x_of_y("3 of 9"), None);
        assert_eq!(range_x_of_y("3"), Some(vec![1, 2, 3]));
    }
}
