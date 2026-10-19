//! TurkceAltyazi.org built into the core (plans/m13-turkcealtyazi.md 3.7):
//! the rows that stand for it, and what a played title asks of it.

use crate::PlayTarget;
use crate::defaults::{self, DefaultAddon};
use mq_turkcealtyazi::Target;

const KEY: &str = "turkcealtyazi";
/// The source name on its tracks and notes.
pub(crate) const NAME: &str = "TurkceAltyazi.org";

fn default() -> &'static DefaultAddon {
    defaults::list()
        .iter()
        .find(|d| d.key == KEY)
        .expect("TurkceAltyazi.org is a default")
}

/// Whether an installed row stands for the built-in source: the default's
/// manifest id, or the default's own link (an older core's refresh may have
/// synced another manifest into it).
pub(crate) fn is_builtin(link: &str, manifest_id: Option<&str>) -> bool {
    let d = default();
    link == d.url || manifest_id == Some(d.manifest.id.as_str())
}

/// The digits the site writes for an IMDb id: `tt0903747` gives `0903747`.
pub(crate) fn digits(item_id: &str) -> Option<&str> {
    item_id.strip_prefix("tt").filter(|d| !d.is_empty())
}

/// A movie, or an episode with its numbers; nothing for anything else.
pub(crate) fn target(t: &PlayTarget) -> Option<Target> {
    match (t.kind.as_str(), t.season, t.episode) {
        ("movie", _, _) => Some(Target::Movie),
        ("series", Some(season), Some(episode)) => Some(Target::Episode { season, episode }),
        _ => None,
    }
}
