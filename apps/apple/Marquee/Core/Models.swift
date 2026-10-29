// View DTOs from the core, mirroring apps/web/src/core/generated/*.ts
// (generated from mq-runtime with ts-rs). Field names match the JSON keys.

struct Tile: Decodable, Hashable, Sendable {
    let id: String
    let kind: String
    let name: String
    /// For a poster of an IMDb title, RPDB's cover with the IMDb, Metacritic and Rotten Tomatoes band drawn in;
    /// otherwise the addon's poster.
    let poster: String?
    /// The addon's own poster, for when `poster` is RPDB's and does not load (an id RPDB does not know answers
    /// 404); nil when `poster` is the addon's own already. Older cores send none.
    let posterFallback: String?
    let background: String?
    let logo: String?
    let posterShape: String
    let releaseInfo: String?
    /// 0.0 to 1.0 for items in progress.
    let progress: Double?
    /// Video to resume for Continue Watching tiles.
    let videoId: String?
    /// The IMDb rating the addon gave, as written ("8.4").
    let rating: String?
    /// The scores the core gives (IMDb's is the one text shows). Optional here
    /// only so replies recorded before the field existed still decode.
    let ratings: Ratings?
    /// A Top 10 tile's place, 1 to 10 (`HomeRow.ranked`); nil everywhere else and in older replies.
    var rank: Int? = nil
    /// A Top 10 tile's first genre.
    var genre: String? = nil

    /// The addon's own poster: what goes anywhere but a cover (a backdrop stand-in, the player, the core's
    /// play target), as a ratings band belongs on a cover only.
    var plainPoster: String? { posterFallback ?? poster }
}

/// A title's IMDb score, written as shown ("8.8"), from the addon's own rating; null when it gave none.
/// Rotten Tomatoes and Metacritic show only in the covers' band (RPDB's), never as text.
struct Ratings: Decodable, Hashable, Sendable {
    let imdb: String?
}

/// A title's score as plain text where text shows one (the hero, the title page, what VoiceOver says): IMDb's
/// number alone, "IMDb 8.8", as Stremio shows it; the covers carry the rest in RPDB's band (the owner,
/// 2026-10-23). No slot for a missing score, and no brand's logo (plans/research/ratings.md).
func ratingParts(_ ratings: Ratings?) -> [String] {
    [ratings?.imdb.map { "IMDb \($0)" }].compactMap { $0 }
}

/// One title's kind in words ("Movie"), for the kinds Marquee knows (the core's `kind_label`); nil for any
/// other type, which is never shown as is: an addon's own type is often its name.
func kindWord(_ kind: String) -> String? {
    switch kind {
    case "movie": "Movie"
    case "series": "Series"
    case "channel": "Channel"
    case "tv": "Live TV"
    case "anime": "Anime"
    default: nil
    }
}

/// What VoiceOver says for a poster: its name and caption, then the score ("Inception, 2010, IMDb 8.8").
func posterLabel(_ parts: [String?], _ ratings: Ratings?) -> String {
    (parts.compactMap { $0 } + ratingParts(ratings)).joined(separator: ", ")
}

struct Hero: Decodable, Hashable, Sendable {
    let tile: Tile
    /// The row it was taken from. Optional here only so replies recorded
    /// before the field existed still decode.
    let rowId: String?
    /// One line telling the user why this title is here, never with the
    /// addon's name: "#3 in Popular Movies" from a ranked catalog, else the
    /// row's title.
    let reason: String
    let description: String?
    /// At most three, in the addon's order. Optional here only so replies
    /// recorded before the field existed still decode.
    let genres: [String]?
    let runtime: String?
    /// A YouTube video id to play muted in the hero.
    let trailer: String?
    /// What Play plays, as the title page's primary button ("Play S1:E1", "Resume S2:E3"); nil gives Details alone.
    /// Optional here so replies recorded before the field existed still decode.
    let play: PlayTarget?
    /// The page's meta is still being read, so Play may come: its place is kept meanwhile. Optional for older replies.
    let playPending: Bool?

