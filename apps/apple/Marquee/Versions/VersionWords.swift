import Foundation

/// The wide layouts by the room they get, not the size class: an iPad in
/// portrait and a large phone in landscape are "regular" with nowhere near
/// the width those layouts were made for. Widths are the page's, inside its gutters.
enum WideLayout {
    /// The pick card (360 pt) beside the Versions table.
    static func pickBesideTable(page: CGFloat) -> Bool { page >= 900 }
    /// The table's columns (quality, features, size, status, via) need about
    /// 680 pt; with less a row reads as the phone's two lines.
    static func tableColumns(table: CGFloat) -> Bool { table >= 680 }
    /// A series' credits (360 pt) beside its episodes.
    static func creditsBesideEpisodes(page: CGFloat) -> Bool { page >= 1000 }
    /// The title's words over its art: a wide window that is also tall enough.
    /// A short one (a large phone in landscape, a short iPad split) stacks them
    /// under the art, as a phone does, with the art held to the window.
    static func titleOverArt(wide: Bool, window: CGFloat) -> Bool { wide && !(window > 0 && window < 500) }
    /// The Mac title hero, as Home's: 16:9, held so the first shelf's title shows on the first screen,
    /// never under 400; `heroFrame` grows it when its words need more.
    static func titleHeroHeight(width: CGFloat, height: CGFloat) -> CGFloat {
        guard width > 0, height > 0 else { return 506 }
        return max(min(width * 0.5625, height - 90), 400)
    }
    /// The panel beside the info: 40 at the left, 40 between, the 420 panel, 40 at the right.
    static let panelRoom: CGFloat = 540
    /// Below about 1020 the column beside the panel would be under 480: the panel covers the info and
    /// the page dims (plans/design-v3-phase3-apple.md 2.3).
    static func panelCoversInfo(window: CGFloat) -> Bool { window - panelRoom < BackdropStops.heroSynopsisWidth }
    /// The hero column beside the open panel, at most the hero's 900.
    static func panelColumnWidth(window: CGFloat) -> CGFloat { min(900, max(0, window - panelRoom)) }
    /// Starring (360 pt at the right) clears a full action row (about 760) with 40 on every side.
    static func starringFits(window: CGFloat) -> Bool { window >= 40 + 760 + 40 + 360 + 40 }
}

/// Plain words for what the core says about a version, equal to the web's
/// `lib/words.ts` and Android's `VersionsCopy`. Never emoji, never codes.
enum VersionWords {
    /// The tone of a line drawn with a dot.
    enum Tone { case idle, ok, warn, bad }

    static func size(_ bytes: Int64?) -> String? {
        bytes.map { ByteCountFormatter.string(fromByteCount: $0, countStyle: .file) }
    }

    static func status(_ status: VersionStatus) -> String {
        switch status {
        case .ready(let service): service.map { "Ready on \($0)" } ?? "Ready"
        case .torrent(let seeds): seeds.map { "Torrent, \($0) \($0 == 1 ? "seed" : "seeds")" } ?? "Torrent"
        case .needsDownload(let service): service.map { "Needs download on \($0)" } ?? "Needs download"
        case .direct: "Direct link"
        }
    }

    /// Good when it can play now; a torrent or a download has to start first.
    static func tone(_ status: VersionStatus) -> Tone {
        switch status {
        case .ready, .direct: .ok
        case .torrent, .needsDownload: .warn
        }
    }

