//! Streaming services Discover can filter by, told from a catalog's own
//! name (plans/discover-v2.md section 4): any addon's "Netflix" list is a
//! Netflix list, and no addon is ever named.

/// A service: its stable key, the name shown, the names a catalog may go by,
/// TMDB's watch provider id (for its logo), and whether its lists belong on
/// the Anime tab whatever their type.
pub(crate) struct Service {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    names: &'static [&'static str],
    pub(crate) tmdb: u64,
    pub(crate) anime: bool,
}

const fn service(
    id: &'static str,
    name: &'static str,
    names: &'static [&'static str],
    tmdb: u64,
) -> Service {
    Service {
        id,
        name,
        names,
        tmdb,
        anime: false,
    }
}

/// TMDB provider ids as `/3/watch/providers/movie` listed them on 2026-10-27.
pub(crate) const SERVICES: [Service; 10] = [
    service("netflix", "Netflix", &["netflix"], 8),
    service(
        "prime",
        "Prime Video",
        &["prime video", "amazon prime video"],
        9,
    ),
    service("disney", "Disney+", &["disney+", "disney plus"], 337),
    service("apple", "Apple TV+", &["apple tv+", "apple tv plus"], 350),
    service("hbo", "HBO Max", &["hbo max", "max"], 1899),
    service(
        "paramount",
        "Paramount+",
        &["paramount+", "paramount plus"],
        531,
    ),
    service("peacock", "Peacock", &["peacock", "peacock premium"], 386),
    service("hulu", "Hulu", &["hulu"], 15),
    // Its lists are typed movie and series but hold anime (86 of 91 series,
    // 2026-10-25): they show on the Anime tab only.
    Service {
        anime: true,
        ..service("crunchyroll", "Crunchyroll", &["crunchyroll"], 283)
    },
    service("mubi", "Mubi", &["mubi"], 11),
];

/// The service a catalog's name names: the whole name, case-folded with its
/// spaces folded, optionally followed by a kind word ("Netflix Movies").
/// Never a part of a name: "Maximum" and "Max Picks" are lists.
pub(crate) fn of_catalog(name: &str) -> Option<&'static Service> {
    let folded = name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let bare = ["movies", "movie", "series", "shows", "show"]
        .iter()
        .find_map(|w| folded.strip_suffix(w).and_then(|n| n.strip_suffix(' ')))
        .unwrap_or(&folded);
    SERVICES.iter().find(|s| s.names.contains(&bare))
}

#[cfg(test)]
mod tests {
    use super::of_catalog;

    #[test]
    fn a_service_is_a_whole_name_never_a_part_of_one() {
        let id = |n: &str| of_catalog(n).map(|s| s.id);
        assert_eq!(id("Netflix"), Some("netflix"));
        assert_eq!(id("Netflix Movies"), Some("netflix"));
        assert_eq!(id("netflix series"), Some("netflix"));
        assert_eq!(id("Prime  Video"), Some("prime"));
        assert_eq!(id("Disney Plus"), Some("disney"));
        assert_eq!(id("Max"), Some("hbo"));
        assert_eq!(id("Crunchyroll"), Some("crunchyroll"));
        for list in [
            "Maximum",
            "Max Picks",
            "Netflix-style picks",
            "Popular",
            "Movies",
            "",
        ] {
            assert_eq!(id(list), None, "{list}");
        }
    }
}
