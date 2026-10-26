import Foundation

/// The wide layouts by the room they get, not the size class: an iPad in
/// portrait and a large phone in landscape are "regular" with nowhere near
/// the width those layouts were made for. Widths are the page's, inside its gutters.
enum WideLayout {
    /// The pick card (360 pt) beside the Versions table.
    static func pickBesideTable(page: CGFloat) -> Bool { page >= 900 }
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
    /// The rated cover card at the hero's right (plans/title-page-ratings-genres-similar.md): a fixed 2:3 poster.
    #if os(tvOS)
    static let coverWidth: CGFloat = 300
    #else
    static let coverWidth: CGFloat = 240
    #endif
    /// The card shows where it clears the hero's words (held to 480 from the 40 margin) with 40 on every side,
    /// and where the hero is tall enough for it above the action row's height and the foot.
    static func coverFits(window: CGFloat, hero: CGFloat) -> Bool {
        window >= 40 + BackdropStops.heroSynopsisWidth + 40 + coverWidth + 40
            && hero >= coverWidth * 1.5 + 16 + 40 + HeroLayout.foot + 24
    }
}

/// Play's wait for the pick, when it opened the player at once: it ends when the core's view is done,
/// its pick has settled (3 s after the first), or its open failed; never on an app clock. After Try
/// again, only what lands later counts.
enum PickWait {
    static func ends(snapshot: VersionsSnapshot?, failed: Bool, settled: Bool) -> Bool {
        failed || snapshot?.done == true || (snapshot?.recommended != nil && settled)
    }

    /// The version to start: with no source in reach the one kept here, else the pick, else (rows and
    /// no pick named) the core's first row.
    static func key(_ snapshot: VersionsSnapshot?) -> String? {
        let kept = snapshot?.empty.flatMap { $0.kind == "unreachable" ? $0.key : nil }
        return kept ?? snapshot?.recommended?.key ?? snapshot?.rows.first?.key
    }

    /// Whether what the view holds came in after the last Try again (`retriedAt`, nil for none).
    static func fresh(landed: Int, retriedAt: Int?) -> Bool {
        retriedAt.map { landed > $0 } ?? true
    }
}

/// Plain words for what the core says about a version, equal to the web's
/// `lib/words.ts` and Android's `VersionsCopy`. Never emoji, never codes.
enum VersionWords {
    /// The tone of a line drawn with a dot.
    enum Tone { case idle, ok, warn, bad }

    /// What an opened row adds under its release name, as Android's
    /// `VersionsCopy.detail`: "English, 2 sources", or "Language not stated".
    static func detail(_ row: VersionRow) -> String {
        let parts = [languages(row), row.failoverCount > 0 ? "\(row.failoverCount + 1) sources" : ""].filter { !$0.isEmpty }
        return parts.isEmpty ? "Language not stated" : parts.joined(separator: ", ")
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
        case settingsAddons, playFromDevice(key: String), tryAgain, back
    }

    /// nothingPlaysHere offers Back only: its rows are listed dimmed, each startable, and Choose a
    /// version belongs to the title and the player.
    static func emptyActs(_ empty: EmptyVersions) -> [EmptyAct] {
        let first: [EmptyAct] = switch empty.kind {
        case "noSource": [.settingsAddons]
        case "unreachable": (empty.key.map { [.playFromDevice(key: $0)] } ?? []) + [.tryAgain]
        default: []
        }
        return first + [.back]
    }

    static func emptyActWord(_ act: EmptyAct) -> String {
        let w = words.empty
        return switch act {
        case .settingsAddons: w["noSource"]?.action ?? "Settings"
        case .playFromDevice: w["unreachable"]?.playFromDevice ?? "Play from this device"
        case .tryAgain: w["unreachable"]?.action ?? "Try again"
        case .back: words.page.back
        }
    }

    /// A dimmed row's one-line status, why it waits, with its icon; nil for a row that can play.
    static func dimmedStatus(_ kind: String?) -> (text: String, glyph: String)? {
        switch kind {
        case "needsAccount": words.empty["needsAccount"]?.row.map { ($0, "ban") }
        case "nothingPlaysHere": words.empty["nothingPlaysHere"]?.row.map { ($0, "ban") }
        default: nil
        }
    }

