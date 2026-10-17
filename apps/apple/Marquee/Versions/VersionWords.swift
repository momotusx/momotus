import SwiftUI

/// Plain words for what the core says about a version. Never emoji, never codes.
enum VersionWords {
    static func size(_ bytes: Int64?) -> String? {
        bytes.map { ByteCountFormatter.string(fromByteCount: $0, countStyle: .file) }
    }

    static func status(_ status: VersionStatus) -> String {
        switch status {
        case .ready(let service): service.map { "Ready on \($0)" } ?? "Ready"
        case .torrent(let seeds): seeds.map { "Torrent, \($0) seeds" } ?? "Torrent"
        case .needsDownload(let service): service.map { "Downloads on \($0)" } ?? "Needs download"
        case .direct: "Direct link"
        }
    }

    static func statusColor(_ status: VersionStatus) -> Color {
        switch status {
        case .ready, .direct: Theme.Palette.ok
        case .torrent: Theme.Palette.warn
        case .needsDownload: Theme.Palette.muted
        }
    }

    static func reason(_ reason: String) -> String {
        switch reason {
        case "readyNow": "Ready now"
        case "playsDirectly": "Plays directly on this device"
        case "matchesScreen": "Matches your screen"
        case "sameReleaseAsLast": "Same release as last time"
        case "yourLanguage": "In your language"
        case "smallerFaster": "Smaller and faster"
        default: reason
        }
    }

    static func group(_ group: String) -> String {
        switch group {
        case "best": "Best quality"
        case "smallerFaster": "Smaller and faster"
        case "otherLanguages": "Other languages"
        case "needsDownload": "Needs download"
        default: group
        }
    }

    static let groupOrder = ["best", "smallerFaster", "otherLanguages", "needsDownload"]

    /// "10 are for other titles, 32 cannot play on this device"
    static func hidden(_ summary: HiddenSummary) -> String {
        summary.byReason.map { item in
            let one = item.count == 1
            let what: String
            switch item.reason {
            case "wrongTitle": what = one ? "is for another title" : "are for other titles"
            case "wrongEpisode": what = one ? "is for another episode" : "are for other episodes"
            case "excluded": what = one ? "is excluded by your settings" : "are excluded by your settings"
            case "tooLarge": what = one ? "is too large" : "are too large"
            case "unplayable": what = "cannot play on this device"
            case "unsupportedSource": what = one ? "needs a source this app cannot open yet" : "need a source this app cannot open yet"
            default: what = item.reason
            }
            return "\(item.count) \(what)"
        }
        .joined(separator: ", ")
    }

    /// The line under Play: "1080p  HDR10  2.4 GB  Ready on Real-Debrid".
    static func line(_ row: VersionRow) -> String {
        ([row.quality == "Unknown" ? "Quality unknown" : row.quality] + row.features + [size(row.sizeBytes)].compactMap { $0 })
            .joined(separator: "  ")
    }
}
