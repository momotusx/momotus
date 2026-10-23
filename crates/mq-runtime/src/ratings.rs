//! The IMDb score every title shows, from the addon that gave the title
//! (the meta addon's own rating, as Cinemeta writes it in its `imdb`
//! link). Rotten Tomatoes and Metacritic come only as the ratings band
//! RPDB draws into the cover (`Tile::rated_cover`), with the key an
//! installed addon was configured with: nothing is looked up elsewhere
//! (the owner, 2026-10-23).

use serde::Serialize;

/// A title's IMDb score, written as shown ("8.8"). A score the addon did
/// not give is null: apps show no slot for it, never a placeholder.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct Ratings {
    pub imdb: Option<String>,
}

impl Ratings {
    /// The addon's own rating (`addon_imdb`, as the addon wrote it), on
    /// IMDb's scale of 1 to 10 with one decimal; anything else (not a
    /// number, off the scale) counts as missing.
    pub fn of(addon_imdb: Option<&str>) -> Self {
        let imdb = addon_imdb
            .and_then(|r| r.trim().parse::<f64>().ok())
            .filter(|v| (1.0..=10.0).contains(v));
        Self {
            imdb: imdb.map(|v| format!("{v:.1}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Ratings;

    #[test]
    fn the_imdb_score_is_the_addons_on_its_scale() {
        assert_eq!(Ratings::of(Some(" 8.4 ")).imdb.as_deref(), Some("8.4"));
        assert_eq!(Ratings::of(Some("7")).imdb.as_deref(), Some("7.0"));
        assert_eq!(Ratings::of(Some("10")).imdb.as_deref(), Some("10.0"));
        // Off the scale or not a number: no slot, never a placeholder.
        for addon in [
            None,
            Some(""),
            Some("N/A"),
            Some("0"),
            Some("-1"),
            Some("10.5"),
            Some("NaN"),
        ] {
            assert_eq!(Ratings::of(addon), Ratings::default(), "{addon:?}");
        }
    }
}
