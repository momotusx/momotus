import Foundation

/// The download control's glyph (plans/offline-downloads.md 3.1).
enum DownloadGlyph: Hashable, Sendable {
    /// `arrow-down-to-line`: nothing downloaded yet.
    case download
    /// The filled square in the ring: running.
    case stop
    case pause
    case wifiOff
    case clock
    case check
    case alert

    /// Its Lucide icon in the catalog.
    var asset: String {
        switch self {
        case .download: "arrow-down-to-line"
        case .stop: "square-fill"
        case .pause: "pause-fill"
        case .wifiOff: "wifi-off"
        case .clock: "clock"
        case .check: "circle-check"
        case .alert: "circle-alert"
        }
    }
}

/// One item of a download's menu.
enum DownloadItem: String, CaseIterable, Hashable, Sendable {
    case start, choose, pause, resume, cancel, delete, play, retry, another, mobileData, again, remove

    var label: String {
        switch self {
        case .start: "Download"
        case .choose: "Choose a version to download"
        case .pause: "Pause download"
        case .resume: "Resume download"
        case .cancel: "Cancel download"
        case .delete: "Delete download"
        case .play: "Play"
        case .retry: "Try again"
        case .another: "Choose another version"
        case .mobileData: "Download now on mobile data"
        case .again: "Download again"
        case .remove: "Remove"
        }
    }

    /// Its Lucide icon, as the web's.
    var glyph: String {
        switch self {
        case .start, .again: "arrow-down-to-line"
        case .choose, .another: "layers"
        case .pause: "pause"
        case .resume, .play: "play"
        case .cancel: "x"
        case .delete, .remove: "trash"
        case .retry: "rotate-cw"
        case .mobileData: "smartphone"
        }
    }
}

/// The op an item of an existing download sends.
enum DownloadOp: String, Hashable, Sendable {
    case pause = "downloadPause"
    case resume = "downloadResume"
    case remove = "downloadRemove"
    case retry = "downloadRetry"
}

/// What the control shows for one video. `label` is the word beside it where labels show ("45%"),
/// `spoken` the state's word for VoiceOver ("Downloading"), `value` the ring's "45 percent", `ring` its
/// fill (nil with no ring), `detail` the reason in full for the menu, `items` the menu a tap opens
/// (empty: a tap starts the download), `hint` what that tap does.
struct DownloadControl: Hashable, Sendable {
    let glyph: DownloadGlyph
    let label: String
    let spoken: String
    let value: String?
    let ring: Double?
    let detail: String?
    let items: [DownloadItem]
    var hint: String?

    init(_ glyph: DownloadGlyph, _ label: String, _ spoken: String, value: String? = nil, ring: Double? = nil, detail: String? = nil, items: [DownloadItem] = []) {
        (self.glyph, self.label, self.spoken, self.value, self.ring, self.detail, self.items) = (glyph, label, spoken, value, ring, detail, items)
        hint = items.isEmpty ? nil : DownloadCopy.menuHint
    }
}

/// One item of a series' Download menu.
struct GroupItem: Hashable, Sendable {
    enum Act: Hashable, Sendable {
        case downloadSeason, downloadSeries, deleteSeason
        /// A line that says where the season stands; pressing it does nothing.
        case status
    }

    let label: String
    let act: Act
}

/// Where downloads show: never on a TV form (no reference app offers them there, and tvOS storage is
/// purgeable; plans/offline-downloads.md 2, decision 14), and a kids profile only sees and plays them.
enum DownloadGate {
    /// Any download UI: the Library entry, the title's control and items, Settings, the offline banner.
    static func shows(tv: Bool) -> Bool { !tv }

    /// The control and its items (add, pause, delete): not for a kids profile, whose ops are refused.
    static func controls(tv: Bool, kids: Bool) -> Bool { shows(tv: tv) && !kids }

    /// This build's form.
    static var tv: Bool {
        #if os(tvOS)
        true
        #else
        false
        #endif
    }
}

