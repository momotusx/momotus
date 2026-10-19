import Foundation
import Observation

/// The recommended addons (plans/m13-default-addons.md sections 6 and 7):
/// the core holds the nine and installs them; the app carries no addon links.
enum DefaultsRules {
    /// What the step lists: every default (first run, after an import), or
    /// only the ones missing from the list (the Home card's Review).
    enum Scope { case all, missing }

    /// The groups, in order, with their headings.
    static let groups: [(id: String, title: String)] = [
        ("catalogs", "Catalogs and details"), ("streams", "Streams"), ("subtitles", "Subtitles"), ("lists", "Lists"),
    ]
    static let title = "Recommended addons"
    static let lead = "Marquee starts with these. Turn off any you do not want; you can change this later in Settings."
    static let reviewLead = "Turn off any you do not want; you can change this later in Settings."
    static let settingsLead = "The addons Marquee recommends. A switch turned on adds the addon; Addons lists everything you have."
    static let settingsDetail = "Catalogs, streams, subtitles and lists Marquee recommends, a switch each."
    static let catalogWarning = "Home and search need a catalog addon."

    static func shown(_ list: [DefaultAddonInfo], scope: Scope) -> [DefaultAddonInfo] {
        scope == .all ? list : list.filter { $0.installed == .absent }
    }

    /// Only a missing default has a switch, on until turned off; one the
    /// profile has keeps its own state (Settings switches it).
    static func hasSwitch(_ addon: DefaultAddonInfo) -> Bool { addon.installed == .absent }

    /// The words in place of a switch.
    static func presentWord(_ addon: DefaultAddonInfo, imported: Bool) -> String {
        imported ? "From Stremio" : addon.installed == .on ? "Installed" : "Installed, off"
    }

    /// What Continue sends: exactly the rows with a switch, since the core
    /// touches only the keys it is sent (a new profile names all nine; an
    /// empty list writes nothing).
    static func choices(_ list: [DefaultAddonInfo], moved: [String: Bool]) -> [DefaultChoice] {
        list.filter(hasSwitch).map { DefaultChoice(key: $0.key, on: moved[$0.key] ?? true) }
    }

    /// The switches would leave the profile without any catalog addon.
    static func warns(_ list: [DefaultAddonInfo], moved: [String: Bool], hasCatalog: Bool) -> Bool {
        !hasCatalog && !list.contains { $0.group == "catalogs" && ($0.installed == .absent ? moved[$0.key] ?? true : $0.installed == .on) }
    }

    /// The Home card's names: the defaults the profile misses, once per list
    /// version, never on a kids profile (it reads the owner's list) or at
    /// first run (onboarding offers them all). Nil hides the card.
    static func cardNames(_ list: DefaultAddons?, seen: Int?, kids: Bool, fresh: Bool) -> [String]? {
        guard let list, let seen, seen < list.version, !kids, !fresh else { return nil }
        let names = list.addons.filter { $0.installed == .absent }.map(\.name)
        return names.isEmpty ? nil : names
    }

    /// "Torrentio, Trakt and OpenSubtitles v3".
    static func namesLine(_ names: [String]) -> String {
        guard let last = names.last, names.count > 1 else { return names.joined() }
        return names.dropLast().joined(separator: ", ") + " and " + last
    }

    /// The setup check's fix for a line with nothing on: its recommended
    /// addon (named by key), when it is missing or off.
    static func fix(_ key: String, in list: DefaultAddons?) -> DefaultAddonInfo? {
        list?.addons.first { $0.key == key && $0.installed != .on }
    }

    /// The width of the onboarding steps and the recommended addons pages:
    /// one centered column on TV and wide windows, as the web's (its TV
    /// column and its desktop one), the full width on a phone.
    static func column(wide: Bool, tv: Bool) -> CGFloat? {
        guard wide else { return nil }
        return tv ? 1400 : 880
    }

    /// `-MQOnboardingStep defaults` opens a first run on the recommended addons
    /// step, so a walk shoots it and its TV focus without a remote. Only a
    /// DEBUG build reads the argument; Release passes `debug: false`.
    static func walkOpensDefaults(_ argument: String?, debug: Bool) -> Bool {
        debug && argument == "defaults"
    }

    /// The mark's letter when there is no logo or it does not load: the
    /// name's first letter or digit, as Find addons marks its cards.
    static func initial(_ name: String) -> String {
        name.first { $0.isLetter || $0.isNumber }.map { $0.uppercased() } ?? ""
    }

    static func fixWords(_ addon: DefaultAddonInfo) -> String {
        addon.installed == .absent ? "Add \(addon.name)" : "Turn \(addon.name) on"
    }
}

/// How a switch draws (design/audit-web-2026-10-19.md, fourth pass): on is a
/// white track with a dark knob, unmistakable as on; off an empty track with
/// the control edge. Never the accent: a list of accent tracks would be a
/// large fill, and the white stays small beside the one white action.
struct SwitchLook: Equatable {
    enum Token: Equatable { case primary, onPrimary, control, muted }
    /// The track's fill; nil leaves it empty.
    let track: Token?
    let edge: Token?
    let knob: Token

    static func of(on: Bool) -> SwitchLook {
        on ? SwitchLook(track: .primary, edge: nil, knob: .onPrimary) : SwitchLook(track: nil, edge: .control, knob: .muted)
    }

