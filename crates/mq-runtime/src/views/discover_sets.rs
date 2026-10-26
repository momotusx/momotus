//! Discover's complete sets (a service's list, the union of the services'
//! lists): what each item says of itself, and the filters and orders the
//! core applies to them (plans/discover-v2.md section 3). Nothing here asks
//! the network.

use crate::covers::Covers;
use crate::views::home::{Tile, imdb_rating, tile_from_preview};
use chrono::{Datelike, NaiveDate};
use std::cmp::Ordering;

/// A listed title with what the order and the filters read.
#[derive(Clone)]
pub(crate) struct Item {
    pub(crate) tile: Tile,
    imdb: Option<f32>,
    /// `released`, else 1 July of the year `releaseInfo` starts with.
    date: Option<NaiveDate>,
    /// Every genre the item's links give, as written.
    pub(crate) genres: Vec<String>,
}

impl Item {
    pub(crate) fn of(m: mq_protocol::MetaItemPreview, covers: &Covers) -> Self {
        // An addon's own text: "NaN" or "inf" would break the sort's order.
        let imdb = imdb_rating(&m)
            .and_then(|r| r.trim().parse::<f32>().ok())
            .filter(|v| v.is_finite() && (0.0..=10.0).contains(v));
        let date = m.released.map(|d| d.date_naive()).or_else(|| {
            let year: i32 = m.release_info.as_deref()?.get(..4)?.parse().ok()?;
            NaiveDate::from_ymd_opt(year, 7, 1)
        });
        let genres = m
            .links
            .iter()
            .filter(|l| l.category.eq_ignore_ascii_case("Genres"))
            .map(|l| l.name.clone())
            .collect();
        Self {
            tile: tile_from_preview(m, covers),
            imdb,
            date,
            genres,
        }
    }

    pub(crate) fn id(&self) -> &str {
        &self.tile.id
    }

    fn year(&self) -> Option<i32> {
        self.date.map(|d| d.year())
    }

    pub(crate) fn has_genre(&self, genre: &str) -> bool {
        self.genres.iter().any(|g| g.eq_ignore_ascii_case(genre))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Order {
    /// As the list gave them (a service's own order).
    Source,
    Imdb,
    Newest,
    Oldest,
    Az,
}

/// The decades the Year menu offers, newest first.
pub(crate) const DECADES: [&str; 6] = ["2020s", "2010s", "2000s", "1990s", "1980s", "Earlier"];

/// Whether `year` falls in `decade` ("1990s", or "Earlier" for before 1980).
pub(crate) fn in_decade(year: i32, decade: &str) -> bool {
    match decade {
        "Earlier" => year < 1980,
        d => d
            .strip_suffix('s')
            .and_then(|d| d.parse::<i32>().ok())
            .is_some_and(|start| (start..start + 10).contains(&year)),
    }
}

/// Whether a listed tile is in `decade`, read from its year line.
pub(crate) fn tile_in_decade(tile: &Tile, decade: &str) -> bool {
    tile.release_info
        .as_deref()
        .and_then(|r| {
            r.split(|c: char| !c.is_ascii_digit())
                .find(|w| w.len() == 4)
        })
        .and_then(|y| y.parse().ok())
        .is_some_and(|y| in_decade(y, decade))
}

/// The items kept by the genre and the decade, in `order`. Stable: ties
/// keep the source order. Items without the value ordered by go last;
/// Newest leaves out what is dated after `today`.
pub(crate) fn arrange<'a>(
    items: impl IntoIterator<Item = &'a Item>,
    order: Order,
    genre: Option<&str>,
    decade: Option<&str>,
    today: NaiveDate,
) -> Vec<&'a Item> {
    let mut kept: Vec<&Item> = items
        .into_iter()
        .filter(|i| genre.is_none_or(|g| i.has_genre(g)))
        .filter(|i| decade.is_none_or(|d| i.year().is_some_and(|y| in_decade(y, d))))
        .filter(|i| order != Order::Newest || i.date.is_none_or(|d| d <= today))
        .collect();
    let last = |a: Option<Ordering>| a.unwrap_or(Ordering::Equal);
    match order {
        Order::Source => {}
        Order::Imdb => kept.sort_by(|a, b| match (a.imdb, b.imdb) {
            (Some(x), Some(y)) => last(y.partial_cmp(&x)),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }),
        Order::Newest | Order::Oldest => kept.sort_by(|a, b| match (a.date, b.date) {
            (Some(x), Some(y)) if order == Order::Newest => y.cmp(&x),
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }),
        Order::Az => kept.sort_by_cached_key(|i| fold(&i.tile.name)),
    }
    kept
}