/// The words of downloads, equal to Android's DownloadCopy.kt and the web's lib/download.ts.
enum DownloadCopy {
    static let menuHint = "Shows download options"

    /// Where nothing is downloaded yet: a tap starts it, a press and hold offers the version.
    static let none = DownloadControl(.download, "Download", "Download")

    /// A video's control; nil is not downloaded.
    static func control(_ badge: DownloadBadge?) -> DownloadControl {
        guard let badge else { return none }
        let progress = badge.progress.map { min(1, max(0, $0)) }
        let percent = progress.map { String(Int(($0 * 100).rounded())) }
        let value = percent.map { "\($0) percent" }
        switch badge.state {
        case .queued:
            return DownloadControl(.clock, "Queued", "Queued", ring: 0, items: [.cancel])
        case .running:
            return DownloadControl(.stop, percent.map { "\($0)%" } ?? "Downloading", "Downloading", value: value, ring: progress ?? 0, items: [.pause, .cancel])
        case .paused:
            return DownloadControl(.pause, "Paused", "Paused", value: value, ring: progress ?? 0, items: [.resume, .cancel])
        case .waiting:
            let word = waiting(badge.reason)
            let release = badge.reason == .wifi || badge.reason == .lowData
            return DownloadControl(waitingGlyph(badge.reason), word, word, value: value, ring: progress ?? 0,
                                   detail: waitingDetail(badge.reason), items: release ? [.mobileData, .cancel] : [.cancel])
        case .done:
            return DownloadControl(.check, "Downloaded", "Downloaded", items: [.play, .delete])
        case .failed:
            return DownloadControl(.alert, "Couldn't download", "Couldn't download", detail: failure(badge.reason), items: [.retry, .another, .cancel])
        case .missing:
            return DownloadControl(.alert, "Download again", "Download again", detail: "The downloaded file is no longer on this device.", items: [.again, .remove])
        case .unknown:
            // A state from a newer core: it can still be cancelled.
            return DownloadControl(.clock, "Waiting", "Waiting", ring: 0, items: [.cancel])
        }
    }

    /// The op an item of an existing download sends, and whether it releases the download onto mobile
    /// data; nil for the items that go somewhere instead (Download, a version, Play).
    static func op(_ item: DownloadItem) -> (op: DownloadOp, mobileData: Bool)? {
        switch item {
        case .pause: (.pause, false)
        case .resume: (.resume, false)
        case .cancel, .delete, .remove: (.remove, false)
        case .retry, .again: (.retry, false)
        case .mobileData: (.retry, true)
        case .start, .choose, .another, .play: nil
        }
    }

    /// The waiting label by its reason. A wait for room is never a storage warning (the owner's rule):
    /// it reads "Waiting", quietly.
    static func waiting(_ reason: DownloadReason?) -> String {
        switch reason {
        case .wifi: "Waiting for Wi-Fi"
        case .lowData: "Waiting: Low Data Mode is on"
        case .network: "Waiting for a connection"
        case .playing: "Paused while you watch"
        case .app: "Opens again when you return to Momotus"
        default: "Waiting"
        }
    }

    private static func waitingGlyph(_ reason: DownloadReason?) -> DownloadGlyph {
        switch reason {
        case .wifi, .lowData, .network: .wifiOff
        default: .clock
        }
    }

    private static func waitingDetail(_ reason: DownloadReason?) -> String? {
        switch reason {
        case .wifi: "Download on Wi-Fi only is on, and this network uses mobile data."
        case .network: "It goes on by itself when the connection comes back."
        case .playing: "It goes on by itself when playback ends."
        default: nil
        }
    }

    /// Why a download failed, in full.
    static func failure(_ reason: DownloadReason?) -> String {
        switch reason {
        case .noVersion: "No version of this one can be downloaded"
        case .source: "The source stopped answering"
        case .retries: "The download was interrupted too many times"
        case .notVideo: "The file wasn't a video"
        default: "Something went wrong"
        }
    }