    /// The header's count: the versions listed, else the ones an empty state lists dimmed; nil for
    /// none, so no zero sits beside an empty line.
    static func count(_ s: VersionsSnapshot?) -> String? {
        guard let s else { return nil }
        // Every row listed, the plugins' own pass too (plans/plugins-and-marketplace.md 4.4).
        let n = s.rows.count + (s.recommended == nil ? 0 : 1) + (s.plugins?.rows.count ?? 0)
        let shown = n > 0 ? n : s.empty?.rows.count ?? 0
        return shown > 0 ? StreamChoice.counted("versions", shown) : nil
    }

    /// The reserved status line: "Finding versions" until one is found, "Still looking for more
    /// versions" while the core still asks, then nothing. Never an addon's name. When every addon
    /// answered with nothing to pick, the plugins' rows are chosen by hand (4.4), or they still look.
    static func status(_ s: VersionsSnapshot?) -> String? {
        guard let s else { return words.page.finding }
        guard s.empty == nil else { return nil }
        let none = s.recommended == nil && s.rows.isEmpty
        guard s.done else { return none ? words.page.finding : words.page.stillLooking }
        guard none else { return nil }
        if !(s.plugins?.rows ?? []).isEmpty { return PluginWords.choose }
        return s.plugins?.pending == true ? words.page.finding : nil
    }

