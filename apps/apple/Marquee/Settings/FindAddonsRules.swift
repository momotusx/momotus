import Foundation
import Observation

// What a Find addons card shows and does, from the core's entry: the core
// sends facts (`installed`, `configuration`), these rules turn them into one
// action, the same on every app (plans/m12-addon-discovery.md 3.2; the web's
// apps/web/src/lib/find-addons.ts).

private typealias W = FindAddonsWords

/// What an addon provides, in words.
func providesWord(_ resource: String) -> String {
    switch resource {
    case "catalog": "Catalogs"
    case "meta": "Details"
    case "stream": "Streams"
    case "subtitles": "Subtitles"
    case "addon_catalog": "Addon lists"
    default: resource
    }
}

/// The Show chips: everything, or the entries that provide one resource.
enum FindFilter: CaseIterable, Hashable {
    case all, sources, subtitles, catalogs

    var label: String {
        switch self {
        case .all: W.all
        case .sources: W.sources
        case .subtitles: W.subtitles
        case .catalogs: W.catalogs
        }
    }

    func shows(_ entry: ListedAddon) -> Bool {
        switch self {
        case .all: true
        case .sources: entry.provides.contains("stream")
        case .subtitles: entry.provides.contains("subtitles")
        case .catalogs: entry.provides.contains("catalog")
        }
    }

    /// "No subtitle addons in this list."; nil for All.
    var emptyWords: String? {
        let kind: String? = switch self {
        case .all: nil
        case .sources: W.kindSources
        case .subtitles: W.kindSubtitles
        case .catalogs: W.kindCatalogs
        }
        return kind.map { W.filterEmpty.replacingOccurrences(of: "{kind}", with: $0) }
    }
}

/// A card's action. A TV shows no Configure; its card says the help line.
enum FindAction: Equatable {
    /// Installed or set up: a word, no action.
    case none(word: String)
    /// Turn on that link (this entry's, or a configured copy's), with the word for its state.
    case turnOn(url: String, word: String)
    case configure
    case install(configure: Bool)

    /// The state word beside the action: Installed, Set up, Off, Set up, off.
    var word: String? {
        switch self {
        case .none(let word), .turnOn(_, let word): word
        case .configure, .install: nil
        }
    }

    /// Install and Turn on run a request; Configure opens a page.
    var pressable: Bool {
        switch self {
        case .install, .turnOn: true
        case .none, .configure: false
        }
    }

    var offersConfigure: Bool { self == .configure || self == .install(configure: true) }
}

extension ListedAddon {
    /// First match: on or set up, none; off, Turn on this link; a set-up copy
    /// off, Turn on that copy (none when the core named no copy); setup
    /// required, Configure; optional, Install and Configure; else Install.
    var action: FindAction {
        switch installed {
        case .on: .none(word: W.installed)
        case .copyOn: .none(word: W.setUp)
        case .off: .turnOn(url: key, word: W.off)
        case .copyOff: copyKey.map { .turnOn(url: $0, word: W.setUpOff) } ?? .none(word: W.setUpOff)
        case .absent:
            configuration == .required ? .configure : .install(configure: configuration == .optional)
        }
    }

    /// The action with the card's last result: an install refused as needing
    /// setup asks for Configure, until the entry is installed some other way
    /// (a pasted configured link), which wins.
    func cardAction(lastCode: String?) -> FindAction {
        installed == .absent && lastCode == "needsSetup" ? .configure : action
    }

    /// "Subtitles for movies and series".
    var providesLine: String {
        let what = provides.map(providesWord).joined(separator: ", ")
        var kinds = types.map(Self.kindWord)
        guard let last = kinds.popLast() else { return what }
        return kinds.isEmpty ? "\(what) for \(last)" : "\(what) for \(kinds.joined(separator: ", ")) and \(last)"
    }

