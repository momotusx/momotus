// Search, Discover, Library, Addons and Settings DTOs from the core,
// mirroring apps/web/src/core/generated/*.ts.

import Foundation

struct SearchSnapshot: Decodable, Hashable, Sendable {
    let query: String
    /// Rows with results, loading or failed; catalogs with no match are left out.
    let rows: [HomeRow]
    /// Recent searches, newest first, for the empty state.
    let recent: [String]
    /// The installed search catalogs a query goes to ("Top results" not counted). With none,
    /// empty `rows` mean no installed addon can search, not "No matches". Always 0 for kids.
    let searchable: Int
    /// The empty state's shelf of titles to start from (Home's first catalog that answers, titled as Home titles
    /// it; a kids profile's own list). nil while it loads, offline, with a query typed and with nothing to show.
    let start: HomeRow?
}

/// The genre grid (core view `genres`, C43): Discover's kinds, each with its catalogs' genres and one art each.
struct GenresSnapshot: Decodable, Hashable, Sendable {
    let kinds: [KindGenres]
}

struct KindGenres: Decodable, Hashable, Sendable {
    let kind: String
    let label: String
    let genres: [GenreTile]
}

struct GenreTile: Decodable, Hashable, Sendable {
    let genre: String
    /// A backdrop URL; nil until fetched, and when the genre's first item has none.
    let art: String?
}

/// Search's own words (plans/design-v3-phase7-apple.md 3): what the core searches (titles, G1), and no
/// addon's name, ever (a group shows by the core's title, a failed one by `HomeRow.stateWords`).
enum SearchWords {
    static let hint = "Search movies and series by title."
    static let noAddons = "None of your addons can search"
    /// The way on from `noAddons`: the recommended addons, which search.
    static let addons = DefaultsRules.title
    static let recent = "Recent"
    static let clearRecent = "Clear"
    static let clearRecentLabel = "Clear recent searches"
    static let topResults = "Top results"
    /// The view could not open: the core's words can name a host or an addon, so these instead.
    static let failed = "Search did not load."
    /// The core's Top results row (`TOP_ROW_ID`).
    static let topRowId = "top"

    /// A kids profile's Search looks over its own list only (plans/m11-kids.md 6.4): its field and its no-match line.
    static let kidsPrompt = "Search your shows"
    static let kidsNoMatch = "That isn't on your list. Ask a grown-up."

    /// The line under the field before a query: that no addon can search, else the hint, and the hint only where
    /// nothing else shows (no Recent, no shelf of titles). nil until the core's first snapshot.
    static func idleLine(_ s: SearchSnapshot?, kids: Bool = false) -> String? {
        guard let s else { return nil }
        if !kids && s.searchable == 0 { return noAddons }
        return s.recent.isEmpty && s.start == nil ? hint : nil
    }

    /// After the core answered `query`: that no addon can search, or that nothing matched; nil otherwise.
    static func answered(_ s: SearchSnapshot?, query: String, kids: Bool = false) -> String? {
        guard let s, s.query == query.trimmingCharacters(in: .whitespaces) else { return nil }
        if kids { return s.rows.isEmpty ? kidsNoMatch : nil }
        if s.searchable == 0 { return noAddons }
        return s.rows.isEmpty ? noMatches(s.query) : nil
    }

    /// Nothing came back for `query`: said with the query, as the web and Android say it.
    static func noMatches(_ query: String) -> String { "No matches for \"\(query)\" in your catalogs." }

    /// No addon can search, so nothing could match: the line offers the recommended addons. Never to a kids profile.
    static func offersAddons(_ s: SearchSnapshot?, kids: Bool = false) -> Bool { !kids && s?.searchable == 0 }

    enum Top: Equatable {
        case loading
        case tiles([Tile])
    }

