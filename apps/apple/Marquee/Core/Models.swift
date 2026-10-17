// View DTOs from the core, mirroring apps/web/src/core/generated/*.ts
// (generated from mq-runtime with ts-rs). Field names match the JSON keys.

struct Tile: Decodable, Hashable, Sendable {
    let id: String
    let kind: String
    let name: String
    let poster: String?
    let background: String?
    let logo: String?
    let posterShape: String
    let releaseInfo: String?
    /// 0.0 to 1.0 for items in progress.
    let progress: Double?
    /// Video to resume for Continue Watching tiles.
    let videoId: String?
}

struct Hero: Decodable, Hashable, Sendable {
    let tile: Tile
    /// One line telling the user why this title is here.
    let reason: String
}

struct HomeHeader: Decodable, Hashable, Sendable {
    let hero: Hero?
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
    let background: String?
    let logo: String?
    let releaseInfo: String?
    let runtime: String?
    let genres: [String]
    let cast: [String]
    let directors: [String]
    let rating: String?
    let source: String
}

struct PlayTarget: Decodable, Hashable, Sendable {
    let videoId: String
    let label: String
    let resumeMs: Int64?
}

struct TitleHeader: Decodable, Hashable, Sendable {
    let state: TitleState
    let details: TitleDetails?
    let inLibrary: Bool
    let seasons: [Int]
    let selectedSeason: Int?
    /// Episode (or the movie itself) the primary button resumes or starts.
    let playTarget: PlayTarget?
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
}

struct TitleSnapshot: Decodable, Sendable {
    let header: TitleHeader
    let episodes: [Episode]
}

enum TitleUpdate: Decodable, Sendable {
    case header(TitleHeader)
    case episodes([ListDiff<Episode>])

    private enum Key: String, CodingKey { case kind, header, diffs }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        switch try c.decode(String.self, forKey: .kind) {
        case "header": self = .header(try c.decode(TitleHeader.self, forKey: .header))
        case "episodes": self = .episodes(try c.decode([ListDiff<Episode>].self, forKey: .diffs))
        case let kind:
            throw DecodingError.dataCorruptedError(forKey: .kind, in: c, debugDescription: "unknown title update \(kind)")
        }
    }
}
