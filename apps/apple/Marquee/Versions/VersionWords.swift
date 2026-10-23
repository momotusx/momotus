import Foundation

/// The wide layouts by the room they get, not the size class: an iPad in
/// portrait and a large phone in landscape are "regular" with nowhere near
/// the width those layouts were made for. Widths are the page's, inside its gutters.
enum WideLayout {
    /// The pick card (360 pt) beside the Versions table.
    static func pickBesideTable(page: CGFloat) -> Bool { page >= 900 }
    /// The table's columns (quality, features, size, status) need about
    /// 680 pt; with less a row reads as the phone's two lines.
    static func tableColumns(table: CGFloat) -> Bool { table >= 680 }
    /// A series' credits (360 pt) beside its episodes.
    static func creditsBesideEpisodes(page: CGFloat) -> Bool { page >= 1000 }
    /// The title's words over its art: a wide window that is also tall enough.
    /// A short one (a large phone in landscape, a short iPad split) stacks them
    /// under the art, as a phone does, with the art held to the window.
    static func titleOverArt(wide: Bool, window: CGFloat) -> Bool { wide && !(window > 0 && window < 500) }
    /// The Mac title hero, as Home's: 16:9, held so the first shelf's title shows on the first screen
    /// below the version note's `note` points, never under 400; `heroFrame` grows it when its words
    /// need more.
    static func titleHeroHeight(width: CGFloat, height: CGFloat, note: CGFloat = 0) -> CGFloat {
        guard width > 0, height > 0 else { return 506 }
        return max(min(width * 0.5625, height - 90 - note), 400)
    }
    /// The panel beside the info: 40 at the left, 40 between, the 420 panel, 40 at the right.
    static let panelRoom: CGFloat = 540
    /// Below about 1020 the column beside the panel would be under 480: the panel covers the info and
    /// the page dims (plans/design-v3-phase3-apple.md 2.3).
    static func panelCoversInfo(window: CGFloat) -> Bool { window - panelRoom < BackdropStops.heroSynopsisWidth }
    /// The hero column beside the open panel, at most the hero's 900.
    static func panelColumnWidth(window: CGFloat) -> CGFloat { min(900, max(0, window - panelRoom)) }
    /// The page under the open panel goes inert only while the scrim shows: beside the panel it stays
    /// in reach.
    static func pageInert(wide: Bool, panelOpen: Bool, window: CGFloat) -> Bool {
        wide && panelOpen && panelCoversInfo(window: window)
    }
    /// Starring (360 pt at the right) shows from a 1240 window, where the glass check reads it, and
    /// only where it clears the action row as drawn with 40 on every side: a movie's row runs to
    /// about 875 (review P2-1), a long Resume further.
    static func starringFits(window: CGFloat, row: CGFloat) -> Bool {
        window >= max(starringFrom, 40 + row + 40 + 360 + 40)
    }
    static let starringFrom: CGFloat = 1240
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
        case .ready: "Ready now"
        case .torrent(let seeds): seeds.map { "Torrent, \($0) \($0 == 1 ? "seed" : "seeds")" } ?? "Torrent"
        case .needsDownload: "Needs download"
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

    /// Why a version is recommended, never naming the streaming service.
    static func reason(_ reason: String, status: VersionStatus? = nil) -> String {
        switch reason {
        case "readyNow":
            "Ready now"
        case "playsDirectly": "Plays directly on this device"
        case "matchesScreen": "Matches your screen"
        case "sameReleaseAsLast": "Same release as last time"
        case "yourLanguage": "In your language"
        case "smallerFaster": "Smaller and faster to start"
        default: reason
        }
    }

    /// The pick's heading, the quality and its first feature: "4K Dolby Vision".
    static func pickHeading(_ row: VersionRow) -> String {
        ([row.quality] + row.features.prefix(1)).joined(separator: " ")
    }