/// The genres the items give, each once in its first spelling, by name,
/// with how many items give it.
pub(crate) fn genres_of<'a>(items: impl IntoIterator<Item = &'a Item>) -> Vec<(String, u32)> {
    let mut out: Vec<(String, u32)> = Vec::new();
    for g in items.into_iter().flat_map(|i| &i.genres) {
        match out.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(g)) {
            Some((_, n)) => *n += 1,
            None => out.push((g.clone(), 1)),
        }
    }
    out.sort_by_cached_key(|(n, _)| fold(n));
    out
}

/// A name as A to Z compares it: Unicode lowercase, Latin letters with
/// accents as their base letter (no collation crate in the workspace),
/// leading punctuation skipped ("(500) Days of Summer" under 5, "'Salem's
/// Lot" under S), leading articles kept.
pub(crate) fn fold(name: &str) -> String {
    name.to_lowercase()
        .trim_start_matches(|c: char| !c.is_alphanumeric())
        .replace('ß', "ss")
        .replace('æ', "ae")
        .replace('œ', "oe")
        .chars()
        .map(|c| match c {
            'à'..='å' => 'a',
            'ç' | 'ć' | 'č' => 'c',
            'è'..='ë' | 'ę' | 'ě' => 'e',
            'ì'..='ï' | 'ı' => 'i',
            'ñ' | 'ń' | 'ň' => 'n',
            'ò'..='ö' | 'ø' | 'ő' => 'o',
            'ù'..='ü' | 'ů' | 'ű' => 'u',
            'ý' | 'ÿ' => 'y',
            'ś' | 'š' | 'ş' => 's',
            'ź' | 'ż' | 'ž' => 'z',
            'ğ' => 'g',
            'ł' => 'l',
            'ř' => 'r',
            'ť' => 't',
            'ď' => 'd',
            c => c,
        })
        .collect()
}

/// What a list's genre extra holds, for the menu's name: "Year" when every
/// option is a year (Cinemeta's New), "Period" when they are time spans
/// (Top seeded's "This Week"), else "Genre".
pub(crate) fn genre_label(options: &[String]) -> &'static str {
    let year = |o: &String| o.len() == 4 && o.chars().all(|c| c.is_ascii_digit());
    let period = |o: &String| {
        let o = o.to_lowercase();
        ["week", "month", "year", "today", "yesterday", "all time"]
            .iter()
            .any(|w| o.contains(w))
    };
    if !options.is_empty() && options.iter().all(year) {
        "Year"
    } else if !options.is_empty() && options.iter().all(period) {
        "Period"
    } else {
        "Genre"
    }
}

