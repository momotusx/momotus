// Versions and playback DTOs from the core, mirroring
// apps/web/src/core/generated/{VersionsSnapshot,VersionRow,PlaybackSnapshot,...}.ts.
// Enumerations the UI only displays (group, reason, hide reason) stay strings
// so a new value from the core never breaks decoding.

struct VersionsSnapshot: Decodable, Hashable, Sendable {
    let recommended: VersionRow?
    let rows: [VersionRow]
    let hidden: HiddenSummary
    let pendingAddons: [String]
    let slowAddons: [String]
    let failedAddons: [String]
    let done: Bool
    let frozen: Bool
}

struct VersionRow: Decodable, Hashable, Sendable, Identifiable {
    /// What `playback` takes.
    let key: String
    /// "4K", "1080p", "720p", "SD" or "Unknown".
    let quality: String
    /// Words, never emoji: "Dolby Vision", "HDR10", "Atmos", "5.1", "Remux".
    let features: [String]
    let sizeBytes: Int64?
    let status: VersionStatus
    /// Service ("Real-Debrid") or addon name.
    let via: String
    let releaseName: String
    let languages: [String]
    /// "readyNow", "playsDirectly", "matchesScreen", "sameReleaseAsLast", "yourLanguage", "smallerFaster".
    let reasons: [String]
    /// "best", "smallerFaster", "otherLanguages" or "needsDownload".
    let group: String
    let failoverCount: Int

    var id: String { key }
}

enum VersionStatus: Decodable, Hashable, Sendable {
    case ready(service: String?)
    case torrent(seeds: Int?)
    case needsDownload(service: String?)
    case direct

    private enum Key: String, CodingKey { case kind, service, seeds }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        switch try c.decode(String.self, forKey: .kind) {
        case "ready": self = .ready(service: try c.decodeIfPresent(String.self, forKey: .service))
        case "torrent": self = .torrent(seeds: try c.decodeIfPresent(Int.self, forKey: .seeds))
        case "needsDownload": self = .needsDownload(service: try c.decodeIfPresent(String.self, forKey: .service))
        case "direct": self = .direct
        case let kind:
            throw DecodingError.dataCorruptedError(forKey: .kind, in: c, debugDescription: "unknown status \(kind)")
        }
    }
}

struct HiddenSummary: Decodable, Hashable, Sendable {
    let count: Int
    /// Hide reason ("wrongTitle", "unplayable", ...) and how many it hid.
    let byReason: [ReasonCount]

    struct ReasonCount: Decodable, Hashable, Sendable {
        let reason: String
        let count: Int

        /// The core sends each pair as a two-element array.
        init(from decoder: any Decoder) throws {
            var c = try decoder.unkeyedContainer()
            reason = try c.decode(String.self)
            count = try c.decode(Int.self)
        }
    }
}

struct PlaybackSnapshot: Decodable, Hashable, Sendable {
    /// "resolving", "ready" or "failed".
    let phase: String
    /// Loopback URL for the player, once `ready`.
    let url: String?
    /// The version that is playing: the picked key or one of its failovers.
    let key: String?
    let filename: String?
    let sizeBytes: Int64?
    /// Where to start, from saved progress.
    let resumeMs: Int64?
    /// Sources that failed, in order, each with a plain reason.
    let attempts: [Attempt]
    /// External subtitles as loopback WebVTT URLs, growing while pending.
    let subtitles: [SubtitleTrack]
    let subtitlesPending: Bool
    /// The next released episode, for an Up Next card.
    let next: NextEpisode?
    /// Pass as `bingeGroup` when opening Versions for the next episode.
    let bingeGroup: String?
}

struct Attempt: Decodable, Hashable, Sendable {
    let key: String
    let via: String
    let error: String
}

struct SubtitleTrack: Decodable, Hashable, Sendable, Identifiable {
    let url: String
    /// As the addon gives it, usually ISO 639-2 ("eng").
    let lang: String
    let label: String?
    /// Addon name.
    let source: String
    let forced: Bool
    let hearingImpaired: Bool

    var id: String { url }
}

struct NextEpisode: Decodable, Hashable, Sendable {
    let videoId: String
    let title: String
    let season: Int
    let episode: Int
    let thumbnail: String?
}
