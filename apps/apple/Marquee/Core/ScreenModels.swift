// Search, Discover, Library, Addons and Settings DTOs from the core,
// mirroring apps/web/src/core/generated/*.ts.

struct SearchSnapshot: Decodable, Hashable, Sendable {
    let query: String
    /// Rows with results, loading or failed; catalogs with no match are left out.
    let rows: [HomeRow]
    /// Recent searches, newest first, for the empty state.
    let recent: [String]
}

struct DiscoverSnapshot: Decodable, Hashable, Sendable {
    /// Types with at least one catalog: ("movie", "Movies").
    let kinds: [KindLabel]
    let kind: String?
    /// Catalogs of the selected type, in addon order.
    let catalogs: [CatalogChoice]
    let catalog: String?
    /// Genre options of the selected catalog.
    let genres: [String]
    let genre: String?
    /// The catalog only answers with a genre picked.
    let genreRequired: Bool
    let items: [Tile]
    let state: DiscoverState
}

/// A type and its label; the core sends each pair as a two-element array.
struct KindLabel: Decodable, Hashable, Sendable {
    let id: String
    let label: String

    init(from decoder: any Decoder) throws {
        var c = try decoder.unkeyedContainer()
        id = try c.decode(String.self)
        label = try c.decode(String.self)
    }
}

struct CatalogChoice: Decodable, Hashable, Sendable {
    /// `addon id|type|catalog id`, the value `select` takes.
    let id: String
    let name: String
    let source: String
}

enum DiscoverState: Decodable, Hashable, Sendable {
    case loading
    case ready
    case end
    case failed(reason: String)

    private enum Key: String, CodingKey { case state, reason }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        switch try c.decode(String.self, forKey: .state) {
        case "loading": self = .loading
        case "ready": self = .ready
        case "end": self = .end
        case "failed": self = .failed(reason: try c.decode(String.self, forKey: .reason))
        case let state:
            throw DecodingError.dataCorruptedError(forKey: .state, in: c, debugDescription: "unknown discover state \(state)")
        }
    }
}

struct LibrarySnapshot: Decodable, Hashable, Sendable {
    /// The type filter, nil for everything.
    let kind: String?
    /// "recent", "added" or "name".
    let sort: String
    /// Types present in the library, for the filter chips.
    let kinds: [KindCount]
    let items: [Tile]
}

struct KindCount: Decodable, Hashable, Sendable {
    let kind: String
    /// "Movies", "Series", ...
    let label: String
    let count: Int
}

struct AddonsSnapshot: Decodable, Hashable, Sendable {
    let addons: [AddonInfo]
    /// First run: the profile starts at the recommended addons step (it never
    /// had a list, this device is in no sync account, not a kids profile).
    let fresh: Bool
}

struct AddonInfo: Decodable, Hashable, Sendable, Identifiable {
    /// The key for commands. May hold API keys: never show or log it.
    let url: String
    /// What to show instead of the URL.
    let host: String
    let name: String
    let description: String?
    let logo: String?
    let version: String
    let enabled: Bool
    /// "catalog", "meta", "stream", "subtitles", ...
    let provides: [String]
    let types: [String]
    let configurable: Bool
    /// The addon's configure page, when its manifest says it has one.
    let configureUrl: String?

    var id: String { url }
}

/// Find addons: Cinemeta's two lists, Official then Community (plans/core-protocol.md).
struct AddonCatalogSnapshot: Decodable, Hashable, Sendable {
    let lists: [AddonList]
}

struct AddonList: Decodable, Hashable, Sendable {
    /// "official" or "community".
    let id: String
    let name: String
    let state: ListState
    let entries: [ListedAddon]
}

enum ListState: Decodable, Hashable, Sendable {
    case loading
    case ready
    case failed(words: String)

    private enum Key: String, CodingKey { case state, words }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        switch try c.decode(String.self, forKey: .state) {
        case "loading": self = .loading
        case "ready": self = .ready
        case "failed": self = .failed(words: try c.decode(String.self, forKey: .words))
        case let state:
            throw DecodingError.dataCorruptedError(forKey: .state, in: c, debugDescription: "unknown list state \(state)")
        }
    }
}

struct ListedAddon: Decodable, Hashable, Sendable {
    /// The canonical link, for `installListed` (and Turn on when `off`) only.
    /// May hold keys: never show or log it.
    let key: String
    /// The manifest id.
    let id: String
    /// What to show instead of the link.
    let host: String
    let name: String
    let description: String?
    let types: [String]
    /// "catalog", "meta", "stream", "subtitles", ...
    let provides: [String]
    let configuration: Configuration
    /// The addon's configure page, always set; it opens in the system browser.
    let configureUrl: String
    let installed: Installed
    /// With `copyOff`: the first configured copy in addon order, for Turn on.
    /// As secret as `key`.
    let copyKey: String?