    /// The pick's facts in the web's order: size, status, more features, languages; never its addon.
    static func facts(_ row: VersionRow) -> [(key: String, value: String)] {
        var facts = [
            (key: "Size", value: size(row.sizeBytes) ?? "Unknown"),
            (key: "Status", value: status(row.status)),
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

    /// The line under Play: "1080p  HDR10  2.4 GB".
    /// The quality is the core's headline as is ("Quality not stated" at worst).
    static func line(_ row: VersionRow) -> String {
        ([row.quality] + row.features + [size(row.sizeBytes)].compactMap { $0 }).joined(separator: "  ")
    }

    /// A short cell (a table column, an OSD value) says "Unknown", one word.
    static func cell(_ quality: String) -> String {
        quality == "Quality not stated" || quality == "Unknown" ? "Unknown" : quality
    }

    // The page as the core lays it out (plans/play-every-version.md Step 3): no addon, no hidden
    // count, no device line, no account card; every word from apps/shared/stream-choice-words.json.

    private static var words: StreamChoiceWords { StreamChoice.words }

    /// The core's line for a zero-playable state: "Can't reach any source".
    static func emptyLine(_ kind: String) -> String? { words.empty[kind]?.text }

    /// What the Versions page offers when nothing can play, in the shared table's order, Back last.
    enum EmptyAct: Hashable {
        case settingsAddons, settingsAccounts, playFromDevice(key: String), tryAgain, back
    }

    /// nothingPlaysHere offers Back only: its rows are listed dimmed, each startable, and Choose a
    /// version belongs to the title and the player.
    static func emptyActs(_ empty: EmptyVersions) -> [EmptyAct] {
        let first: [EmptyAct] = switch empty.kind {
        case "noSource": [.settingsAddons]
        case "unreachable": (empty.key.map { [.playFromDevice(key: $0)] } ?? []) + [.tryAgain]
        case "needsAccount": [.settingsAccounts]
        default: []
        }
        return first + [.back]
    }

    static func emptyActWord(_ act: EmptyAct) -> String {
        let w = words.empty
        return switch act {
        case .settingsAddons: w["noSource"]?.action ?? "Settings"
        case .settingsAccounts: w["needsAccount"]?.action ?? "Settings"
        case .playFromDevice: w["unreachable"]?.playFromDevice ?? "Play from this device"
        case .tryAgain: w["unreachable"]?.action ?? "Try again"
        case .back: words.page.back
        }
    }

    /// A dimmed row's one-line status, why it waits, with its icon; nil for a row that can play.
    static func dimmedStatus(_ kind: String?) -> (text: String, glyph: String)? {
        switch kind {
        case "needsAccount": words.empty["needsAccount"]?.row.map { ($0, "lock") }
        case "nothingPlaysHere": words.empty["nothingPlaysHere"]?.row.map { ($0, "ban") }
        default: nil
        }
    }

    /// The header's count: the versions listed, else the ones an empty state lists dimmed; nil for
    /// none, so no zero sits beside an empty line.
    static func count(_ s: VersionsSnapshot?) -> String? {
        guard let s else { return nil }
        let n = s.rows.count + (s.recommended == nil ? 0 : 1)
        let shown = n > 0 ? n : s.empty?.rows.count ?? 0
        return shown > 0 ? StreamChoice.counted("versions", shown) : nil
    }

    /// The reserved status line: "Finding versions" until one is found, "Still looking for more
    /// versions" while the core still asks, then nothing. Never an addon's name.
    static func status(_ s: VersionsSnapshot?) -> String? {
        guard let s else { return words.page.finding }
        guard !s.done, s.empty == nil else { return nil }
        return s.recommended == nil && s.rows.isEmpty ? words.page.finding : words.page.stillLooking
    }

    /// "Smaller file: 720p, 1.5 GB" and "Sharper: 4K, 12 GB", in the core's order, each only when the
    /// core sends it and it is not the pick.
    static func shortcuts(_ s: VersionsSnapshot?) -> [(label: String, row: VersionRow)] {
        guard let s else { return [] }
        let pick = s.recommended?.key
        return [(words.page.smaller, s.smaller), (words.page.sharper, s.sharper)].compactMap { name, row in
            guard let row, row.key != pick else { return nil }
            // The size as the rows below say it, so a shortcut and its row read the same.
            let what = ([cell(row.quality)] + [size(row.sizeBytes)].compactMap { $0 }).joined(separator: ", ")
            return ("\(name): \(what)", row)
        }
    }

    /// A quality section as drawn: its heading ("4K", "720p and below") and its rows.
    struct Section: Equatable {
        let quality: String
        let title: String?
        let rows: [VersionRow]
    }

    /// The list under the card: the core's sections in its order, the rows looked up by key, nothing
    /// grouped here. An empty state lists its rows (dimmed); an older core without sections lists every
    /// row as one untitled section. `held`: the order held while Versions is over the player.
    static func sections(_ s: VersionsSnapshot?, held: [String: [String]]? = nil) -> [Section] {
        guard let s else { return [] }
        if let empty = s.empty { return empty.rows.isEmpty ? [] : [Section(quality: "", title: nil, rows: empty.rows)] }
        guard let sections = s.sections, !sections.isEmpty else {
            return s.rows.isEmpty ? [] : [Section(quality: "", title: nil, rows: s.rows)]
        }
        let byKey = Dictionary(s.rows.map { ($0.key, $0) }, uniquingKeysWith: { first, _ in first })
        return sections.compactMap { section in
            let keys = held?[section.quality] ?? section.keys
            let rows = keys.compactMap { byKey[$0] }
            return rows.isEmpty ? nil : Section(quality: section.quality, title: words.page.sections[section.quality] ?? section.quality, rows: rows)
        }
    }

    /// The order held while Versions is over the paused player: rows already shown keep their place in
    /// their section, new ones come after them in the core's order, gone ones go.
    static func hold(_ fresh: [VersionSection]?, shown: [String: [String]]) -> [String: [String]] {
        var held: [String: [String]] = [:]
        for section in fresh ?? [] {
            let kept = (shown[section.quality] ?? []).filter(section.keys.contains)
            held[section.quality] = kept + section.keys.filter { !kept.contains($0) }
        }
        return held
    }

    /// "Show all 12" for a closed section.
    static func showAll(_ n: Int) -> String { words.page.showAll.replacingOccurrences(of: "{n}", with: String(n)) }
}
