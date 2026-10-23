// Search, Discover, Library, Addons and Settings DTOs from the core,
// mirroring apps/web/src/core/generated/*.ts.

struct SearchSnapshot: Decodable, Hashable, Sendable {
    let query: String
    /// Rows with results, loading or failed; catalogs with no match are left out.
    let rows: [HomeRow]
    /// Recent searches, newest first, for the empty state.
    let recent: [String]
    /// The installed search catalogs a query goes to ("Top results" not counted). With none,
    /// empty `rows` mean no installed addon can search, not "No matches". Always 0 for kids.
    let searchable: Int
}

struct DiscoverSnapshot: Decodable, Hashable, Sendable {
    /// The kind, catalog and genre shown: the list a selection and a preview belong to.
    var list: String { "\(kind ?? "")|\(catalog ?? "")|\(genre ?? "")" }

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
    /// The genre a kind-plus-genre select asked for that no catalog of the kind lists (C40).
    let missingGenre: String?
    /// The item `preview {id}` named, with what its catalog said of it (C44a); nil until it is named and listed.
    let preview: ItemDetails?
}

/// A catalog item's synopsis, up to three genres and runtime, as its catalog gave them (C44a).
struct ItemDetails: Decodable, Hashable, Sendable {
    let id: String
    let description: String?
    let genres: [String]
    let runtime: String?
}

/// A title page's genre chip: one `select {kind, genre}`, and the core opens the first catalog of
/// the kind listing the genre (C40; plans/design-v3-phase7-apple.md 4.5).
struct DiscoverRequest: Equatable, Sendable {
    let kind: String
    let genre: String

    /// The select to send, or nil when Discover has no such kind (Discover then opens untouched).
    func selectCommand(kinds: [KindLabel]) -> [String: any Sendable]? {
        guard kinds.contains(where: { $0.id == kind }) else { return nil }
        return ["type": "select", "kind": kind, "genre": genre]
    }

    /// Sends the pending request once Discover has a snapshot, then clears it, so a later snapshot
    /// sends nothing.
    static func follow(_ pending: inout DiscoverRequest?, snapshot: DiscoverSnapshot?, send: ([String: any Sendable]) -> Void) {
        guard let request = pending, let snapshot else { return }
        pending = nil
        if let command = request.selectCommand(kinds: snapshot.kinds) { send(command) }
    }
}

/// Discover's own words. A catalog shows by its name, never its addon's (`CatalogChoice.source`), and a
/// failure never by the core's reason, which can name a host or an addon.
enum DiscoverWords {
    /// The quiet line beside the filters after a genre chip no catalog lists.
    static func missingGenre(_ genre: String?) -> String? { genre.map { "No \($0) here" } }

    static let failed = "This catalog did not load."
    static let empty = "Nothing here for this choice."
    static let needsGenre = "This catalog lists titles by genre. Pick one above."
    static let allGenres = "All genres"

    /// The kind control's choices: the core's labels ("Movies", "Series").
    static func kinds(_ s: DiscoverSnapshot) -> [(id: String, title: String)] { s.kinds.map { ($0.id, $0.label) } }
    static func kind(_ s: DiscoverSnapshot) -> String { s.kinds.first { $0.id == s.kind }?.label ?? "Kind" }

    static func catalogs(_ s: DiscoverSnapshot) -> [(id: String, title: String)] { s.catalogs.map { ($0.id, $0.name) } }
    static func catalog(_ s: DiscoverSnapshot) -> String { s.catalogs.first { $0.id == s.catalog }?.name ?? "Catalog" }

    /// The genre pull-down: "All genres" first (nil), unless the catalog needs a genre.
    static func genres(_ s: DiscoverSnapshot) -> [(id: String?, title: String)] {
        (s.genreRequired ? [] : [(nil, allGenres)]) + s.genres.map { ($0, $0) }
    }
    static func genre(_ s: DiscoverSnapshot) -> String { s.genre ?? (s.genreRequired ? "Genre" : allGenres) }

    /// The preview's meta line ("Movie · 2010 · Action, Sci-Fi · 148 min") and synopsis, from the tile
    /// and the core's details for it (`preview`); details for another item are not this tile's.
    static func preview(_ tile: Tile, _ details: ItemDetails?) -> (parts: [String], synopsis: String?) {
        let own = details?.id == tile.id ? details : nil
        let genres = own.map { $0.genres.joined(separator: ", ") }.flatMap { $0.isEmpty ? nil : $0 }
        let parts = [kindWord(tile.kind), tile.releaseInfo, genres, own?.runtime].compactMap { $0 }.filter { !$0.isEmpty }
        return (parts, own?.description.flatMap { $0.isEmpty ? nil : $0 })
    }

