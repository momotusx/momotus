//! The ratings band on every cover (the owner, 2026-10-22 and 2026-10-23:
//! "the tomato shows on all of them"): RPDB's poster with the IMDb,
//! Metacritic and Rotten Tomatoes band drawn in, for the title's IMDb id,
//! with the RPDB key an installed, enabled addon was configured with. A
//! title its addon names by another id (Kitsu's `kitsu:`) takes the IMDb id
//! the addon gave beside it (`imdb_id`, as the Kitsu addon writes it on its
//! catalog items and metas), kept in the library so Library, Continue
//! Watching and a kids list have it too. A title with neither keeps its
//! addon's poster.

use crate::Core;
use crate::segments::is_imdb;
use std::collections::HashMap;

pub(crate) const RPDB: &str = "https://api.ratingposterdb.com/";

/// What a batch of covers is drawn with: the addons' RPDB key and the IMDb
/// ids kept for the batch's other ids.
#[derive(Debug, Default)]
pub(crate) struct Covers {
    key: Option<String>,
    imdb: HashMap<String, String>,
}

impl Covers {
    /// RPDB's cover for `id`: its own IMDb id, else the one its addon gave.
    /// `None` with no key, or with no IMDb id for it.
    pub(crate) fn url(&self, id: &str) -> Option<String> {
        let key = self.key.as_deref()?;
        let imdb = if is_imdb(id) {
            id
        } else {
            self.imdb.get(id)?.as_str()
        };
        Some(format!(
            "{RPDB}{key}/imdb/poster-default/{imdb}.jpg?fallback=true"
        ))
    }

    #[cfg(test)]
    pub(crate) fn with(key: Option<&str>, imdb: &[(&str, &str)]) -> Self {
        Self {
            key: key.map(str::to_owned),
            imdb: imdb
                .iter()
                .map(|(id, tt)| ((*id).to_owned(), (*tt).to_owned()))
                .collect(),
        }
    }
}

impl Core {
    /// The covers for these ids: the RPDB key an installed, enabled addon
    /// was configured with, the first in the user's order (its link's
    /// config; the default Rotten Tomatoes addon carries
    /// `{"rpdb_key":"t0-free-rpdb"}`), and the IMDb ids kept for those that
    /// are not IMDb's. No key, no band: covers stay the addons' own. A kids
    /// profile reads the owner's list, as for every addon. Tier 0 serves
    /// `poster-default` only; images may be cached for personal use, so the
    /// apps' image caches must keep them.
    pub(crate) fn covers<'a>(&self, ids: impl IntoIterator<Item = &'a str>) -> Covers {
        self.covers_with(self.rpdb_key(), ids)
    }

    /// The RPDB key `covers` draws with, for a caller that reads it once for
    /// many batches (each reads every installed addon's manifest).
    pub(crate) fn rpdb_key(&self) -> Option<String> {
        self.installed_addons()
            .ok()
            .and_then(|addons| addons.iter().find_map(|a| rpdb_key_in(&a.transport_url)))
    }

    /// A title's IMDb id: its own, or the one its addon gave (kept by
    /// `learn_imdb_ids`) for an id that is not IMDb's.
    pub(crate) fn imdb_id_of(&self, id: &str) -> Option<String> {
        if is_imdb(id) {
            return Some(id.to_owned());
        }
        self.library().imdb_ids(&[id]).ok()?.remove(id)
    }

    /// `covers` with a key read before.
    pub(crate) fn covers_with<'a>(
        &self,
        key: Option<String>,
        ids: impl IntoIterator<Item = &'a str>,
    ) -> Covers {
        let others: Vec<&str> = ids.into_iter().filter(|id| !is_imdb(id)).collect();
        let imdb = if key.is_some() && !others.is_empty() {
            self.library().imdb_ids(&others).unwrap_or_default()
        } else {
            HashMap::new()
        };
        Covers { key, imdb }
    }

    /// Keeps the IMDb ids an addon's answer (a catalog's `metas` or a
    /// `meta`) gives for titles it names by another id, writing only those
    /// that are new or changed. The table is keyed by the id alone, with no
    /// addon scope: one addon can overwrite another's mapping. That is fine
    /// for a cover and for the title page's lookups, which send only a
    /// checked `tt` id; anything stronger must scope it first.
    pub(crate) fn learn_imdb_ids(&self, body: &[u8]) {
        let library = self.library();
        let removals = library.removals();
        let pairs = imdb_ids_in(body);
        if pairs.is_empty() {
            return;
        }
        let ids: Vec<&str> = pairs.iter().map(|(id, _)| id.as_str()).collect();
        let kept = library.imdb_ids(&ids).unwrap_or_default();
        let new: Vec<(String, String)> = pairs
            .into_iter()
            .filter(|(id, imdb)| kept.get(id) != Some(imdb))
            .collect();
        if let Err(e) = library.put_imdb_ids(&new, removals) {
            tracing::warn!("IMDb ids not kept: {e}");
        }
    }
}

/// The `rpdb_key` in an addon link's config segment, when it is a plain
/// key: it goes into RPDB's path, so anything but letters, digits, `-` and
/// `_` is refused.
fn rpdb_key_in(link: &url::Url) -> Option<String> {
    link.path_segments()?.find_map(|segment| {
        let config = percent_encoding::percent_decode_str(segment)
            .decode_utf8()
            .ok()?;
        let config: serde_json::Value = serde_json::from_str(&config).ok()?;
        let key = config.get("rpdb_key")?.as_str()?;
        let plain = (1..=64).contains(&key.len())
            && key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        plain.then(|| key.to_owned())
    })
}

