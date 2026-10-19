import Foundation
import Observation

/// The recommended addons (plans/m13-default-addons.md sections 6 and 7):
/// the core holds the nine and installs them; the app carries no addon links.
enum DefaultsRules {
    /// What the step lists: every default (first run, after an import, the
    /// setup check), or only the ones missing from the list (the Home card).
    enum Scope { case all, missing }

    /// Onboarding's groups, in order, with their headings.
    static let groups: [(id: String, title: String)] = [
        ("catalogs", "Catalogs and details"), ("streams", "Streams"), ("subtitles", "Subtitles"), ("lists", "Lists"),
    ]
    static let title = "Recommended addons"
    static let lead = "Marquee starts with these. Turn off any you do not want; you can change this later in Settings."
    static let catalogKey = "cinemeta"
    static let catalogWarning = "Home and search need a catalog addon."

    static func shown(_ list: [DefaultAddonInfo], scope: Scope) -> [DefaultAddonInfo] {
        scope == .all ? list : list.filter { $0.installed == .absent }
    }

    /// A default already in the list and on has no switch: it stays as it is.
    static func hasSwitch(_ addon: DefaultAddonInfo) -> Bool { addon.installed != .on }

    /// Where a switch starts: a missing default on, one in the list but off stays off.
    static func starts(_ addon: DefaultAddonInfo) -> Bool { addon.installed != .off }

    /// The word in place of a switch.
    static func presentWord(imported: Bool) -> String { imported ? "From Stremio" : "Added" }

    /// Every default's choice for `installDefaults`, which takes a key left
    /// out as on: a switch the step shows sends its position, and every other
    /// default what the list has, so one in the list and off stays off.
    static func choices(_ list: [DefaultAddonInfo], scope: Scope, moved: [String: Bool]) -> [DefaultChoice] {
        let shown = Set(shown(list, scope: scope).filter(hasSwitch).map(\.key))
        return list.map { addon in
            DefaultChoice(key: addon.key, on: shown.contains(addon.key) ? moved[addon.key] ?? starts(addon) : addon.installed != .off)
        }
    }

    static func warns(_ choices: [DefaultChoice]) -> Bool {
        choices.first { $0.key == catalogKey }?.on == false
    }

    /// The Home card's names: the defaults the profile misses, once per list
    /// version, never on a kids profile (it reads the owner's list) or at
    /// first run (onboarding offers them all). Nil hides the card.
    static func cardNames(_ list: DefaultAddons?, seen: Int?, kids: Bool, fresh: Bool) -> [String]? {
        guard let list, let seen, seen < list.version, !kids, !fresh else { return nil }
        let names = list.addons.filter { $0.installed == .absent }.map(\.name)
        return names.isEmpty ? nil : names
    }

    /// The setup check's fix for a line with nothing on.
    enum Fix: Equatable {
        /// The group's default is in the list but off, and none is missing.
        case turnOn(key: String, name: String)
        /// Some default is missing: installing appends every missing one, so the user sees their switches first.
        case review
    }

    static func fix(group: String, in list: DefaultAddons?) -> Fix? {
        guard let addons = list?.addons else { return nil }
        if addons.contains(where: { $0.installed == .absent }) { return .review }
        return addons.first { $0.group == group && $0.installed == .off }.map { .turnOn(key: $0.key, name: $0.name) }
    }
}

/// One switch sent with `installDefaults`.
struct DefaultChoice: Equatable, Sendable {
    let key: String
    let on: Bool

    var fields: [String: any Sendable] { ["key": key, "on": on] }
}

/// The recommended addons step: the list from the core, the switches the
/// user moved, and Continue.
@MainActor @Observable
final class DefaultsStep {
    let scope: DefaultsRules.Scope
    private(set) var list: DefaultAddons?
    private(set) var failure: String?
    private(set) var busy = false
    /// Switches the user moved, by key; the rest sit where they start.
    private(set) var moved: [String: Bool] = [:]
    @ObservationIgnored let core: CoreClient

    init(core: CoreClient, scope: DefaultsRules.Scope) {
        self.core = core
        self.scope = scope
    }

    var rows: [DefaultAddonInfo] { DefaultsRules.shown(list?.addons ?? [], scope: scope) }
    var choices: [DefaultChoice] { DefaultsRules.choices(list?.addons ?? [], scope: scope, moved: moved) }

    func isOn(_ addon: DefaultAddonInfo) -> Bool { moved[addon.key] ?? DefaultsRules.starts(addon) }

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

    /// Continue: every switch in one write. A list not read yet is read
    /// first, so a default in the list keeps its state. The profile has then
    /// seen this list: one removed later brings no card. True once installed.
    @discardableResult
    func install() async -> Bool {
        guard !busy else { return false }
        busy = true
        defer { busy = false }
        if list == nil { await load() }
        guard let version = list?.version else { return false }
        do {
            try await core.perform("installDefaults", ["choices": choices.map(\.fields)])
            failure = nil
            // ponytail: a failed write only means the card may show once.
            try? await core.perform("setPrefs", ["patch": ["defaultsSeen": version]])
            return true
        } catch is CancellationError {
            return false
        } catch {
            failure = error.localizedDescription
            return false
        }
    }

    /// The setup check's one-press fix: that default on, the rest as they are.
    func turnOn(_ key: String) async -> Bool {
        await load()
        moved = [key: true]
        return await install()
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
    static let tokenWarning = "This link carries a sign-in in its settings, maybe someone else's. Add it only if it is yours."
    static let tokenConfirm = "It is mine, add it"

    var link = ""
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