    /// The state line under the filters, if any.
    static func state(_ s: DiscoverSnapshot) -> String? {
        switch s.state {
        case .failed: failed
        case .end where s.items.isEmpty: empty
        default: s.genreRequired && s.genre == nil ? needsGenre : nil
        }
    }
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

extension AddonsSnapshot {
    /// Settings' Addons row: "4 installed. 1 finds streams." Counts only: an addon's name shows on
    /// the Addons pages alone (the owner, 2026-10-22; design/DECISION.md, Shelves).
    var summary: String {
        let finding = addons.filter { $0.enabled && $0.provides.contains("stream") }.count
        let count = addons.count == 1 ? "1 installed." : "\(addons.count) installed."
        switch finding {
        case 0: return "\(count) None of them finds streams yet."
        case 1: return "\(count) 1 finds streams."
        default: return "\(count) \(finding) find streams."
        }
    }
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
    /// Marquee runs this source itself (TurkceAltyazi.org, plans/m13-turkcealtyazi.md
    /// 3.7): no request reaches the addon's host. Absent from an older core.
    let builtIn: Bool?

    var id: String { url }

    /// What the card shows under the name: the host, or that it runs here.
    var hostLine: String { builtIn == true ? "Built into Marquee, runs on this device" : host }
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
    /// Its bundled logo on the core's loopback server, valid while that core
    /// runs: read it from each answer, never store it. Nil for Cinemeta
    /// (it publishes none), when the server did not start, and from a core
    /// before logos.
    let logo: String?

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

/// tvOS: the top bar steps out of the way once Home is scrolled into its
/// rows, as tvOS apps do, and comes back at the top (Up to the hero, or
/// Menu): it no longer sits over the first row's posters (the tvOS walk,
/// ATV-F4).
enum TVTopBar {
    static func hidden(atHome: Bool, scrolledDown: Bool) -> Bool {
        atHome && scrolledDown
    }
}

struct Prefs: Decodable, Hashable, Sendable {
    let streams: StreamPrefs
    /// Subtitle languages as addons name them (usually ISO 639-2, "eng"), most wanted first.
    let subtitleLanguages: [String]
    /// How big subtitles show, on every device of the profile; the delay stays per play.
    let subtitleSize: SubtitleSize
    /// Start the next episode when one ends.
    let autoplayNext: Bool
    /// The default addons version this profile was last offered.
    let defaultsSeen: Int
    /// What the picks taught, by screen class (plans/m13-stream-choice.md 3.9).
    let learned: LearnedByClass
    /// Counters stamped below it read as zero: "Forget what was learned".
    let learnedEpoch: Int
}

enum SubtitleSize: String, Codable, Hashable, Sendable, CaseIterable {
    case small, medium, large, extraLarge
}

/// One entry of the `languages` op, for the language pickers: `code` goes into
/// `subtitleLanguages` as is; audio (`streams.languages`) takes the part before "-".
struct LanguageChoice: Decodable, Hashable, Sendable {
    let code: String
    let name: String
    /// The language the device's code reads as ("nb" is Norwegian, "no"),
    /// when the request named one; an older core sends none.
    var device: Bool? = nil
}

extension LanguageChoice {
    /// The picker's rows for a search: subtitles keep the core's tag, audio
    /// takes the part before "-" once, named without its script or region
    /// ("zh-Hans" and "zh-Hant" are one "zh", Chinese). A search matches a
    /// name in part or a code whole.
    static func rows(_ all: [LanguageChoice], audio: Bool, query: String) -> [LanguageChoice] {
        var seen = Set<String>()
        let wanted = query.trimmingCharacters(in: .whitespaces)
        return all.compactMap { choice in
            let code = audio ? String(choice.code.split(separator: "-").first ?? Substring(choice.code)) : choice.code
            guard seen.insert(code).inserted else { return nil }
            let name = audio ? (choice.name.components(separatedBy: " (").first ?? choice.name) : choice.name
            guard wanted.isEmpty || name.localizedCaseInsensitiveContains(wanted) || code.caseInsensitiveCompare(wanted) == .orderedSame else { return nil }
            return LanguageChoice(code: code, name: name)
        }
    }
}

struct LearnedByClass: Decodable, Hashable, Sendable {
    let uhd: Learned
    let fhd: Learned
    let hd: Learned
    let sd: Learned
}

/// One screen class's counters, -5 to 5 each; at 3 or more either way a choice is learned.
struct Learned: Decodable, Hashable, Sendable {
    let ready: Int
    let height: Int
    let hdr: Int
    let size: Int
    let picks: Int
    let epoch: Int
    let atMs: Int64
    /// Choices undone here: they no longer learn.
    let muted: [String]
}

/// This device's connection and space for Settings ("This device").
struct StreamConditions: Decodable, Hashable, Sendable {
    let network: String
    let http: Speed
    let p2p: Speed
    let freeBytes: Int64?
    let engine: Bool
}

struct StreamPrefs: Decodable, Hashable, Sendable {
    /// "best", "balanced" or "dataSaver"; read once by the core and mapped, then ignored.
    let quality: String
    /// Never pick above this many lines (720, 1080, 2160), whatever the screen; nil is the screen's.
    let maxHeight: Int?
    let maxSizeBytes: Int64?
    /// ISO 639-1 audio language preferences, most wanted first.
    let languages: [String]
    /// Lowercase terms that hide a stream when its name contains them.
    let avoid: [String]
    /// The soft size limit (5 GiB by default); nil is no limit.
    let sizeLimitBytes: Int64?
    /// Seeders wanted (50 by default).
    let minSeeders: Int
    let hdr: Bool
    let preferReady: Bool
    let learn: Bool
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