    /// The word On or Off beside the switch: TV and wide windows. A phone
    /// reads the state from the track (VoiceOver from the value).
    static func showsWord(wide: Bool) -> Bool { wide }
}

/// One switch sent with `installDefaults`.
struct DefaultChoice: Equatable, Sendable {
    let key: String
    let on: Bool

    var fields: [String: any Sendable] { ["key": key, "on": on] }
}

/// The recommended addons: the list from the core, the switches the user
/// moved, and Continue; Settings and the setup check send one key at a time.
@MainActor @Observable
final class DefaultsStep {
    let scope: DefaultsRules.Scope
    private(set) var list: DefaultAddons?
    private(set) var failure: String?
    private(set) var busy = false
    /// Switches the user moved, by key; the rest are on.
    private(set) var moved: [String: Bool] = [:]
    @ObservationIgnored let core: CoreClient

    init(core: CoreClient, scope: DefaultsRules.Scope) {
        self.core = core
        self.scope = scope
    }

    var rows: [DefaultAddonInfo] { DefaultsRules.shown(list?.addons ?? [], scope: scope) }
    var choices: [DefaultChoice] { DefaultsRules.choices(rows, moved: moved) }

    func isOn(_ addon: DefaultAddonInfo) -> Bool { moved[addon.key] ?? true }

    func flip(_ addon: DefaultAddonInfo) {
        moved[addon.key] = !isOn(addon)
    }

    func load() async {
        do {
            list = try await core.call("defaultAddons", as: DefaultAddons.self)
            failure = nil
        } catch is CancellationError {
        } catch {
            failure = error.localizedDescription
        }
    }

    /// Continue: every switch in one write (the core then counts this list
    /// as seen, so a default removed later brings no card). A list not read
    /// yet is read first, so the switches are never sent empty. True once installed.
    @discardableResult
    func install() async -> Bool {
        guard !busy else { return false }
        busy = true
        defer { busy = false }
        if list == nil { await load() }
        guard list != nil else { return false }
        return await send(choices)
    }

    /// One default added, or switched where it is (the setup check's fix, Settings).
    func set(_ key: String, on: Bool) async -> Bool {
        guard await send([DefaultChoice(key: key, on: on)]) else { return false }
        await load()
        return true
    }

    private func send(_ choices: [DefaultChoice]) async -> Bool {
        do {
            try await core.perform("installDefaults", ["choices": choices.map(\.fields)])
            failure = nil
            return true
        } catch is CancellationError {
            return false
        } catch {
            failure = error.localizedDescription
            return false
        }
    }
}

/// Home's "New recommended addons" card and first-run routing: the profile's
/// addons and prefs, live, and the defaults read again when the list changes.
@MainActor @Observable
final class DefaultsOffer {
    let addons: LiveRecord<AddonsSnapshot>
    let prefs: LiveRecord<Prefs>
    private(set) var list: DefaultAddons?
    @ObservationIgnored private let core: CoreClient

    init(core: CoreClient) {
        self.core = core
        addons = LiveRecord("addons", core: core)
        prefs = LiveRecord("settings", core: core)
    }

    func start() {
        addons.start()
        prefs.start()
    }

    var fresh: Bool { addons.snapshot?.fresh == true }

    /// Reads the defaults again; the view calls it whenever the list changes.
    func refresh() async {
        guard addons.snapshot != nil else { return }
        if let list = try? await core.call("defaultAddons", as: DefaultAddons.self) { self.list = list }
    }

    func names(kids: Bool) -> [String]? {
        DefaultsRules.cardNames(list, seen: prefs.snapshot?.defaultsSeen, kids: kids, fresh: fresh)
    }

    /// Review and Not now: the card has been answered, on every device.
    func seen() async {
        guard let version = list?.version else { return }
        await prefs.perform("setPrefs", ["patch": ["defaultsSeen": version]])
    }
}

/// The addon link field's install: a link whose settings carry a sign-in is
/// refused by the core (`tokenInLink`) until the user says it is theirs.
@MainActor @Observable
final class LinkInstall {
    static let tokenWarning = "This link carries a sign-in: a token or a key. Whoever has it uses that account. Install it only if it is yours."
    static let tokenConfirm = "It is mine, install"

    /// Editing the link drops a refusal: the warning was about the old one.
    var link = "" {
        didSet { if link != oldValue { refused = nil } }
    }
    private(set) var installing = false
    private(set) var problem: String?
    /// The refused link, waiting for the user's word. May hold keys: never show or log it.
    private(set) var refused: String?
    @ObservationIgnored let core: CoreClient

    init(core: CoreClient) {
        self.core = core
    }

    var trimmed: String { link.trimmingCharacters(in: .whitespaces) }

    /// Installs the typed link; `acknowledged` sends the refused one again with the user's word.
    func install(acknowledged: Bool = false) async {
        guard !installing, let url = acknowledged ? refused : (trimmed.isEmpty ? nil : trimmed) else { return }
        installing = true
        defer { installing = false }
        refused = nil
        problem = nil
        var fields: [String: any Sendable] = ["url": url]
        if acknowledged { fields["acknowledgeToken"] = true }
        do {
            try await core.perform("installAddon", fields)
            if trimmed == url { link = "" }
        } catch let error as CoreError where error.code == "tokenInLink" && !acknowledged {
            refused = url
        } catch is CancellationError {
        } catch {
            problem = error.localizedDescription
        }
    }
}