    /// Top results: its first six titles, skeletons while it loads, nothing when absent, failed or empty.
    static func top(_ rows: [HomeRow]) -> Top? {
        guard let row = rows.first(where: { $0.rowId == topRowId }) else { return nil }
        switch row.state {
        case .loading, .slow: return .loading
        case .ready where !row.tiles.isEmpty: return .tiles(Array(row.tiles.prefix(6)))
        default: return nil
        }
    }

    /// The kind groups, in the core's order.
    static func groups(_ rows: [HomeRow]) -> [HomeRow] { rows.filter { $0.rowId != topRowId } }

    /// "Movie · 2010" under a result's name: Marquee's kind word and the year.
    static func meta(_ tile: Tile) -> String {
        [kindWord(tile.kind), tile.releaseInfo].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " \u{B7} ")
    }
}

struct DiscoverSnapshot: Decodable, Hashable, Sendable {
    /// The kind, service, sort, catalog, genre and decade shown: the list a selection and a preview belong to.
    var list: String { "\(kind ?? "")|\(platform ?? "")|\(sort ?? "")|\(catalog ?? "")|\(genre ?? "")|\(decade ?? "")" }

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
    // Discover v2 (plans/discover-v2.md section 4): optional, so recordings and an older core decode.
    /// The services the rail offers for the kind; nil is All.
    let platforms: [ServiceChoice]?
    let platform: String?
    /// The orders the sort menu offers, then every other catalog of the kind ("More lists").
    let sorts: [SortChoice]?
    let sort: String?
    let lists: [CatalogChoice]?
    let genreChoices: [GenreChoice]?
    /// What the genre menu is called: "Genre", or "Year" or "Period".
    let genreLabel: String?
    let decades: [String]?
    let decade: String?
    let scope: DiscoverScope?
    let missingSort: String?
}

/// A streaming service on Discover's rail: `id` is the stable key, `name` the name shown.
struct ServiceChoice: Decodable, Hashable, Sendable {
    let id: String
    let name: String
    /// TMDB's logo for it, nil until fetched or when there is none: the name is the tile's words then.
    let logo: String?
}

/// An order the sort menu offers; `mark` names a rating mark ("imdb", "rt").
struct SortChoice: Decodable, Hashable, Sendable {
    let id: String
    let label: String
    let mark: String?
}

/// A genre the genre menu offers, with how many titles give it in a complete set.
struct GenreChoice: Decodable, Hashable, Sendable {
    let name: String
    let count: Int?
}

/// What the list on screen covers, for the footer.
struct DiscoverScope: Decodable, Hashable, Sendable {
    let titles: Int?
    let services: Int
    let failed: Int
    let pending: Int
    let looked: Int?
    let note: String?
}

/// A catalog item's synopsis, up to three genres and runtime, as its catalog gave them (C44a).
struct ItemDetails: Decodable, Hashable, Sendable {
    let id: String
    let description: String?
    let genres: [String]
    let runtime: String?
    /// In the Library as it stands; `setInLibrary {id, value}` on Discover changes it (C44b).
    let inLibrary: Bool?
    /// Add to Library can be offered: false on a kids profile.
    let librarySettable: Bool?
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

    /// Sends the pending request once Discover has a snapshot with its kinds and the view can take a
    /// command (`canSend`: its subscription is open), then clears it, so a later snapshot sends nothing.
    static func follow(_ pending: inout DiscoverRequest?, snapshot: DiscoverSnapshot?, canSend: Bool = true, send: ([String: any Sendable]) -> Void) {
        guard canSend, let request = pending, let snapshot, !snapshot.kinds.isEmpty else { return }
        pending = nil
        if let command = request.selectCommand(kinds: snapshot.kinds) { send(command) }
    }
}

/// Discover's own words. A catalog shows by its name, never its addon's (`CatalogChoice.source`), and a
/// failure never by the core's reason, which can name a host or an addon.
enum DiscoverWords {
    /// The quiet line beside the filters after a genre chip no catalog lists.
    static func missingGenre(_ genre: String?) -> String? { genre.map { "No \($0) here" } }

