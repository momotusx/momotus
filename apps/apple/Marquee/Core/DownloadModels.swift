import Foundation

// The `downloads` view and the download fields of other views (plans/core-protocol.md, "downloads";
// plans/offline-downloads.md 4.15), by hand as every view's models are.

/// Where a download stands; a state from a newer core reads as `unknown`.
enum DownloadState: String, Decodable, Hashable, Sendable {
    case queued, running, paused, waiting, done, failed, missing, unknown

    init(from decoder: any Decoder) throws {
        self = DownloadState(rawValue: try decoder.singleValueContainer().decode(String.self)) ?? .unknown
    }
}

/// Why a download waits (`wifi` to `app`) or failed (`noVersion` to `notVideo`).
enum DownloadReason: String, Decodable, Hashable, Sendable {
    case wifi, lowData, space, network, playing, app, noVersion, source, retries, notVideo, unknown

    init(from decoder: any Decoder) throws {
        self = DownloadReason(rawValue: try decoder.singleValueContainer().decode(String.self)) ?? .unknown
    }
}

/// The download control's state for one video (3.1).
struct DownloadBadge: Decodable, Hashable, Sendable {
    let id: String
    let state: DownloadState
    let reason: DownloadReason?
    /// Released onto mobile data by the user.
    let mobileData: Bool
    /// 0 to 1, when the size is known.
    let progress: Double?
    let bytesTotal: Int64?
}

/// A season's or a series' downloads: its aired episodes, how many have a download (not failed or
/// missing), and how many of those are on their way.
struct GroupDownload: Decodable, Hashable, Sendable {
    let seasons: Int
    let episodes: Int
    let downloaded: Int
    let active: Int
}

/// One download as the screens show it.
struct DownloadRow: Decodable, Hashable, Sendable, Identifiable {
    let id: String
    let kind: String
    let itemId: String
    let videoId: String
    let season: Int?
    let episode: Int?
    let name: String
    let episodeTitle: String?
    /// Loopback URLs of the copies the download keeps.
    let poster: String?
    let still: String?
    let runtime: String?
    let released: String?
    let state: DownloadState
    let reason: DownloadReason?
    let mobileData: Bool
    let bytesDone: Int64
    let bytesTotal: Int64?
    let speedBps: Int64?
    let etaSecs: Int64?
    /// "1080p", from the version kept.
    let quality: String?
    /// The languages of the subtitles kept with it, as tags (nil from an older core).
    var subtitles: [String]? = nil
}

struct DownloadTitle: Decodable, Hashable, Sendable, Identifiable {
    let itemId: String
    let kind: String
    let name: String
    let poster: String?
    let count: Int
    let bytes: Int64
    let rows: [DownloadRow]

    var id: String { itemId }
}

struct DownloadStorage: Decodable, Hashable, Sendable {
    /// What downloads hold on disk.
    let downloadsBytes: Int64
    let freeBytes: Int64?
    let neededBytes: Int64?
}

/// The device's settings (3.7).
struct DownloadSettings: Decodable, Hashable, Sendable {
    let wifiOnly: Bool
    let smart: Bool
}

/// What the platform keeper shows and whether it keeps the app running.
struct DownloadActivity: Decodable, Hashable, Sendable {
    let running: Int
    let queued: Int
    let bytesDone: Int64
    let bytesTotal: Int64
    /// Waiting for Wi-Fi, a connection, room or the app: still left to do, so the keeper holds on for them
    /// (nil from an older core).
    var waiting: Int? = nil
    /// One left to do was let onto mobile data.
    var mobileData: Bool? = nil
    /// The title the first running download belongs to; none on a locked session.
    let title: String?
}

struct DownloadsSnapshot: Decodable, Hashable, Sendable {
    let storage: DownloadStorage
    let settings: DownloadSettings
    let active: DownloadActivity
    /// Queued, running, paused and waiting, in queue order.
    let downloading: [DownloadRow]
    /// Done, failed and missing, by title, newest first.
    let titles: [DownloadTitle]
    /// A locked session's counts-only view: no titles, nothing done or failed to read (nil from an older core).
    var locked: Bool? = nil
    /// The profile these rows are for (nil from an older core).
    var profile: String? = nil
}

/// Home with no network and downloads to play (3.5).
struct HomeOffline: Decodable, Hashable, Sendable {
    let downloads: Int
}