    /// "Smaller file: 720p, 1.5 GB" and "Sharper: 4K, 12 GB", in the core's order, each only when the
    /// core sends it and it is not the pick.
    static func shortcuts(_ s: VersionsSnapshot?) -> [(label: String, row: VersionRow)] {
        guard let s else { return [] }
        let pick = s.recommended?.key
        return [(words.page.smaller, s.smaller), (words.page.sharper, s.sharper)].compactMap { name, row in
            guard let row, row.key != pick else { return nil }
            // The size as the rows below say it, so a shortcut and its row read the same.
            let what = ([cell(row.quality)] + [bytes(row)].compactMap { $0 }).joined(separator: ", ")
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
        // "From plugins" (plans/plugins-and-marketplace.md 4.4, 7.8): their own pass, after the addons' rows.
        let plugins = (s.plugins?.rows ?? []).isEmpty ? [] : [Section(quality: "plugins", title: PluginWords.section, rows: s.plugins?.rows ?? [])]
        return addonSections(s, held: held) + plugins
    }

    private static func addonSections(_ s: VersionsSnapshot, held: [String: [String]]?) -> [Section] {
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

    /// The card's kicker: "Best for this screen" on a computer, "Best for this phone", "Best for this TV".
    static var kicker: String {
        #if os(tvOS)
        words.page.bestTv
        #elseif os(iOS)
        // An iPad reads as a phone too: the words have no tablet yet.
        words.page.bestPhone
        #else
        "Best for \(words.conditions.devices["desktop"] ?? "this computer")"
        #endif
    }

    /// One of Apple's badges: the resolution filled, the Dolby words plain, the short marks outlined.
    struct Badge: Hashable {
        enum Look { case filled, plain, outlined }
        let text: String
        let look: Look
    }

    /// A version's badge row, as Android and the v3 frames draw it: the quality first, then one video
    /// format (Dolby Vision, else HDR10+, else HDR) and one audio format (Dolby Atmos, else 7.1, else 5.1).
    /// A quality the release does not state gets no badge: its section ("Other") already says so.
    static func badges(_ row: VersionRow) -> [Badge] {
        let f = Set(row.features)
        let video: Badge? = if f.contains("Dolby Vision") { Badge(text: "Dolby Vision", look: .plain) }
            else if f.contains("HDR10+") { Badge(text: "HDR10+", look: .outlined) }
            else if !f.isDisjoint(with: ["HDR10", "HDR", "HLG"]) { Badge(text: "HDR", look: .outlined) }
            else { nil }
        let audio: Badge? = if f.contains("Atmos") { Badge(text: "Dolby Atmos", look: .plain) }
            else if f.contains("7.1") { Badge(text: "7.1", look: .outlined) }
            else if f.contains("5.1") { Badge(text: "5.1", look: .outlined) }
            else { nil }
        let quality = cell(row.quality) == "Unknown" ? nil : Badge(text: row.quality, look: .filled)
        return [quality, video, audio].compactMap { $0 }
    }

    /// The row's language when it is not the viewer's (the core's `otherLanguages` group): "Turkish".
    static func languageTag(_ row: VersionRow) -> String? {
        row.group == "otherLanguages" && !row.languages.isEmpty ? languages(row) : nil
    }

    /// A size as every line on the page says it, 1024-based as the shared words' rule: "1.5 GB".
    private static func bytes(_ row: VersionRow) -> String? { row.sizeBytes.map(StreamChoice.bytes) }

    /// The core's line for a row, "On this device" taking its place for the version kept here.
    private static func rowLine(_ row: VersionRow) -> LineWords? {
        let line = row.line.flatMap { StreamChoice.lineWords($0) }
        guard let kept = row.onDevice else { return line }
        return LineWords(tone: .ok, text: kept, details: line?.details ?? "")
    }

    /// A row's one quiet line: its size first, then its source in plain words ("2 GB, shared by 412
    /// people", "4 GB, ready now"), the core's warning ("12 GB, may start slowly"), or, dimmed, why it
    /// may not play ("2 GB, needs an account"). Words after a size go on in lower case.
    static func rowWords(_ row: VersionRow, dimmed: String? = nil) -> String {
        let line = rowLine(row)
        let said: String? = if let dimmed { dimmed }
            else if let kept = row.onDevice { kept }
            else if line.map({ $0.tone != .ok }) == true { line?.text }
            // A plugin's row names its provider and the host it plays from (4.4).
            else if row.origin == "plugin" { PluginWords.rowLine(row) }
            else { source(row.status) ?? line?.text }
        guard let size = bytes(row) else { return said ?? "" }
        return ([size] + [said.map(StreamChoice.lowerFirst)].compactMap { $0 }).joined(separator: ", ")
    }

    private static func source(_ status: VersionStatus) -> String? {
        let s = words.page.source
        return switch status {
        case .ready: s["ready"]
        case .direct: s["direct"]
        case .needsDownload: s["download"]
        case .torrent(let seeds): seeds.flatMap { n in s["shared"]?.replacingOccurrences(of: "{people}", with: StreamChoice.counted("people", n)) }
        }
    }

    /// A row's details, reserved under it and shown on focus or hover: the release name, the line's
    /// details, its languages and sources.
    static func details(_ row: VersionRow) -> String {
        [row.releaseName, rowLine(row)?.details ?? "", detail(row)].filter { !$0.isEmpty }.joined(separator: " \u{B7} ")
    }

    /// The iPhone's details sheet under what the row shows: the core's details its face does not say, the file,
    /// then its languages and sources, one per line, as Android's sheet.
    static func detailLines(_ row: VersionRow, dimmed: String? = nil) -> [String] {
        [sheetDetails(row, dimmed: dimmed), row.releaseName, detail(row)].filter { !$0.isEmpty }
    }

    /// The core's details under a row's face in its sheet, with one rule on every platform: each part the face
    /// ([rowWords]) already says is left out, the rest stays. "2 GB, ready now" drops "Ready now" and "2 GB";
    /// "7.4 GB, on this device" keeps "7.4 GB; Marquee prefers files up to 5 GB".
    static func sheetDetails(_ row: VersionRow, dimmed: String? = nil) -> String {
        let face = rowWords(row, dimmed: dimmed)
        let kept = (row.line.map(StreamChoice.lineParts) ?? []).filter { !says(face, $0) }
        let joined = kept.joined(separator: ", ")
        return joined.prefix(1).uppercased() + joined.dropFirst()
    }

    /// Whether `face` says `part` whole, case-folded: found at the start or after a space, and followed by the end
    /// or by neither a letter nor a digit, so "4 GB" is not said by "14 GB" or "1.4 GB".
    static func says(_ face: String, _ part: String) -> Bool {
        let f = face.lowercased()
        let p = part.lowercased()
        var from = f.startIndex
        while let found = f.range(of: p, range: from..<f.endIndex) {
            let beforeOK = found.lowerBound == f.startIndex || f[f.index(before: found.lowerBound)].isWhitespace
            let afterOK = found.upperBound == f.endIndex || !(f[found.upperBound].isLetter || f[found.upperBound].isNumber)
            if beforeOK && afterOK { return true }
            from = f.index(after: found.lowerBound)
        }
        return false
    }

    /// Words that go on a sentence: the first letter in lower case, unless it opens a word in capitals ("CAM", "HDR").
    static func lowerFirst(_ words: String) -> String {
        let chars = Array(words)
        if chars.count > 1, chars[1].isUppercase { return words }
        return words.prefix(1).lowercased() + words.dropFirst()
    }

    /// The card's one why line: the core's line and its details joined by a colon ("Starts quickly: shared
    /// by 412 people, 2 GB"), or, for the version kept on this device, that alone ("On this device"): one idea,
    /// as the web and Android say it.
    static func why(_ row: VersionRow) -> String {
        if let kept = row.onDevice { return kept }
        guard let line = rowLine(row) else { return [bytes(row)].compactMap { $0 }.joined() }
        // After the colon the details go on as one sentence: "May start slowly: shared by 21 people".
        return line.details.isEmpty ? line.text : "\(line.text): \(lowerFirst(line.details))"
    }

    /// The card as a screen reader says it: its kicker, its badges, its why line, with no empty part.
    static func cardSpoken(_ row: VersionRow, kicker: String = VersionWords.kicker) -> String {
        [kicker, badges(row).map(\.text).joined(separator: ", "), why(row)].filter { !$0.isEmpty }.joined(separator: ". ")
    }

    /// A row as a screen reader says it: its badges, its language, its line.
    static func rowSpoken(_ row: VersionRow, dimmed: String? = nil) -> String {
        (badges(row).map(\.text) + [languageTag(row), rowWords(row, dimmed: dimmed)].compactMap { $0 }).joined(separator: ", ")
    }

    /// "Show all 12" for a closed section.
    static func showAll(_ n: Int) -> String { words.page.showAll.replacingOccurrences(of: "{n}", with: String(n)) }

    // The kind chips (plans/plugins-and-marketplace.md 4.2): the core counts the kinds and, under one (`setKind`),
    // sends that kind's own card, rows and sections.

    /// A kind chip's words: its kind (nil: All), its name, its count as shown ("1,204"; none on All) and what
    /// VoiceOver says.
    struct KindLabel: Hashable {
        let kind: String?
        let label: String
        let count: String?
        let spoken: String
    }

    /// The core's kinds call for chips: two or more. Once shown, the page keeps them.
    static func showsKinds(_ s: VersionsSnapshot?) -> Bool { (s?.kinds ?? []).count >= 2 }

    /// All, then each kind the core counted, in its order (a kind from a newer core left out).
    static func kindChips(_ s: VersionsSnapshot?) -> [KindLabel] {
        let kinds = (s?.kinds ?? []).filter { ["torrent", "direct", "debrid"].contains($0.kind) }
        return [KindLabel(kind: nil, label: words.kind.all, count: nil, spoken: words.kind.all)] + kinds.map { k in
            let label = switch k.kind {
            case "torrent": words.kind.torrent
            case "direct": words.kind.direct
            default: k.service ?? words.kind.debrid
            }
            let spoken = words.kind.a11y.replacingOccurrences(of: "{kind}", with: label)
                .replacingOccurrences(of: "{versions}", with: StreamChoice.counted("versions", k.count))
            return KindLabel(kind: k.kind, label: label, count: StreamChoice.grouped(k.count), spoken: spoken)
        }
    }

    /// The card's kicker: the form's with every kind listed, else "Best torrent", "Best direct link",
    /// "Best on Real-Debrid".
    static func kindKicker(_ s: VersionsSnapshot?) -> String {
        guard let kind = s?.selectedKind, let best = words.kind.best[kind] else { return kicker }
        if kind == "debrid", let service = s?.kinds?.first(where: { $0.kind == kind })?.service {
            return (words.kind.best["service"] ?? best).replacingOccurrences(of: "{service}", with: service)
        }
        return best
    }
}