    static let failed = "This list did not load."
    static let empty = "Nothing here for this choice."
    static let needsGenre = "This catalog lists titles by genre. Pick one above."
    static let allGenres = "All genres"

    /// The kind control's choices: the core's labels ("Movies", "Series").
    static func kinds(_ s: DiscoverSnapshot) -> [(id: String, title: String)] { s.kinds.map { ($0.id, $0.label) } }

    /// Movies, Series and Anime: the segmented control, whatever else is installed (plans/discover-v2.md 6.1).
    static let mainKinds: Set<String> = ["movie", "series", "anime"]
    static func segments(_ s: DiscoverSnapshot) -> [(id: String, title: String)] { kinds(s).filter { mainKinds.contains($0.id) } }
    /// Every other kind (Channels, Live TV, Other): the trailing More pull-down.
    static func more(_ s: DiscoverSnapshot) -> [(id: String, title: String)] { kinds(s).filter { !mainKinds.contains($0.id) } }
    static func moreTitle(_ s: DiscoverSnapshot) -> String {
        more(s).first { $0.id == s.kind }?.title ?? "More"
    }

    /// How the kinds are offered: Movies, Series and Anime segmented with More beside (plans/discover-v2.md 6.1), a
    /// pull-down of every kind at an accessibility text size (6.4), nothing for a single kind.
    static func kindControl(_ s: DiscoverSnapshot, accessibilitySize: Bool) -> KindControl {
        let segments = segments(s), more = more(s)
        if segments.count + more.count < 2 { return .none }
        return accessibilitySize || segments.isEmpty ? .pullDown : .segmented
    }
    static func kindTitle(_ s: DiscoverSnapshot) -> String { kinds(s).first { $0.id == s.kind }?.title ?? "Kind" }

    /// Where Clear hands focus (6.5): the genre menu, else the year menu, else the rail's All, so focus is not left
    /// on a button that goes away.
    enum ClearTarget: Equatable { case genre, year, all }
    static func clearTarget(_ s: DiscoverSnapshot) -> ClearTarget {
        if !genreChoices(s).isEmpty, !(s.genreChoices ?? []).isEmpty || !s.genres.isEmpty { return .genre }
        return decades(s).isEmpty ? .all : .year
    }

    /// The rail: All (nil) first, then the services, each with its logo when there is one.
    static func services(_ s: DiscoverSnapshot) -> [(id: String?, title: String, logo: String?)] {
        let all = s.platforms ?? []
        return all.isEmpty ? [] : [(nil, "All", nil)] + all.map { ($0.id, $0.name, $0.logo) }
    }

    /// The sort menu: the core's sorts, then the other lists by their own names.
    static func sorts(_ s: DiscoverSnapshot) -> [(id: String, title: String)] {
        (s.sorts ?? []).map { ($0.id, $0.label) } + (s.lists ?? []).map { ("list:\($0.id)", $0.name) }
    }
    static func sort(_ s: DiscoverSnapshot) -> String { sorts(s).first { $0.id == s.sort }?.title ?? "Sort" }
    /// The menu in two parts: the orders, then the other catalogs under "More lists", so a catalog called "Top" does
    /// not read as an order.
    static func orders(_ s: DiscoverSnapshot) -> [(id: String, title: String)] { sorts(s).filter { !$0.id.hasPrefix("list:") } }
    static func lists(_ s: DiscoverSnapshot) -> [(id: String, title: String)] { sorts(s).filter { $0.id.hasPrefix("list:") } }
    static let moreLists = "More lists"

    /// The genre menu's choices: "Any genre" first (nil) unless the list needs one.
    static func genreChoices(_ s: DiscoverSnapshot) -> [(id: String?, title: String)] {
        let label = (s.genreLabel ?? "Genre").lowercased()
        let names = s.genreChoices.map { $0.map(\.name) } ?? s.genres
        return (s.genreRequired ? [] : [(nil, "Any \(label)")]) + names.map { ($0, $0) }
    }
    static func genreTitle(_ s: DiscoverSnapshot) -> String { s.genre ?? genreChoices(s).first?.title ?? "Genre" }