/// `(id, imdb_id)` for each item of an addon answer whose id is not IMDb's
/// and whose `imdb_id` is: stremio-core's types drop the field, so it is
/// read from the raw JSON. Anything else is left out.
fn imdb_ids_in(body: &[u8]) -> Vec<(String, String)> {
    // Most addons never write the field: their answers are not read again.
    if !body.windows(7).any(|w| w == b"imdb_id") {
        return Vec::new();
    }
    // Only the two fields are kept: Cinemeta writes `imdb_id` on every item,
    // and its answers are long.
    #[derive(serde::Deserialize)]
    struct Item {
        id: Option<serde_json::Value>,
        imdb_id: Option<serde_json::Value>,
    }
    /// An entry that is not an object (a stray null or string) is skipped,
    /// not the whole answer.
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Entry {
        Item(Item),
        Other(serde::de::IgnoredAny),
    }
    #[derive(serde::Deserialize)]
    struct Answer {
        metas: Option<Vec<Entry>>,
        meta: Option<Entry>,
    }
    let Ok(answer) = serde_json::from_slice::<Answer>(body) else {
        return Vec::new();
    };
    answer
        .metas
        .unwrap_or_default()
        .into_iter()
        .chain(answer.meta)
        .filter_map(|entry| {
            let Entry::Item(item) = entry else {
                return None;
            };
            let id = item.id?.as_str()?.to_owned();
            let imdb = item.imdb_id?.as_str()?.to_owned();
            (!is_imdb(&id) && id.len() <= 128 && is_imdb(&imdb)).then_some((id, imdb))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The owner, 2026-10-23: the band comes from the addon the user added,
    /// so the key is read from its link's config segment; one that could
    /// leave RPDB's path is refused.
    #[test]
    fn the_rpdb_key_is_read_from_the_addon_link() {
        let key = |link: &str| rpdb_key_in(&url::Url::parse(link).unwrap());
        assert_eq!(
            key("https://7a82163c306e-rottentomatoes.baby-beamup.club/%7B%22rpdb_key%22%3A%22t0-free-rpdb%22%7D/manifest.json")
                .as_deref(),
            Some("t0-free-rpdb")
        );
        assert_eq!(
            key("https://7a82163c306e-rottentomatoes.baby-beamup.club/manifest.json"),
            None
        );
        assert_eq!(
            key("https://x.example/%7B%22rpdb_key%22%3A%22%22%7D/manifest.json"),
            None
        );
        for bad in ["..%2F..%2Fadmin", "a%3Fb", "a%20b", "t0%2Fx"] {
            let link =
                format!("https://x.example/%7B%22rpdb_key%22%3A%22{bad}%22%7D/manifest.json");
            assert_eq!(key(&link), None, "{bad}");
        }
        assert_eq!(
            key("https://x.example/%7B%22rpdb_key%22%3A7%7D/manifest.json"),
            None
        );
    }

    /// The Kitsu addon's catalog items and metas carry `imdb_id` (the live
    /// answers of 2026-10-23: 50 of 50 trending, 17 of 20 airing); only a
    /// real IMDb id for a title named otherwise is kept.
    #[test]
    fn an_answer_gives_the_imdb_ids_of_its_other_titles() {
        let catalog = br#"{"metas":[
            {"id":"kitsu:50016","type":"anime","name":"Cyberpunk: Edgerunners II","imdb_id":"tt12590266"},
            {"id":"kitsu:13874","type":"anime","name":"Fate/Grand Order CMs","imdb_id":null},
            {"id":"kitsu:1","type":"anime","name":"x","imdb_id":"not-an-id"},
            {"id":"tt0111161","type":"movie","name":"y","imdb_id":"tt0111161"},
            {"type":"anime","imdb_id":"tt1"},
            null,
            "stray"
        ]}"#;
        assert_eq!(
            imdb_ids_in(catalog),
            [("kitsu:50016".to_owned(), "tt12590266".to_owned())]
        );
        let meta = br#"{"meta":{"id":"kitsu:50016","type":"anime","imdb_id":"tt12590266"}}"#;
        assert_eq!(imdb_ids_in(meta).len(), 1);
        assert!(imdb_ids_in(b"{}").is_empty());
        assert!(imdb_ids_in(b"not json").is_empty());
    }

    #[test]
    fn a_cover_needs_a_key_and_an_imdb_id() {
        let covers = Covers::with(Some("t0-free-rpdb"), &[("kitsu:50016", "tt12590266")]);
        assert_eq!(
            covers.url("tt0111161").as_deref(),
            Some(
                "https://api.ratingposterdb.com/t0-free-rpdb/imdb/poster-default/tt0111161.jpg?fallback=true"
            )
        );
        assert_eq!(
            covers.url("kitsu:50016").as_deref(),
            Some(
                "https://api.ratingposterdb.com/t0-free-rpdb/imdb/poster-default/tt12590266.jpg?fallback=true"
            )
        );
        assert_eq!(covers.url("kitsu:13874"), None);
        assert_eq!(
            Covers::with(None, &[("kitsu:50016", "tt12590266")]).url("kitsu:50016"),
            None
        );
        assert_eq!(Covers::default().url("tt0111161"), None);
    }
}
