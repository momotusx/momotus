//! The title page (`/mov/<digits>/<slug>.html`): one list of every subtitle
//! of a title, in sections (Türkçe Altyazılar, İngilizce Altyazılar, İşitme
//! Engelliler İçin), one row per subtitle (plans/m13-turkcealtyazi.md 2.2).

use regex::Regex;
use std::sync::LazyLock;

/// What a title page holds.
#[derive(Debug, Clone, PartialEq)]
pub enum TitlePage {
    /// No page for this IMDb id (the site answers with `404-error.html`),
    /// or a page whose list is empty.
    Nothing,
    /// The site answered with another title's page: nothing for this one,
    /// worth a log line.
    OtherTitle,
    /// The rows that parsed, in page order, and how many subtitle rows did
    /// not (at most a quarter; worth a log line, the others still list).
    Rows { rows: Vec<Row>, unparsed: usize },
}

/// The page is not what the site used to send: say so, never "nothing".
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("TurkceAltyazi.org changed its pages: {0}")]
pub struct Changed(pub &'static str);

/// One subtitle in the list.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub altid: u32,
    /// The subtitle page, checked: `/sub/<altid>/<slug>.html`.
    pub path: String,
    pub lang: Lang,
    pub kind: Kind,
    /// The row's release line (`.ripdiv`): a release, a group or, for a
    /// special, its title. Plain text, at most 80 characters.
    pub release: String,
    pub fps: Option<f32>,
    pub downloads: u32,
    /// The translators, joined with " & ".
    pub translator: Option<String>,
    pub hearing_impaired: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Turkish,
    English,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// A movie in this many parts.
    Movie {
        discs: u32,
    },
    Episode {
        season: u32,
        episode: u32,
    },
    /// Several episodes in one archive: a list (`20,21,22`) or a range
    /// (`03-06`).
    Episodes {
        season: u32,
        episodes: Vec<u32>,
    },
    Pack {
        season: u32,
    },
    /// `Özel` or `Özel Bölüm`: an extra with no episode number.
    Special {
        season: u32,
    },
}

const ORIGIN_MOV: &str = "https://turkcealtyazi.org/mov/";
const WRAPPER: &str = r#"<div class="altyazi-list-wrapper">"#;
/// Where the list ends: the next block of the page ("Bu diziyi sevenler",
/// "Forumdan Benzer Başlıklar").
const AFTER_LIST: &str = r#"<div class="nm-block"#;
const ROW: &str = r#"<div class="altsonsez"#;
const RELEASE_MAX: usize = 80;

macro_rules! re {
    ($name:ident, $pattern:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pattern).expect("pattern"));
    };
}