    static func decades(_ s: DiscoverSnapshot) -> [(id: String?, title: String)] {
        let all = s.decades ?? []
        return all.isEmpty ? [] : [(nil, "Any year")] + all.map { ($0, $0) }
    }
    static func decadeTitle(_ s: DiscoverSnapshot) -> String { s.decade ?? "Any year" }

    /// Clear shows with a genre, a decade or a service set, never for the sort alone.
    static func filtered(_ s: DiscoverSnapshot) -> Bool { s.genre != nil || s.decade != nil || s.platform != nil }

    /// The footer, as the web words it: the size, the scope beside the sort, what did not answer.
    static func scope(_ s: DiscoverSnapshot) -> String? {
        guard !s.items.isEmpty else { return nil }
        var parts: [String] = []
        let titles = s.scope?.titles ?? s.items.count
        parts.append("\(titles) \(titles == 1 ? "title" : "titles")")
        if let note = s.scope?.note, let label = (s.sorts ?? []).first(where: { $0.id == s.sort })?.label { parts.append("\(label), \(note)") }
        if let looked = s.scope?.looked { parts.append("Looked through the first \(looked)") }
        if let n = s.scope?.pending, n > 0 { parts.append("\(n) \(n == 1 ? "service" : "services") still loading") }
        if let n = s.scope?.failed, n > 0 { parts.append("\(n) \(n == 1 ? "service" : "services") did not answer") }
        return parts.joined(separator: " · ")
    }

    /// On the poster's corner: the value the list is ordered by, the IMDb score for the IMDb sort (even with the
    /// cover's band, which can be weeks old) and the year for the date sorts; nil for the others.
    static func corner(_ sort: String?, _ tile: Tile) -> String? {
        switch sort {
        case "imdb": tile.ratings?.imdb.map { "IMDb \($0)" }
        case "newest", "oldest": tile.releaseInfo
        default: nil
        }
    }

    /// The empty line: what was asked ("No Drama on Netflix right now.").
    static func emptyLine(_ s: DiscoverSnapshot) -> String {
        let service = (s.platforms ?? []).first { $0.id == s.platform }?.name
        let decade = s.decade.map { " from the \($0)" } ?? ""
        return "No \(s.genre ?? "titles")\(decade)\(service.map { " on \($0)" } ?? "") right now."
    }
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

    /// The preview's Library state for its own item: nil hides Add to Library (no details yet, details
    /// for another item, or a profile whose Library cannot be set).
    static func library(_ tile: Tile, _ details: ItemDetails?) -> Bool? {
        guard let details, details.id == tile.id, details.librarySettable == true else { return nil }
        return details.inLibrary ?? false
    }