    private static func kindWord(_ kind: String) -> String {
        switch kind {
        case "movie": "movies"
        case "series": "series"
        case "channel": "channels"
        case "tv": "live TV"
        case "anime": "anime"
        default: kind.lowercased()
        }
    }

    /// The mark on the brand surface: the name's first letter or digit,
    /// symbols skipped; no remote logo, so opening a list contacts no image host.
    var initial: String {
        name.first { $0.isLetter || $0.isNumber }.map { $0.uppercased() } ?? ""
    }

    /// For a screen reader: "Install OpenSubtitles v3, subtitles", with the
    /// host when another card of the list has the same name. An installed
    /// card says its state instead of an action.
    func spoken(_ act: FindAction, sameName: Bool) -> String {
        let verb: String? = switch act {
        case .none: nil
        case .turnOn: W.turnOn
        case .configure: W.configure
        case .install: W.install
        }
        let state: String? = switch act {
        case .none(let word): word.lowercased()
        default: nil
        }
        let parts = [
            verb.map { "\($0) \(name)" } ?? name,
            sameName ? W.fromHost.replacingOccurrences(of: "{host}", with: host) : nil,
            provides.map(providesWord).joined(separator: ", ").lowercased(),
            state,
        ]
        return parts.compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: ", ")
    }

    /// Names shown by more than one card of a list: their cards also say the host.
    static func repeatedNames(_ entries: [ListedAddon]) -> Set<String> {
        var seen = Set<String>()
        var twice = Set<String>()
        for entry in entries where !seen.insert(entry.name).inserted {
            twice.insert(entry.name)
        }
        return twice
    }
}

extension String {
    /// Whether the text reads right to left, by its first letter (the web's
    /// `dir="auto"`): Hebrew, Arabic and the other right-to-left scripts.
    var readsRightToLeft: Bool {
        guard let first = unicodeScalars.first(where: \.properties.isAlphabetic) else { return false }
        switch first.value {
        case 0x0590...0x08FF, 0xFB1D...0xFDFF, 0xFE70...0xFEFF, 0x10800...0x10FFF, 0x1E800...0x1EFFF: return true
        default: return false
        }
    }
}

/// Each Find addons card's Installing and failure state, by entry key. It
/// lives outside the page, so a result that lands after a list switch, or
/// after the page closed and opened again, updates only its own card. Keys
/// may hold an addon's keys: they stay in memory and are never shown.
@MainActor @Observable
final class InstallCards {
    static let shared = InstallCards()

    struct Card: Equatable {
        var busy = false
        var code: String?
        var words: String?
    }

    private var cards: [String: Card] = [:]
    /// Bumped when every card is forgotten for a new profile: a request still
    /// running from before must not land on the new profile's card.
    @ObservationIgnored private var generation = 0

    subscript(key: String) -> Card { cards[key] ?? Card() }

    /// Forgets a card's last result (its entry's installed state changed).
    func forget(_ key: String) {
        if cards[key]?.busy == false { cards[key] = nil }
    }

    /// Forgets every card's result (Try again); for a new profile also those
    /// still running, whose answers then land nowhere.
    func forgetAll(running: Bool = true) {
        if running { generation += 1 }
        cards = running ? [:] : cards.filter(\.value.busy)
    }

    /// Runs one card's request; a second press while it runs does nothing.
    /// `failure` gives a refusal's code and the words the card shows.
    func run(
        _ key: String, _ request: @MainActor () async throws -> Void,
        failure: (any Error) -> (code: String?, words: String)
    ) async {
        guard cards[key]?.busy != true else { return }
        let mine = generation
        cards[key] = Card(busy: true)
        do {
            try await request()
            if mine == generation { cards[key] = nil }
        } catch {
            guard mine == generation else { return }
            // A request the core settled as cancelled (a switch) shows nothing.
            if error is CancellationError {
                cards[key] = nil
            } else {
                let (code, words) = failure(error)
                cards[key] = Card(code: code, words: words)
            }
        }
    }
}
