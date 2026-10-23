//! Handlers that PTT implements as plain functions rather than a regex plus transformer.

use regex::Regex;
use std::sync::LazyLock;

use crate::engine::{Ctx, F, Hit, Matched, Pos, Rule, in_leading_bracket, nb, rule};
use crate::transform::{Tf, Val};

fn re(pat: &str, ci: bool) -> Regex {
    crate::lean::std_regex(pat, ci)
}

fn hit(idx: usize) -> Hit {
    Hit {
        raw_len: 0,
        idx,
        remove: false,
        skip_from_title: false,
    }
}

/// `title[a..b]`, clamped to char boundaries (stale PTT indices may point anywhere).
pub(crate) fn slice(s: &str, a: usize, b: usize) -> &str {
    let b = s.floor_char_boundary(b.min(s.len()));
    let a = s.floor_char_boundary(a.min(b));
    &s[a..b]
}

static SCENE_WEB: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"^.*\b\d{3,4}p\b.*[_. ]WEB(?:[. ](?:[^\WD]|D(?:[^L]|$))|_(?:\W|$))",
        false,
    )
});
static SCENE_GROUP: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"\b(-CAKES|-GGEZ|-GGWP|-GLHF|-GOSSIP|-NAISU|-KOGI|-PECULATE|-SLOT|-EDITH|-ETHEL|-ELEANOR|-B2B|-SPAMnEGGS|-FTP|-DiRT|-SYNCOPY|-BAE|-SuccessfulCrab|-NHTFS|-SURCODE|-B0MBARDIERS)",
        false,
    )
});

/// Scene release flag. Only its match position matters to later handlers.
pub(crate) fn scene(ctx: &mut Ctx) -> Option<Hit> {
    if ctx.has(F::Scene) {
        return None;
    }
    let (idx, raw) = if SCENE_WEB.is_match(&ctx.title) {
        (0, String::new())
    } else {
        let m = SCENE_GROUP.find(&ctx.title)?;
        (m.start(), m.as_str().to_string())
    };
    ctx.remember(F::Scene, &raw, idx);
    ctx.set(F::Scene, Val::Bool);
    Some(Hit {
        skip_from_title: in_leading_bracket(&ctx.title, &raw),
        ..hit(idx)
    })
}

/// Normalise "10-bit" / "10 bit" to "10bit".
pub(crate) fn bit_depth(ctx: &mut Ctx) -> Option<Hit> {
    if let Some(Val::Str(s)) = ctx.get(F::BitDepth) {
        let clean: String = s.chars().filter(|c| *c != ' ' && *c != '-').collect();
        ctx.set(F::BitDepth, Val::Str(clean));
    }
    None
}

static HI10P: LazyLock<Regex> = LazyLock::new(|| re(r"\bhi10p?\b", true));

/// "Hi10P" is a 10-bit H.264 profile: a name with it and no codec token is H.264, so a device that cannot
/// decode it is not offered the row (run after the codec rules).
pub(crate) fn hi10p_codec(ctx: &mut Ctx) -> Option<Hit> {
    if !ctx.has(F::Codec) && HI10P.is_match(&ctx.title) {
        ctx.set(F::Codec, Val::Str("avc".to_string()));
    }
    None
}

static VOLUME: LazyLock<Regex> = LazyLock::new(|| re(r"\bvol(?:ume)?[. -]*(\d{1,2})", true));

pub(crate) fn volumes(ctx: &mut Ctx) -> Option<Hit> {
    let from = ctx.match_idx(F::Year).unwrap_or(0);
    let tail = slice(&ctx.title, from, ctx.title.len());
    let caps = VOLUME.captures(tail)?;
    let m0 = caps.get(0)?;
    let n: u32 = caps.get(1)?.as_str().parse().ok()?;
    let rel = m0.start();
    let len = m0.len();
    let base = ctx.title.len() - tail.len();
    ctx.matched[F::Volumes as usize] = Some(Matched {
        raw: String::new(),
        idx: rel,
    });
    ctx.set(F::Volumes, Val::Ints(vec![n]));
    Some(Hit {
        raw_len: len,
        idx: rel + base,
        remove: true,
        skip_from_title: false,
    })
}

static EP_BEGIN: LazyLock<Rule> = LazyLock::new(|| {
    rule(
        F::Episodes,
        r"(?:[ .]+-[ .]+|[(\[][ .]*)(\d{1,4})(?:a|b|v\d|\.\d)?(?:\W|$)",
        Tf::Raw,
    )
    .ci()
    .nb(r"movie\W*|film\W*|^")
    .na(r"movie|film|\d+")
    .look(nb(r"\[(?:480|720|1080)\]").at(Pos::End(0)))
});
static EP_MIDDLE: LazyLock<Rule> = LazyLock::new(|| {
    rule(
        F::Episodes,
        r"^(?:[(\[-][ .]?)?(\d{1,4})(?:a|b|v\d)?(?:\W|$)",
        Tf::Raw,
    )
    .ci()
    .na(r"movie|film")
    .na(r"\[(?:480|720|1080)\]")
});

