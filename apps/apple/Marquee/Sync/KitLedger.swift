import Foundation
import SwiftUI

/// A recovery kit made here and not yet said saved: its id in the device
/// list ("" when the list did not tell) and its letters, in memory only.
struct HeldKit: Equatable, CustomStringConvertible {
    let id: String
    let kit: String

    /// Never the letters, wherever a value is printed or logged.
    var description: String { "HeldKit(id: \(id))" }
}

/// The kits made here and not yet said saved (Android's SyncAccount): the
/// one whose letters are held in memory, and the ids of every one still
/// pending, which `SyncModel` keeps across launches (never a kit). A kit
/// whose letters went before "I saved it" is unseen: nobody holds it, and
/// the next kit made here removes it.
struct KitLedger: Equatable {
    /// A pending kit whose id could not be read.
    static let unknown = "?"
    /// How long the letters stay once the kit's page is left.
    static let hold: Duration = .seconds(300)

    private(set) var held: HeldKit?
    private(set) var pending: [String]
    /// The last kit to go unseen did so because its hold ran out (not because the app closed).
    private(set) var expired = false

    init(pending: [String] = []) {
        self.pending = pending
    }

    private var heldEntry: String? { held.map { $0.id.isEmpty ? Self.unknown : $0.id } }

    private var others: [String] {
        var rest = pending
        if let entry = heldEntry, let i = rest.firstIndex(of: entry) { rest.remove(at: i) }
        return rest
    }

    /// Unseen kits with a known id: the next kit made here removes them.
    var unseen: [String] { others.filter { $0 != Self.unknown } }

    /// An unseen kit nobody could name: the page asks the user to check the list, never claims it gone.
    var unknownUnseen: Bool { others.contains(Self.unknown) }

    mutating func hold(_ kit: HeldKit) {
        held = kit
        expired = false
        pending.append(kit.id.isEmpty ? Self.unknown : kit.id)
    }

    /// "I saved it": the letters leave memory and nothing marks the kit pending.
    mutating func saved() {
        if let entry = heldEntry, let i = pending.firstIndex(of: entry) { pending.remove(at: i) }
        held = nil
    }

    /// The hold ran out for `kit`; a newer kit held since stays. By its letters:
    /// the kit may have been named since its hold began.
    mutating func expire(_ kit: HeldKit) {
        guard held?.kit == kit.kit else { return }
        held = nil
        expired = true
    }

    /// A pending kit is removed, or gone from the list: it leaves the ledger, and memory if held.
    mutating func settle(_ id: String) {
        if held?.id == id { held = nil }
        pending.removeAll { $0 == id }
    }

    /// A pending kit whose row left the list needs no saving and no removing.
    mutating func follow(_ members: [MemberInfo]) {
        let ids = Set(members.map(\.id))
        for id in pending where id != Self.unknown && !ids.contains(id) { settle(id) }
    }

    /// The user checked the list for a kit nobody could name.
    mutating func checkedUnknown() {
        let keep = heldEntry == Self.unknown
        pending.removeAll { $0 == Self.unknown }
        if keep { pending.append(Self.unknown) }
    }

    /// A join or a leave: kits of the old account mean nothing now.
    mutating func drop() {
        self = KitLedger()
    }
}

/// When a kit on screen is covered (the OS prompt shows it again).
enum KitRules {
    static let mac: Bool = {
        #if os(macOS)
        true
        #else
        false
        #endif
    }()

    /// Phone and tablet: whenever the app is not in front, as the app switcher
    /// takes its picture then and another app or the lock screen is up. Mac:
    /// when the window is hidden or minimized; a window that is only not the
    /// key one stays readable beside the password manager it is typed into.
    static func covers(_ phase: ScenePhase, mac: Bool = mac) -> Bool {
        mac ? phase == .background : phase != .active
    }
}

/// Whether the kit page may show the held kit: only after the OS prompt said
/// it is the owner (or right after making it, behind the same prompt), and
/// only when the page stayed in view while the prompt and the server
/// answered. The prompt's own scene change (on a device the app goes
/// inactive under it) neither covers the kit nor asks again; the background,
/// another tab or leaving the page does. On a Mac, where no API keeps a
/// window out of every screen recording, the kit shows for `macWindow` after
/// each reveal, or while Show is held.
struct KitGate: Equatable {
    static let macWindow: TimeInterval = 20
    /// Mac: how long after the prompt Hold to show still works (once per prompt).
    static let macHold: TimeInterval = 60

    /// The owner was confirmed and the page stayed in view since.
    private(set) var unlocked = false
    /// The OS prompt (or the server, for a new kit) has not answered yet.
    private(set) var asking = false
    /// The page left view while asking: the answer shows nothing.
    private(set) var coveredWhileAsking = false
    /// Mac: when the kit hides again.
    private(set) var until: Date?
    /// Mac: when Hold to show stops working, until the next prompt.
    private(set) var holdUntil: Date?

    mutating func ask() {
        asking = true
        coveredWhileAsking = false
    }

    /// The prompt answered (and, for a new kit, the kit arrived).
    mutating func answered(ok: Bool, now: Date = .now) {
        asking = false
        guard ok, !coveredWhileAsking else { return }
        unlocked = true
        until = now.addingTimeInterval(Self.macWindow)
        holdUntil = now.addingTimeInterval(Self.macHold)
    }

    /// The page left view: the background, another tab, leaving it.
    mutating func cover() {
        unlocked = false
        until = nil
        holdUntil = nil
        if asking { coveredWhileAsking = true }
    }

    /// The scene changed. While the prompt is up only the background covers.
    mutating func phase(_ phase: ScenePhase, mac: Bool = KitRules.mac) {
        if asking ? phase == .background : KitRules.covers(phase, mac: mac) { cover() }
    }

    /// The scene is about to go inactive: the prompt's own doing while asking, else leaving.
    mutating func deactivating() {
        if !asking { cover() }
    }

    /// Mac: whether the kit shows now (inside the window after a reveal, or held within a minute of it).
    func shows(now: Date = .now, holding: Bool, mac: Bool = KitRules.mac) -> Bool {
        guard unlocked else { return false }
        return !mac || (until.map { now < $0 } ?? false) || (holding && canHold(now: now))
    }

    /// Mac: Hold to show still works (a minute after the prompt); after that only a new prompt shows the kit.
    func canHold(now: Date = .now) -> Bool {
        unlocked && (holdUntil.map { now < $0 } ?? false)
    }

    #if DEBUG
    /// The made-up preview kit only (`DebugHooks.kitPreview`).
    mutating func previewUnlock(now: Date = .now) {
        unlocked = true
        until = now.addingTimeInterval(Self.macWindow)
        holdUntil = now.addingTimeInterval(Self.macHold)
    }
    #endif
}