    /// A series' Download menu (3.2): the selected season's item, then every season's. Counts are aired
    /// regular episodes, from the core.
    static func seriesMenu(season: Int?, group: GroupDownload?, series: GroupDownload?) -> [GroupItem] {
        var out: [GroupItem] = []
        if let season, let group, group.episodes > 0 {
            let left = group.episodes - group.downloaded
            if group.active > 0 {
                out.append(GroupItem(label: "Downloading Season \(season)", act: .status))
            } else if left <= 0 {
                out.append(GroupItem(label: "Season \(season) downloaded", act: .status))
            } else {
                out.append(GroupItem(label: "Download Season \(season) (\(episodes(left)))", act: .downloadSeason))
            }
            if group.downloaded > 0 { out.append(GroupItem(label: "Delete Season \(season) downloads", act: .deleteSeason)) }
        }
        if let series, series.seasons > 1, series.downloaded < series.episodes {
            out.append(GroupItem(label: "Download every season (\(series.seasons) seasons, \(episodes(series.episodes)))", act: .downloadSeries))
        }
        return out
    }

    /// Whether a series menu item asks before it acts: deleting a season's downloads does, with the words of
    /// a series delete ("Delete 8 downloaded episodes of Severance?"), on every platform.
    static func asksFirst(_ act: GroupItem.Act) -> Bool { act == .deleteSeason }

    /// The series' action-row control: Download, or where its downloads stand; a tap always opens its menu.
    static func seriesControl(_ series: GroupDownload?) -> DownloadControl {
        var control: DownloadControl
        if series == nil || series?.downloaded == 0 {
            control = none
        } else if (series?.active ?? 0) > 0 {
            control = DownloadControl(.clock, "Downloading", "Downloading")
        } else {
            control = DownloadControl(.check, "Downloaded", "Downloaded")
        }
        control.hint = menuHint
        return control
    }

    static func episodes(_ n: Int) -> String { n == 1 ? "1 episode" : "\(n) episodes" }

    /// "Downloads use 12.4 GB · 48 GB free on this device".
    static func storage(_ storage: DownloadStorage) -> String {
        ["Downloads use \(StreamChoice.bytes(storage.downloadsBytes))", storage.freeBytes.map { "\(StreamChoice.bytes($0)) free on this device" }]
            .compactMap { $0 }.joined(separator: " \u{B7} ")
    }

    /// "1.2 of 2.1 GB": the first figure drops the unit both share.
    static func ofTotal(_ done: Int64, _ total: Int64) -> String {
        let (a, b) = (StreamChoice.bytes(done), StreamChoice.bytes(total))
        let unit = { (s: String) in s.split(separator: " ").last.map(String.init) ?? "" }
        guard unit(a) == unit(b), let space = a.lastIndex(of: " ") else { return "\(a) of \(b)" }
        return "\(a[..<space]) of \(b)"
    }

    /// "6 min left", "1 h 5 min left"; under a minute "Less than a minute left".
    static func left(_ seconds: Int64) -> String {
        if seconds < 60 { return "Less than a minute left" }
        let minutes = (seconds + 59) / 60
        return minutes < 60 ? "\(minutes) min left" : "\(minutes / 60) h \(minutes % 60) min left"
    }

    /// A row of the Downloading section: "1.2 of 2.1 GB · 6 min left", or the state's word where it
    /// does not run.
    static func progressLine(_ row: DownloadRow) -> String {
        let size: String? = if let total = row.bytesTotal, total > 0 { ofTotal(row.bytesDone, total) } else if row.bytesDone > 0 { StreamChoice.bytes(row.bytesDone) } else { nil }
        let state = row.state == .running ? row.etaSecs.map(left) : control(badge(row)).label
        return [size, state].compactMap { $0 }.joined(separator: " \u{B7} ")
    }