/// Bare episode numbers such as "Show - 05 [720p]" found before the release tags.
pub(crate) fn episodes(ctx: &mut Ctx) -> Option<Hit> {
    if ctx.has(F::Episodes) {
        return None;
    }
    let nonzero = |f: F| ctx.match_idx(f).filter(|i| *i != 0);
    let start = [F::Year, F::Seasons]
        .into_iter()
        .filter_map(nonzero)
        .min()
        .unwrap_or(0);
    let end = [F::Resolution, F::Quality, F::Codec, F::Audio]
        .into_iter()
        .filter_map(nonzero)
        .min()
        .map_or(ctx.title.len(), |e| e.min(ctx.title.len()));
    let begin = slice(&ctx.title, 0, end);
    let middle = slice(&ctx.title, start, end);
    let (digits, whole) = match EP_BEGIN.find(begin) {
        Some(f) => (f.group(begin, 1)?.to_string(), f.raw(begin).to_string()),
        None => {
            let f = EP_MIDDLE.find(middle)?;
            (f.group(middle, 1)?.to_string(), f.raw(middle).to_string())
        }
    };
    let n: u32 = digits.parse().ok()?;
    ctx.set(F::Episodes, Val::Ints(vec![n]));
    Some(hit(ctx.title.find(&whole).unwrap_or(0)))
}

static ANIME: LazyLock<Regex> = LazyLock::new(|| re(r"One.*?Piece|Bleach|Naruto", false));
static BARE_NUMBER: LazyLock<Regex> = LazyLock::new(|| re(r"\b\d{1,4}\b", false));

/// One Piece, Bleach and Naruto releases carry just the episode number.
pub(crate) fn anime_eps(ctx: &mut Ctx) -> Option<Hit> {
    if !ANIME.is_match(&ctx.title) {
        return None;
    }
    if matches!(ctx.get(F::Episodes), Some(Val::Ints(v)) if !v.is_empty()) {
        return None;
    }
    let m = BARE_NUMBER.find(&ctx.title)?;
    let n: u32 = m.as_str().parse().ok()?;
    let idx = ctx.title.find(m.as_str()).unwrap_or(0);
    ctx.set(F::Episodes, Val::Ints(vec![n]));
    Some(hit(idx))
}

static PT_EPISODE: LazyLock<Regex> = LazyLock::new(|| re(r"capitulo|ao", true));
static DUBLADO: LazyLock<Regex> = LazyLock::new(|| re(r"dublado", true));

/// Portuguese episode naming ("capitulo", "ao") or "dublado" implies Portuguese audio.
pub(crate) fn infer_language(ctx: &mut Ctx) -> Option<Hit> {
    let has_pt_es = matches!(
        ctx.get(F::Languages),
        Some(Val::Strs(v)) if v.iter().any(|l| l == "pt" || l == "es")
    );
    if has_pt_es {
        return None;
    }
    let ep_hint = ctx.matched[F::Episodes as usize]
        .as_ref()
        .is_some_and(|m| PT_EPISODE.is_match(&m.raw));
    if ep_hint || DUBLADO.is_match(&ctx.title) {
        let mut langs = match ctx.get(F::Languages) {
            Some(Val::Strs(v)) => v.clone(),
            _ => Vec::new(),
        };
        langs.push("pt".to_string());
        ctx.set(F::Languages, Val::Strs(langs));
    }
    None
}

/// A `[Group]` that overlaps another match was not a group after all.
pub(crate) fn group_bracket(ctx: &mut Ctx) -> Option<Hit> {
    let g = ctx.matched[F::Group as usize].as_ref()?;
    if !(g.raw.starts_with('[') && g.raw.ends_with(']')) {
        return None;
    }
    let end = g.idx + g.raw.len();
    let overlaps = ctx
        .matched
        .iter()
        .enumerate()
        .any(|(i, m)| i != F::Group as usize && m.as_ref().is_some_and(|m| m.idx < end));
    if overlaps && ctx.has(F::Group) {
        ctx.unset(F::Group);
    }
    None
}

pub(crate) fn group_exclusion(ctx: &mut Ctx) -> Option<Hit> {
    if matches!(ctx.get(F::Group), Some(Val::Str(s)) if s == "-" || s.is_empty()) {
        ctx.unset(F::Group);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_never_splits_a_char() {
        let s = "aé";
        assert_eq!(slice(s, 0, 2), "a");
        assert_eq!(slice(s, 2, 1), "");
        assert_eq!(slice(s, 1, 99), "é");
    }
}