    /// Play's place is held, hidden, so a Play landing late never pushes Details.
    var holdsPlaySlot: Bool { play == nil && playPending == true }
}

struct HomeHeader: Decodable, Hashable, Sendable {
    /// The hero carousel's pages, in order; none in a reply recorded before they existed (review r1, S10).
    let heroes: [Hero]
    /// The first page, which Home shows until the carousel lands.
    var hero: Hero? { heroes.first }
    /// No network, and downloads to play (3.5); the shell's banner reads the keeper's view, not this.
    var offline: HomeOffline? = nil

    init(heroes: [Hero], offline: HomeOffline? = nil) {
        self.heroes = heroes
        self.offline = offline
    }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        heroes = try c.decodeIfPresent([Hero].self, forKey: .heroes) ?? []
        offline = try c.decodeIfPresent(HomeOffline.self, forKey: .offline)
    }

    private enum CodingKeys: String, CodingKey { case heroes, offline }
}

enum RowState: Decodable, Hashable, Sendable {
    case loading
    case slow
    case ready
    case empty
    case failed(reason: String)

    private enum Key: String, CodingKey { case state, reason }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        switch try c.decode(String.self, forKey: .state) {
        case "loading": self = .loading
        case "slow": self = .slow
        case "ready": self = .ready
        case "empty": self = .empty
        case "failed": self = .failed(reason: try c.decode(String.self, forKey: .reason))
        case let state:
            throw DecodingError.dataCorruptedError(forKey: .state, in: c, debugDescription: "unknown row state \(state)")
        }
    }
}

struct HomeRow: Decodable, Hashable, Sendable {
    let rowId: String
    let title: String
    let source: String
    let state: RowState
    let tiles: [Tile]
    /// A Top 10 row (the first ranked catalog of its kind); nil in replies recorded before it existed.
    var ranked: Bool? = nil
}

extension HomeRow {
    /// The Home view's command for this row's Try again (core `retryRow`).
    var retryCommand: [String: String] { ["type": "retryRow", "rowId": rowId] }

    // No addon's name on Home or Search, not even for two rows with the same title, and not in a
    // row's state (the owner, 2026-10-22; design/DECISION.md, Shelves). The Addons pages and
    // Settings > Home rows name it.

    /// Beside the title: "Slow to answer, still trying" for a slow row, else nothing (the web's words).
    var subtitle: String? { state == .slow ? "Slow to answer, still trying" : nil }

    /// What an empty or failed row says under its title.
    var stateWords: String? {
        switch state {
        case .empty: "Nothing in this row right now."
        case .failed: "This row did not load."
        case .loading, .slow, .ready: nil
        }
    }
}

extension [HomeRow] {
    /// Every lane ended with nothing to show and at least one failed, as
    /// Android's `allFailed`: Home says it could not load, with one Try
    /// again, instead of a failed row each. A lane still loading or holding
    /// tiles (even ones that came before a failure) keeps the rows.
    var allFailed: Bool {
        !isEmpty
            && !contains { !$0.tiles.isEmpty || $0.state == .loading || $0.state == .slow }
            && contains { if case .failed = $0.state { true } else { false } }
    }
}

extension [Tile] {
    /// Stable identities for a strip or a grid: kind and id, a repeat
    /// numbered. Keyed by position, a reorder (Continue Watching after a play)
    /// changed the title under the focused place instead of moving it.
    var keyed: [(key: String, tile: Tile)] {
        var seen: [String: Int] = [:]
        return map { tile in
            let base = "\(tile.kind):\(tile.id)"
            let repeats = seen[base, default: 0]
            seen[base] = repeats + 1
            return (repeats == 0 ? base : "\(base)#\(repeats)", tile)
        }
    }
}

/// Where TV focus enters Home, and where Menu brings it back, as the web and
/// Android have it: Continue Watching's first tile when that row leads, else
/// the hero's Play, else the first tile of the first row that has any (an
/// empty or failed first lane is skipped).
enum HomeStart: Equatable {
    case row(String)
    case hero