    /// The state line under the filters, if any.
    static func state(_ s: DiscoverSnapshot) -> String? {
        switch s.state {
        case .failed: failed
        case .end where s.items.isEmpty: s.sorts == nil ? empty : emptyLine(s)
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

/// The type filter and sort a Library shows. The core keeps them per open view, so a reopened one
/// starts at `fresh`.
struct LibraryChoice: Equatable {
    let kind: String?
    let sort: String
    static let fresh = LibraryChoice(kind: nil, sort: "recent")
}

extension LibrarySnapshot {
    var choice: LibraryChoice { LibraryChoice(kind: kind, sort: sort) }
}

/// A command that sets the filter or the sort.
enum LibraryChange: Equatable {
    case filter(String?)
    case sort(String)

    var fields: [String: any Sendable] {
        switch self {
        case .filter(let kind): ["type": "setFilter", "kind": kind.map { $0 as any Sendable } ?? (NSNull() as any Sendable)]
        case .sort(let sort): ["type": "setSort", "sort": sort]
        }
    }
}

/// What puts a reopened Library, now showing `now`, back to the choice the viewer had.
func libraryRestore(wanted: LibraryChoice, now: LibraryChoice) -> [LibraryChange] {
    (wanted.kind == now.kind ? [] : [.filter(wanted.kind)]) + (wanted.sort == now.sort ? [] : [.sort(wanted.sort)])
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
        if addons.isEmpty { return "None installed yet." }
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

    /// What the card shows under the name: that it runs here, else nothing (the host
    /// waits in the card's actions, behind Details: never a raw host on the card).
    var hostLine: String? { builtIn == true ? "Built into Momotus, runs on this device" : nil }
}

/// Find addons: Cinemeta's two lists, Official then Community, then the lists
/// of addons the user installed (plans/core-protocol.md,
/// plans/plugins-and-marketplace.md 7.1).
struct AddonCatalogSnapshot: Decodable, Hashable, Sendable {
    let lists: [AddonList]
}

struct AddonList: Decodable, Hashable, Sendable {
    /// "official" or "community" for Cinemeta's lists; `addon-<hash>:<id>`
    /// for a list an installed addon publishes.
    let id: String
    let name: String
    /// The installed addon's host a list comes from ("From {host}"); nil for
    /// Cinemeta's two.
    var host: String?
    let state: ListState
    let entries: [ListedAddon]
    /// The list holds more than it shows; nil from an older core.
    var more: Bool?
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

/// The still first run frames its question steps with (`firstRunStill`): a title's backdrop, its logo
/// when it has one, and its name. The core answers null when none came within its budget: the frame stays black.
struct FirstRunStill: Decodable, Hashable, Sendable {
    let background: String
    let logo: String?
    let name: String
}

/// Menu (Back) at the top of a TV place, as on Android: a place other than
/// Home puts focus on the bar, then goes to Home, Home scrolled down goes back to its top row, and only
/// then does the app exit. A pushed page pops first; that is the stack's.
enum TVBack: Equatable {
    case toBar
    case goHome
    case toTop
    case exit

    /// `inBar`: focus is on the top bar. In another place's content, Menu first puts it there, as on Apple TV.
    static func step(atHome: Bool, scrolledDown: Bool, inBar: Bool) -> TVBack {
        guard atHome else { return inBar ? .goHome : .toBar }
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
    /// The second subtitle line (plans/dual-subtitles.md 3.1): "audio" for the spoken language, else a
    /// language tag; nil for none, and from an older core.
    let secondSubtitle: String?
    /// How two subtitle lines draw; nil from an older core (read it as `.stacked`).
    let subtitleLayout: SubtitleLayout?
    /// The second line shows only while paused; nil from an older core (read it as false).
    let secondOnPause: Bool?
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

/// How the second subtitle line draws beside the first (plans/dual-subtitles.md 4).
enum SubtitleLayout: String, Codable, Hashable, Sendable, CaseIterable {
    case stacked, split, oneBox
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

    /// Nothing to list and nothing still loading: why, said for no series and for no dates; nil otherwise.
    var emptyWords: String? {
        guard days.isEmpty, pending == 0 else { return nil }
        return series == 0
            ? "No series in your library yet. Series you add show their episodes here."
            : "Nothing airs from a week ago to a month ahead for the series in your library."
    }

    /// The empty page's way on, as the web and Android offer it: Discover.
    static let findSeries = "Find series to follow"
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

extension CalendarSeries {
    /// A series that could not load, on its own line: "Could not load Severance (timed out)." (Android's words).
    var failedWords: String { "Could not load \(reason.map { "\(name) (\($0))" } ?? name)." }
    /// Its own Try again: the core fetches this series alone again.
    var retryCommand: [String: String] { ["type": "retry", "itemId": itemId] }
    /// What VoiceOver says for that Try again.
    var retrySpoken: String { "Try again for \(name)" }
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