    enum Configuration: String, Decodable, Sendable {
        case none, optional, required
    }

    /// This link installed on or off, a configured copy (same manifest id
    /// and host, another link) on or off, or neither.
    enum Installed: String, Decodable, Sendable {
        case absent, on, off, copyOn, copyOff
    }
}

/// The default addons and whether each is in the profile's list
/// (`defaultAddons`, plans/m13-default-addons.md).
struct DefaultAddons: Decodable, Hashable, Sendable {
    /// The list's version: a profile whose `defaultsSeen` is below it is
    /// offered the missing ones once.
    let version: Int
    /// The nine, in order.
    let addons: [DefaultAddonInfo]
}

struct DefaultAddonInfo: Decodable, Hashable, Sendable, Identifiable {
    /// Names it in `installDefaults` choices.
    let key: String
    let name: String
    /// What it does, in one line; shown as is.
    let line: String
    /// "catalogs", "streams", "subtitles" or "lists".
    let group: String
    /// In the list by manifest id: a copy with other settings counts.
    let installed: Installed

    var id: String { key }

    enum Installed: String, Decodable, Sendable {
        case absent, on, off
    }
}

/// Menu (Back) at the top of a TV place, as on Android: a place other than
/// Home goes to Home, Home scrolled down goes back to its top row, and only
/// then does the app exit. A pushed page pops first; that is the stack's.
enum TVBack: Equatable {
    case goHome
    case toTop
    case exit

    static func step(atHome: Bool, scrolledDown: Bool) -> TVBack {
        guard atHome else { return .goHome }
        return scrolledDown ? .toTop : .exit
    }
}

struct Prefs: Decodable, Hashable, Sendable {
    let streams: StreamPrefs
    /// Subtitle languages as addons name them (usually ISO 639-2, "eng"), most wanted first.
    let subtitleLanguages: [String]
    /// Start the next episode when one ends.
    let autoplayNext: Bool
    /// The default addons version this profile was last offered.
    let defaultsSeen: Int
}

struct StreamPrefs: Decodable, Hashable, Sendable {
    /// "best", "balanced" or "dataSaver".
    let quality: String
    /// Never pick above this many lines (720, 1080, 2160), whatever the screen.
    let maxHeight: Int?
    let maxSizeBytes: Int64?
    /// ISO 639-1 audio language preferences, most wanted first.
    let languages: [String]
    /// Lowercase terms that hide a stream when its name contains them.
    let avoid: [String]
}

/// Episodes of library series from a week before to a month after `today`, by date.
struct CalendarSnapshot: Decodable, Sendable {
    /// The local date the app gave, "YYYY-MM-DD".
    let today: String
    let days: [CalendarDay]
    /// Series whose details are still loading.
    let pending: Int
    /// Series whose details could not be fetched.
    let failed: [CalendarSeries]
    /// Series without a dated episode.
    let noSchedule: [CalendarSeries]
    /// Library series the calendar follows; 0 means there are none to follow.
    let series: Int
}

extension CalendarSnapshot {
    /// Where the screen opens: the first day on or after today (dates compare
    /// as "YYYY-MM-DD" text), else the last day.
    var startDay: String? { days.first { $0.date >= today }?.date ?? days.last?.date }
}

struct CalendarDay: Decodable, Hashable, Sendable {
    /// "YYYY-MM-DD": a date, never shifted by time zone.
    let date: String
    let items: [CalendarItem]
}

struct CalendarItem: Decodable, Hashable, Sendable {
    let itemId: String
    /// The meta's video id: the item's stable key.
    let videoId: String
    let name: String
    let season: Int?
    let episode: Int?
    let title: String
    let date: String
    let poster: String?
    let still: String?
    let watched: Bool
}

struct CalendarSeries: Decodable, Hashable, Sendable {
    let itemId: String
    let name: String
    let reason: String?
}

/// One Home catalog as the Home rows screen lists it; hidden ones sit
/// where they would show.
struct HomeCatalog: Decodable, Hashable, Sendable {
    let rowId: String
    let title: String
    let source: String
    let hidden: Bool

    /// The row's hidden value after a toggle, read from the latest list (not
    /// from what was on screen when it was pressed); nil when the row is gone.
    static func toggled(_ rowId: String, in rows: [HomeCatalog]?) -> Bool? {
        rows?.first { $0.rowId == rowId }.map { !$0.hidden }
    }
}