    static func target(rows: [HomeRow], hero: Bool) -> HomeStart? {
        if let first = rows.first, first.rowId == "continue-watching", !first.tiles.isEmpty {
            return .row(first.rowId)
        }
        if hero { return .hero }
        return rows.first { !$0.tiles.isEmpty }.map { .row($0.rowId) }
    }
}

struct HomeSnapshot: Decodable, Sendable {
    let header: HomeHeader
    let rows: [HomeRow]
}

enum HomeUpdate: Decodable, Sendable {
    case header(HomeHeader)
    case rows([ListDiff<HomeRow>])

    private enum Key: String, CodingKey { case kind, header, diffs }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        switch try c.decode(String.self, forKey: .kind) {
        case "header": self = .header(try c.decode(HomeHeader.self, forKey: .header))
        case "rows": self = .rows(try c.decode([ListDiff<HomeRow>].self, forKey: .diffs))
        case let kind:
            throw DecodingError.dataCorruptedError(forKey: .kind, in: c, debugDescription: "unknown home update \(kind)")
        }
    }
}

enum TitleState: Decodable, Hashable, Sendable {
    case loading
    case ready
    case failed(reason: String)

    private enum Key: String, CodingKey { case state, reason }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        switch try c.decode(String.self, forKey: .state) {
        case "loading": self = .loading
        case "ready": self = .ready
        case "failed": self = .failed(reason: try c.decode(String.self, forKey: .reason))
        case let state:
            throw DecodingError.dataCorruptedError(forKey: .state, in: c, debugDescription: "unknown title state \(state)")
        }
    }
}

struct TitleDetails: Decodable, Hashable, Sendable {
    let id: String
    let kind: String
    let name: String
    let description: String?
    let poster: String?
    /// RPDB's cover with the IMDb, Metacritic and Rotten Tomatoes band drawn in, as the title's tiles show it;
    /// nil with no RPDB key or no IMDb id (then `poster`, and `poster` too when this one fails to load).
    let ratedPoster: String?
    /// The poster at the size the iPhone's title page draws it (metahub's large); nil from an older core.
    let cover: String?
    /// The cover is known to print the title (metahub's): no logo with it. Nil from an older core.
    let coverTitled: Bool?
    let background: String?
    let logo: String?
    let releaseInfo: String?
    let runtime: String?
    let genres: [String]
    /// From the meta's Cast links, each with TMDB's headshot and role once its credits land.
    let cast: [CastMember]
    let directors: [String]
    /// From the meta's Writers links; an older core sends none.
    let writers: [String]?
    let rating: String?
    /// As on tiles; optional only so older recordings decode.
    let ratings: Ratings?
    /// The title's IMDb page, where its IMDb score links (not on TV).
    let imdbUrl: String?
    /// TMDB's and Trakt's numbers; nil from an older core, which sends none.
    let scores: TitleScores?
    let source: String
    /// Trailers the metadata names (YouTube), first is the main one.
    let trailers: [Trailer]
}

/// One source's number on the title page: `pending` while an answer can still come (keep the slot's width),
/// `shown` with the number as written ("87%"), `none` when there is none (draw nothing, take no space).
enum Slot: Decodable, Hashable, Sendable {
    case pending
    /// The number as written, and the title's page on the service (from checked ids; nil on a kids profile).
    case shown(String, url: String?)
    case none

    private enum Key: String, CodingKey { case state, text, url }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        switch try c.decode(String.self, forKey: .state) {
        case "pending": self = .pending
        case "shown": self = .shown(try c.decode(String.self, forKey: .text), url: try c.decodeIfPresent(String.self, forKey: .url))
        default: self = .none
        }
    }
}

/// TMDB's user score and Trakt's community score, as percentages, beside the addon's IMDb score.
struct TitleScores: Decodable, Hashable, Sendable {
    let tmdb: Slot
    let trakt: Slot
}