/// Today, from the system clock (UTC).
pub(crate) fn today() -> NaiveDate {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|d| d.date_naive())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(body: &str) -> Vec<Item> {
        let mq_protocol::ResourceResponse::Metas { metas } =
            mq_protocol::decode_catalog(body.as_bytes()).unwrap()
        else {
            panic!("metas")
        };
        metas
            .into_iter()
            .map(|m| Item::of(m, &Covers::default()))
            .collect()
    }

    fn meta(
        id: &str,
        name: &str,
        imdb: Option<&str>,
        released: Option<&str>,
        genres: &[&str],
    ) -> String {
        let mut links: Vec<String> = genres
            .iter()
            .map(|g| {
                format!(r#"{{"name":"{g}","category":"Genres","url":"stremio:///discover/x"}}"#)
            })
            .collect();
        if let Some(r) = imdb {
            links.push(format!(
                r#"{{"name":"{r}","category":"imdb","url":"https://imdb.com/title/{id}"}}"#
            ));
        }
        let released = released
            .map(|d| format!(r#","released":"{d}T00:00:00.000Z""#))
            .unwrap_or_default();
        format!(
            r#"{{"id":"{id}","type":"movie","name":"{name}","links":[{}]{released}}}"#,
            links.join(",")
        )
    }

    fn set() -> Vec<Item> {
        let metas = [
            meta(
                "tt1",
                "Fargo",
                Some("8.9"),
                Some("1996-03-08"),
                &["Crime", "Drama"],
            ),
            meta("tt2", "Élite", None, Some("2018-10-05"), &["Drama"]),
            meta("tt3", "Dune", Some("8.0"), Some("2021-10-22"), &["Sci-Fi"]),
            meta(
                "tt4",
                "Tomorrow",
                Some("9.9"),
                Some("2999-01-01"),
                &["Drama"],
            ),
            meta("tt5", "Arrival", Some("7.9"), None, &["Sci-Fi", "Drama"]),
        ];
        items(&format!(r#"{{"metas":[{}]}}"#, metas.join(",")))
    }

    fn ids(v: Vec<&Item>) -> Vec<&str> {
        v.into_iter().map(Item::id).collect()
    }

    const TODAY: fn() -> NaiveDate = || NaiveDate::from_ymd_opt(2026, 10, 26).unwrap();

    #[test]
    fn imdb_puts_unscored_last_and_keeps_ties_in_source_order() {
        let s = set();
        assert_eq!(
            ids(arrange(&s, Order::Imdb, None, None, TODAY())),
            ["tt4", "tt1", "tt3", "tt5", "tt2"]
        );
    }

    #[test]
    fn newest_leaves_out_future_titles_and_oldest_keeps_them() {
        let s = set();
        assert_eq!(
            ids(arrange(&s, Order::Newest, None, None, TODAY())),
            ["tt3", "tt2", "tt1", "tt5"]
        );
        assert_eq!(
            ids(arrange(&s, Order::Oldest, None, None, TODAY())),
            ["tt1", "tt2", "tt3", "tt4", "tt5"]
        );
    }

    #[test]
    fn an_imdb_score_that_is_not_a_number_counts_as_none() {
        let metas = [
            meta("tt1", "A", Some("NaN"), None, &[]),
            meta("tt2", "B", Some("inf"), None, &[]),
            meta("tt3", "C", Some("7.1"), None, &[]),
            meta("tt4", "D", Some("11"), None, &[]),
        ];
        let s = items(&format!(r#"{{"metas":[{}]}}"#, metas.join(",")));
        assert_eq!(
            ids(arrange(&s, Order::Imdb, None, None, TODAY())),
            ["tt3", "tt1", "tt2", "tt4"]
        );
    }

    #[test]
    fn a_to_z_skips_leading_punctuation_and_folds_ligatures() {
        assert_eq!(fold("'Salem's Lot"), "salem's lot");
        assert_eq!(fold("(500) Days of Summer"), "500) days of summer");
        assert_eq!(fold("Straße"), "strasse");
        assert!(fold("Æon Flux") < fold("Alien"));
    }

    #[test]
    fn a_to_z_folds_accents_and_case() {
        let s = set();
        assert_eq!(
            ids(arrange(&s, Order::Az, None, None, TODAY())),
            ["tt5", "tt3", "tt2", "tt1", "tt4"]
        );
    }

    #[test]
    fn genre_and_decade_filter_as_the_items_say() {
        let s = set();
        assert_eq!(
            ids(arrange(&s, Order::Source, Some("drama"), None, TODAY())),
            ["tt1", "tt2", "tt4", "tt5"]
        );
        assert_eq!(
            ids(arrange(&s, Order::Source, None, Some("2010s"), TODAY())),
            ["tt2"]
        );
        assert_eq!(
            ids(arrange(&s, Order::Source, None, Some("1990s"), TODAY())),
            ["tt1"]
        );
        assert!(in_decade(1979, "Earlier") && !in_decade(1980, "Earlier"));
        let genres = genres_of(&s);
        assert_eq!(genres[0], ("Crime".to_string(), 1));
        assert!(genres.contains(&("Drama".to_string(), 4)));
    }

    #[test]
    fn a_genre_extra_is_named_by_what_it_holds() {
        let v = |o: &[&str]| o.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(genre_label(&v(&["2026", "2025"])), "Year");
        assert_eq!(
            genre_label(&v(&["Yesterday", "This Week", "All Time"])),
            "Period"
        );
        assert_eq!(genre_label(&v(&["Action", "Drama"])), "Genre");
        assert_eq!(genre_label(&[]), "Genre");
    }
}