re!(
    FINAL_URL,
    r"^https://turkcealtyazi\.org/mov/(\d{1,9})/([a-z0-9-]{1,120})\.html$"
);
re!(ANCHOR, r#"href="/sub/"#);
// A row's language cell; a section header's says "Dil" instead.
re!(LANGUAGE_CELL, r#"<div class="aldil">\s*<span"#);
re!(LINK, r#"href="(/sub/(\d{1,9})/[a-z0-9-]{1,120}\.html)""#);
re!(
    FLAG,
    r#"<div class="aldil">\s*<span class="flag([a-z]+)"></span>"#
);
re!(MARKER, r#"(?s)<div class="alcd">(.*?)</div>"#);
re!(
    FPS,
    r#"<div class="alfps">\s*([0-9]{1,3}(?:[.,][0-9]{1,3})?)\s*</div>"#
);
re!(
    DOWNLOADS,
    r#"<div class="alindirme">\s*([0-9.,]{1,13})\s*</div>"#
);
re!(TRANSLATORS, r#"(?s)<div class="alcevirmen">(.*?)</div>"#);
re!(STRONG, r"<strong>([^<]*)</strong>");
re!(RIPDIV, r#"(?s)<div class="ripdiv">(.*?)</div>"#);
re!(TAG, r"(?s)<[^>]*>");
re!(MOVIE, r"^(\d{1,2})$");
re!(PACK, r"^S<b>(\d{1,3})</b> <b>(?i:paket)</b>$");
re!(
    EPISODES,
    r#"^S<b>(\d{1,3})</b> <span class="cred">\|</span> E<b>([^<]{1,60})</b>$"#
);
re!(ONE, r"^\d{1,4}$");
re!(LIST, r"^\d{1,4}(?:,\d{1,4})+$");
re!(RANGE, r"^(\d{1,4})-(\d{1,4})$");
re!(SPECIAL, r"^Özel(?: Bölüm)?$");

/// Reads the title page that `final_url` answered for IMDb `digits` (as
/// the site writes them, `0903747` for tt0903747). `final_url` is where the
/// request ended after redirects: anything but a title page on the site's
/// own origin is a change, and `404-error.html` means the site has no such
/// title. Rows are counted three ways (the language cell, the subtitle
/// link and the row's opening tag), so renaming any one of them still
/// counts the rows; more than a quarter of them unread is a change.
pub fn title_page(final_url: &str, digits: &str, html: &str) -> Result<TitlePage, Changed> {
    let Some(url) = FINAL_URL.captures(final_url) else {
        return Err(Changed(if final_url.starts_with(ORIGIN_MOV) {
            "the title page has a new address"
        } else {
            "the title page is no longer on the site"
        }));
    };
    if &url[1] != digits {
        return Ok(TitlePage::OtherTitle);
    }
    if &url[2] == "404-error" {
        return Ok(TitlePage::Nothing);
    }
    let Some(start) = html.find(WRAPPER) else {
        return Err(Changed("the subtitle list is gone"));
    };
    let list = &html[start..];
    let list = &list[..list.find(AFTER_LIST).unwrap_or(list.len())];
    let starts: Vec<usize> = list.match_indices(ROW).map(|(at, _)| at).collect();
    let total = LANGUAGE_CELL
        .find_iter(list)
        .count()
        .max(ANCHOR.find_iter(list).count())
        .max(starts.len());
    if total == 0 {
        return Ok(TitlePage::Nothing);
    }
    let rows: Vec<Row> = starts
        .iter()
        .enumerate()
        .filter_map(|(i, &at)| row(&list[at..starts.get(i + 1).copied().unwrap_or(list.len())]))
        .collect();
    let unparsed = total.saturating_sub(rows.len());
    if rows.is_empty() || unparsed * 4 > total {
        return Err(Changed("most subtitle rows cannot be read"));
    }
    Ok(TitlePage::Rows { rows, unparsed })
}

/// One row, from its opening `<div class="altsonsez…">` to the next one.
/// A section header that follows a row has column titles, not values, so
/// none of the patterns below can take one for the row's.
fn row(own: &str) -> Option<Row> {
    let link = LINK.captures(own)?;
    let lang = match &FLAG.captures(own)?[1] {
        "tr" => Lang::Turkish,
        "en" => Lang::English,
        _ => return None,
    };
    let kind = kind(&collapse(&MARKER.captures(own)?[1]))?;
    let fps = FPS
        .captures(own)
        .and_then(|c| c[1].replace(',', ".").parse().ok());
    let downloads = DOWNLOADS
        .captures(own)
        .and_then(|c| c[1].replace([',', '.'], "").parse().ok())
        .unwrap_or(0);
    let translators: Vec<String> = TRANSLATORS
        .captures(own)
        .map(|c| {
            STRONG
                .captures_iter(&c[1])
                .map(|s| text(&s[1]))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let ripdiv = RIPDIV
        .captures(own)
        .map(|c| c[1].to_owned())
        .unwrap_or_default();
    let mut release = text(&TAG.replace_all(&ripdiv, " "));
    if let Some((cut, _)) = release.char_indices().nth(RELEASE_MAX) {
        release.truncate(cut);
    }
    Some(Row {
        altid: link[2].parse().ok()?,
        path: link[1].to_owned(),
        lang,
        kind,
        release,
        fps,
        downloads,
        translator: (!translators.is_empty()).then(|| translators.join(" & ")),
        // The site marks each such row with this image, in its own
        // section ("İşitme Engelliler İçin") too.
        hearing_impaired: ripdiv.contains("isitme.png"),
    })
}

/// The `.alcd` cell: a disc count, a season pack, or a season with one
/// episode, a list, a range or a special.
fn kind(marker: &str) -> Option<Kind> {
    if let Some(c) = MOVIE.captures(marker) {
        return Some(Kind::Movie {
            discs: c[1].parse().ok()?,
        });
    }
    if let Some(c) = PACK.captures(marker) {
        return Some(Kind::Pack {
            season: c[1].parse().ok()?,
        });
    }
    let c = EPISODES.captures(marker)?;
    let season = c[1].parse().ok()?;
    let episodes = c[2].trim();
    if SPECIAL.is_match(episodes) {
        return Some(Kind::Special { season });
    }
    if ONE.is_match(episodes) {
        return Some(Kind::Episode {
            season,
            episode: episodes.parse().ok()?,
        });
    }
    if LIST.is_match(episodes) {
        let list = episodes
            .split(',')
            .map(str::parse)
            .collect::<Result<_, _>>();
        return Some(Kind::Episodes {
            season,
            episodes: list.ok()?,
        });
    }
    let r = RANGE.captures(episodes)?;
    let (first, last): (u32, u32) = (r[1].parse().ok()?, r[2].parse().ok()?);
    // A range that runs backwards or past a season's length is not one.
    (first <= last && last - first < 100).then(|| Kind::Episodes {
        season,
        episodes: (first..=last).collect(),
    })
}

/// Whitespace collapsed to single spaces, trimmed.
fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// HTML text as plain text: the common entities decoded, whitespace
/// collapsed.
fn text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let entity = rest
            .find(';')
            .filter(|&end| end <= 10)
            .map(|end| &rest[1..end]);
        let decoded = entity.and_then(|e| match e {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            _ => e
                .strip_prefix('#')
                .and_then(|n| n.parse::<u32>().ok())
                .and_then(char::from_u32),
        });
        match (entity, decoded) {
            (Some(e), Some(c)) => {
                out.push(c);
                rest = &rest[e.len() + 2..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    collapse(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_of_every_shape_seen_on_the_site() {
        let ep = |s: &str| kind(&collapse(s));
        assert_eq!(ep(" 1 "), Some(Kind::Movie { discs: 1 }));
        assert_eq!(
            ep(" S<b>05</b> <b>Paket</b> "),
            Some(Kind::Pack { season: 5 })
        );
        let cred = |e: &str| format!(r#" S<b>09</b> <span class="cred">|</span> E<b>{e}</b> "#);
        assert_eq!(
            ep(&cred("13")),
            Some(Kind::Episode {
                season: 9,
                episode: 13
            })
        );
        assert_eq!(
            ep(&cred("20,21,22")),
            Some(Kind::Episodes {
                season: 9,
                episodes: vec![20, 21, 22]
            })
        );
        assert_eq!(
            ep(&cred("03-06")),
            Some(Kind::Episodes {
                season: 9,
                episodes: vec![3, 4, 5, 6]
            })
        );
        assert_eq!(ep(&cred("Özel Bölüm")), Some(Kind::Special { season: 9 }));
        assert_eq!(ep(&cred("Özel")), Some(Kind::Special { season: 9 }));
        for bad in [
            "Dizi",
            "CD",
            " S<b>05</b> ",
            &cred("06-03"),
            &cred("1-500"),
            &cred("Bonus"),
        ] {
            assert_eq!(ep(bad), None, "{bad}");
        }
    }

    #[test]
    fn text_decodes_entities_and_keeps_stray_ampersands() {
        assert_eq!(
            text(" A &amp; B &#39;x&#39; &lt;1&gt; R&D "),
            "A & B 'x' <1> R&D"
        );
        assert_eq!(text("&nosuchentity; &#xZZ;"), "&nosuchentity; &#xZZ;");
    }
}