/// One of the title's cast.
struct CastMember: Decodable, Hashable, Sendable {
    let name: String
    /// The role TMDB names; nil until its credits land, or when it names none.
    var character: String? = nil
    /// TMDB's headshot (185 px wide); nil until its credits land, or when it has none: draw the initials.
    var photo: String? = nil
}

struct Trailer: Decodable, Hashable, Sendable {
    let title: String
    let youtubeId: String
    /// The watch page, for handing the trailer to a browser or the YouTube app.
    let url: String
}

struct PlayTarget: Decodable, Hashable, Sendable {
    let videoId: String
    let label: String
    let resumeMs: Int64?
    /// What the file has left, from the stored duration; nil before it starts.
    let leftMs: Int64?
}

struct TitleHeader: Decodable, Hashable, Sendable {
    let state: TitleState
    let details: TitleDetails?
    let inLibrary: Bool
    let seasons: [Int]
    let selectedSeason: Int?
    /// Episode (or the movie itself) the primary button resumes or starts;
    /// nil while the next episode is not out yet (see `nextAirs`).
    let playTarget: PlayTarget?
    /// The day the next episode comes out ("YYYY-MM-DD", as `Episode.released`)
    /// when it is not out yet: say "Next episode airs <date>" instead of Play.
    let nextAirs: String?
    /// A movie the user watched (episodes carry their own flag); toggled with
    /// `setWatched` on the movie's id.
    let watched: Bool
    /// False on a kids profile, whose Library is the owner's list (`setInLibrary` is refused there); an older
    /// core sends nothing, which counts as true.
    let librarySettable: Bool?
    /// False on a kids profile, which has no Discover: genres show as plain text. An older core sends
    /// nothing, which counts as true.
    let genresLinked: Bool?
    /// A movie's download on this device (plans/offline-downloads.md 3.1).
    var download: DownloadBadge? = nil
    /// The selected season's downloads and every regular season's, for the series menu.
    var seasonDownload: GroupDownload? = nil
    var seriesDownload: GroupDownload? = nil
    /// The meta could not be read: the page shows what the downloads kept (4.18).
    var offline: Bool? = nil

    /// Whether the title offers Add to Library.
    var canSetLibrary: Bool { librarySettable ?? true }
    /// Whether a genre opens Discover.
    var linksGenres: Bool { genresLinked ?? true }
}

struct Episode: Decodable, Hashable, Sendable {
    let videoId: String
    let season: Int
    let episode: Int
    let title: String
    let overview: String?
    let thumbnail: String?
    let released: String?
    let watched: Bool
    let progress: Double?
    let leftMs: Int64?
    /// Its download on this device.
    var download: DownloadBadge? = nil
}

struct TitleSnapshot: Decodable, Sendable {
    let header: TitleHeader
    let episodes: [Episode]
    /// "More like this"; empty from an older core.
    let similar: [Tile]

    private enum Key: String, CodingKey { case header, episodes, similar }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        header = try c.decode(TitleHeader.self, forKey: .header)
        episodes = try c.decode([Episode].self, forKey: .episodes)
        similar = try c.decodeIfPresent([Tile].self, forKey: .similar) ?? []
    }
}

enum TitleUpdate: Decodable, Sendable {
    case header(TitleHeader)
    case episodes([ListDiff<Episode>])
    /// "More like this", whole.
    case similar([Tile])
    /// A kind a newer core sends: ignored, so the page keeps working.
    case other

    private enum Key: String, CodingKey { case kind, header, diffs, tiles }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        switch try c.decode(String.self, forKey: .kind) {
        case "header": self = .header(try c.decode(TitleHeader.self, forKey: .header))
        case "episodes": self = .episodes(try c.decode([ListDiff<Episode>].self, forKey: .diffs))
        case "similar": self = .similar(try c.decode([Tile].self, forKey: .tiles))
        default: self = .other
        }
    }
}