    /// Why a version is recommended; "cached on X" when the status names the service.
    static func reason(_ reason: String, status: VersionStatus? = nil) -> String {
        switch reason {
        case "readyNow":
            if case .ready(let service?)? = status { "Ready now, cached on \(service)" } else { "Ready now" }
        case "playsDirectly": "Plays directly on this device"
        case "matchesScreen": "Matches your screen"
        case "sameReleaseAsLast": "Same release as last time"
        case "yourLanguage": "In your language"
        case "smallerFaster": "Smaller and faster to start"
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

    /// "19 are for other titles, 1 is another episode". `window` as in
    /// `StreamChoice.blockedWords`.
    static func hidden(_ summary: HiddenSummary, window: Bool = true) -> String {
        summary.byReason.map { item in
            let one = item.count == 1
            let what: String
            switch item.reason {
            case "wrongTitle": what = one ? "is for another title" : "are for other titles"
            case "wrongEpisode": what = one ? "is another episode" : "are other episodes"
            case "excluded": what = one ? "matches your avoid list" : "match your avoid list"
            case "tooLarge": what = one ? "is over your size limit" : "are over your size limit"
            case "unplayable": what = "cannot play on this device"
            case "unsupportedSource": what = one ? "uses a source this app cannot open yet" : "use a source this app cannot open yet"
            // Kids profiles only.
            case "leavesApp": what = "would open outside Marquee"
            case "notForKids": what = one ? "is not for kids" : "are not for kids"
            case "needsDebrid": what = one ? "needs a paid streaming service account" : "need a paid streaming service account"
            case "noSpace" where !window: what = "\(one ? "is" : "are") too large for this storage"
            case "noSpace": what = "\(one ? "has" : "have") no room to play here, even a few minutes at a time"
            default: what = item.reason
            }
            return "\(item.count) \(what)"
        }
        .joined(separator: ", ")
    }

    /// Why there is no pick once every addon has answered; blocked versions say what would let them play.
    static func none(_ snapshot: VersionsSnapshot) -> String {
        let window = snapshot.conditions?.window ?? true
        if let blocked = snapshot.blocked, let words = StreamChoice.blockedWords(blocked, window: window) { return words.text }
        return snapshot.hidden.count > 0
            ? "No version plays here: \(hidden(snapshot.hidden, window: window))"
            : "No version found: your addons have no source for this"
    }

    /// The Versions header: who is still asked, slow or silent, else how many versions there are.
    static func progress(_ snapshot: VersionsSnapshot?) -> (text: String, tone: Tone) {
        guard let s = snapshot else { return ("Opening", .idle) }
        if !s.pendingAddons.isEmpty { return ("Asking \(s.pendingAddons.joined(separator: ", "))", .warn) }
        if !s.slowAddons.isEmpty { return ("\(s.slowAddons.joined(separator: ", ")) is slow, still waiting", .warn) }
        if !s.failedAddons.isEmpty { return ("\(s.failedAddons.joined(separator: ", ")) did not answer", .bad) }
        // Found, but none listed: how many there are, as a warning.
        if let blocked = s.blocked { return (StreamChoice.counted("versions", blocked.count), .warn) }
        let count = s.rows.count + (s.recommended == nil ? 0 : 1)
        return (count == 1 ? "1 version" : "\(count) versions", .ok)
    }

    /// "This screen: 1080p, HDR, plays H.264, HEVC in MP4, MOV": what the ranking was made for.
    static func device(screen: String, decoders: [String], containers: [String]) -> String {
        let codecs = ["h264": "H.264", "h265": "HEVC", "av1": "AV1", "vp9": "VP9"]
        let plays = decoders.map { codecs[$0] ?? $0.uppercased() }.joined(separator: ", ")
        let boxes = containers.map { $0 == "webm" ? "WebM" : $0.uppercased() }.joined(separator: ", ")
        return "This screen: \(screen), plays \(plays) in \(boxes)"
    }

    /// The pick's heading, the quality and its first feature: "4K Dolby Vision".
    static func pickHeading(_ row: VersionRow) -> String {
        ([row.quality] + row.features.prefix(1)).joined(separator: " ")
    }

    /// The pick's facts in the web's order: size, status, where from, more features, languages.
    static func facts(_ row: VersionRow) -> [(key: String, value: String)] {
        var facts = [
            (key: "Size", value: size(row.sizeBytes) ?? "Unknown"),
            (key: "Status", value: status(row.status)),
            (key: "Via", value: row.via),
        ]
        if row.features.count > 1 { facts.append((key: "Features", value: row.features.joined(separator: ", "))) }
        if !row.languages.isEmpty { facts.append((key: "Languages", value: languages(row))) }
        return facts
    }

    /// The line under Play: how many sources back this release up.
    /// What an opened row adds under its release name, as Android's
    /// `VersionsCopy.detail`: "English, 2 sources", or "Language not stated".
    static func detail(_ row: VersionRow) -> String {
        let parts = [languages(row), row.failoverCount > 0 ? "\(row.failoverCount + 1) sources" : ""].filter { !$0.isEmpty }
        return parts.isEmpty ? "Language not stated" : parts.joined(separator: ", ")
    }

    static func failover(_ row: VersionRow) -> String {
        row.failoverCount > 0
            ? "\(row.failoverCount + 1) sources. If one fails, the next starts on its own."
            : "One source for this release."
    }

    /// The card's heading while there is no pick, once every addon has answered.
    static func waitingTitle(_ snapshot: VersionsSnapshot) -> String {
        snapshot.rows.isEmpty ? "No playable version found" : "Nothing is ready to play yet"
    }

    /// Why nothing is picked first, then any addon that failed.
    static func waitingBody(_ snapshot: VersionsSnapshot) -> String {
        let why = if !snapshot.rows.isEmpty {
            "Choose a version to start it."
        } else if snapshot.hidden.count > 0 {
            "Every result was hidden: \(hidden(snapshot.hidden))."
        } else {
            "None of your addons returned a stream for this video. A source addon finds playable streams."
        }
        return [why, addons(snapshot).map { "\($0)." }].compactMap { $0 }.joined(separator: " ")
    }

    /// Addons still answering, slow or failed, as one line, or nil.
    static func addons(_ snapshot: VersionsSnapshot) -> String? {
        let parts = [
            snapshot.pendingAddons.isEmpty ? nil : "Waiting for \(snapshot.pendingAddons.joined(separator: ", "))",
            snapshot.slowAddons.isEmpty ? nil : "Slow: \(snapshot.slowAddons.joined(separator: ", "))",
            snapshot.failedAddons.isEmpty ? nil : "No answer from \(snapshot.failedAddons.joined(separator: ", "))",
        ].compactMap { $0 }
        return parts.isEmpty ? nil : parts.joined(separator: ". ")
    }

    /// "English, Turkish" from the release's codes.
    static func languages(_ row: VersionRow) -> String {
        row.languages.map(languageName).joined(separator: ", ")
    }

    /// "fr" or "fre" to "French"; an unknown code stays as given, in capitals.
    static func languageName(_ code: String) -> String {
        let lower = code.lowercased()
        let name = Locale(identifier: "en").localizedString(forIdentifier: iso1[lower] ?? lower)
        if let name, name.lowercased() != lower { return name }
        return code.uppercased()
    }

    // The bibliographic three-letter codes Locale does not take, as on the web.
    private static let iso1 = [
        "eng": "en", "tur": "tr", "fre": "fr", "fra": "fr", "ger": "de", "deu": "de", "spa": "es", "ita": "it",
        "por": "pt", "rus": "ru", "jpn": "ja", "kor": "ko", "chi": "zh", "zho": "zh", "ara": "ar", "dut": "nl",
        "nld": "nl", "pol": "pl", "swe": "sv", "hin": "hi", "gre": "el", "ell": "el", "rum": "ro", "ron": "ro",
        "hun": "hu", "cze": "cs", "ces": "cs", "heb": "he", "per": "fa", "fas": "fa", "pob": "pt-BR",
    ]

    /// The line under Play: "1080p  HDR10  2.4 GB  Ready on Real-Debrid".
    /// The quality is the core's headline as is ("Quality not stated" at worst).
    static func line(_ row: VersionRow) -> String {
        ([row.quality] + row.features + [size(row.sizeBytes)].compactMap { $0 }).joined(separator: "  ")
    }

    /// A short cell (a table column, an OSD value) says "Unknown", one word.
    static func cell(_ quality: String) -> String {
        quality == "Quality not stated" || quality == "Unknown" ? "Unknown" : quality
    }
}