    /// A kept title's line: a film "1080p · 2.1 GB", a series "4 episodes · 7.8 GB".
    static func titleLine(_ title: DownloadTitle) -> String {
        title.kind == "movie"
            ? [title.rows.first?.quality, StreamChoice.bytes(title.bytes)].compactMap { $0 }.joined(separator: " \u{B7} ")
            : "\(episodes(title.count)) \u{B7} \(StreamChoice.bytes(title.bytes))"
    }

    /// An episode of a series page: "E3 Name".
    static func episodeName(_ row: DownloadRow) -> String {
        let name = [row.episode.map { "E\($0)" }, row.episodeTitle.flatMap { $0.trimmingCharacters(in: .whitespaces).isEmpty ? nil : $0 }]
            .compactMap { $0 }.joined(separator: " ")
        return name.isEmpty ? row.name : name
    }

    /// Under it: "52 min · 1.9 GB".
    static func episodeLine(_ row: DownloadRow) -> String {
        let size = row.bytesTotal ?? row.bytesDone
        return [row.runtime, size > 0 ? StreamChoice.bytes(size) : nil].compactMap { $0 }.joined(separator: " \u{B7} ")
    }

    /// A row's badge, for the control on the Downloads screen.
    static func badge(_ row: DownloadRow) -> DownloadBadge {
        DownloadBadge(
            id: row.id, state: row.state, reason: row.reason, mobileData: row.mobileData,
            progress: row.bytesTotal.flatMap { $0 > 0 ? Double(row.bytesDone) / Double($0) : nil }, bytesTotal: row.bytesTotal)
    }

    /// The control's VoiceOver label with what it is for: "Downloaded, Severance E3 In Perpetuity", so a list
    /// of rows is not a list of identical buttons (review r1 m8).
    static func spoken(_ control: DownloadControl, for name: String?) -> String {
        guard let name, !name.isEmpty else { return control.spoken }
        return "\(control.spoken), \(name)"
    }

    /// The subtitles kept with a download, by their English names in the core's order: "Subtitles:
    /// English, Turkish"; nil with none.
    static func subtitlesLine(_ tags: [String]?) -> String? {
        var names: [String] = []
        for tag in tags ?? [] {
            let name = Locale(identifier: "en").localizedString(forIdentifier: tag) ?? tag
            if !names.contains(name) { names.append(name) }
        }
        return names.isEmpty ? nil : "Subtitles: \(names.joined(separator: ", "))"
    }

    /// An episode card's accessibility value gains the state: "Downloaded", "Downloading, 45 percent".
    static func spokenState(_ badge: DownloadBadge?) -> String? {
        guard let badge else { return nil }
        let control = control(badge)
        return [control.spoken, control.value].compactMap { $0 }.joined(separator: ", ")
    }

    static func deleteSeries(_ count: Int, _ name: String) -> String {
        "Delete \(count == 1 ? "1 downloaded episode" : "\(count) downloaded episodes") of \(name)?"
    }

    static func deleteAll(_ bytes: Int64) -> String { "Delete all downloads on this device? \(StreamChoice.bytes(bytes))" }

    static let emptyTitle = "Nothing downloaded yet"
    static let emptyBody = "Download movies and episodes to watch them without a connection."
    static let offline = "You're offline. Your downloads still play."
    static let offlineAction = "Go to Downloads"
    static let titleOffline = "Showing what's downloaded"
    static let wifiOnly = "Download on Wi-Fi only"
    static let smart = "Smart downloads"
    static let smartLine = "When you finish a downloaded episode, the next one downloads and the one you watched is deleted."
    static let deleteAllButton = "Delete all downloads"
    static let pick = "Pick the version to keep on this device"
    static let pickAction = "Download this version"
    /// A download op the core refused, the same lines as Android and web (review r3 N1).
    static let addFailed = "The download could not start. Try again."
    static let actFailed = "That did not go through. Try again."
    static let loading = "Loading downloads"
}
