import Foundation

// Profiles (plans/m11-profiles.md, section 6 and P1 apps), mirroring
// apps/web/src/core/generated/ProfilesSnapshot.ts and ProfileInfo.ts.

struct ProfilesSnapshot: Decodable, Hashable, Sendable {
    let profiles: [ProfileInfo]
    /// The session's profile (it may be gone: then the session is locked).
    let active: String
    /// Only the picker and unlocking work.
    let locked: Bool
    /// How many profiles `createProfile` allows.
    let max: Int
    /// A sign-out that removes the library waits for its steps 2 and 3 (D13): the core holds it.
    var wipePending: WipePending? = nil
    /// Incognito runs, entered from this profile (plans/batch-10-incognito.md 2.8): `active` is `incognito` meanwhile,
    /// and `profiles` never lists it.
    var incognitoHost: String? = nil
    /// The profile whose PIN entering Incognito asks for; nil when none is asked.
    var incognitoPin: String? = nil
    /// An Incognito end this session has not read yet (the ended line); it clears on the next profile op or
    /// `ackIncognitoEnded`.
    var incognitoEnded: Bool? = nil
    /// First run's profile step was answered on this device (`finishProfileStep`, plans/first-screens.md 4.2).
    /// Optional so an older core's snapshot still decodes; read it through `stepDone`.
    var profileStepDone: Bool? = nil

    var incognito: Bool { incognitoHost != nil }

    /// The profile step is answered: an older core that sends no field counts as answered, so it never shows the form.
    var stepDone: Bool { profileStepDone ?? true }

    /// The session's profile: Incognito while it runs (the app face, 2.8), else the active one listed.
    var current: ProfileInfo? { incognito ? Incognito.profile : profiles.first { $0.id == active } }
}

/// The removal the core holds (`signOut {removeAfter}`) until its wipe answers.
struct WipePending: Decodable, Hashable, Sendable {
    /// The debrid accounts go too.
    let debrid: Bool
}

struct ProfileInfo: Decodable, Hashable, Sendable, Identifiable {
    let id: String
    let name: String
    /// `glyph:color` as some app stored it; kept as it came.
    let avatar: String
    let position: Int
    let hasPin: Bool
    let isOwner: Bool
    /// Its PIN record has not arrived from another device yet: shown, not openable.
    let syncing: Bool
    /// A kids profile: it reads the owner's addons.
    var kids = false
}

/// Profile rules the screens share, equal to the web's `lib/profiles.ts` and
/// `lib/avatar.ts` and Android's `ProfileCopy`.
enum ProfileRules {
    static let glyphs = ["letter", "popcorn", "clapperboard", "tv", "rocket", "ghost", "cat", "dog", "leaf", "moon", "flame", "headphones", "gamepad"]
    static let colors = ["ember", "amber", "moss", "teal", "sky", "plum", "rose", "graphite"]

    /// The editor holds changes not saved yet: a typed name that differs from what was last saved
    /// (`saved`, newer than the snapshot right after a Save) or stored (a new profile has none), or a
    /// picked picture. A profile that is gone (removed on another device) has nothing left to save.
    static func isDirty(name: String, stored: String?, saved: String? = nil, adding: Bool, touched: Bool) -> Bool {
        if !adding, stored == nil { return false }
        let baseline = (saved ?? stored ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        return touched || name.trimmingCharacters(in: .whitespacesAndNewlines) != baseline
    }

    struct Avatar: Equatable {
        let glyph: String
        /// nil draws on `raised`.
        let color: String?
    }

    /// An unknown or missing glyph is the letter; an unknown color is nil
    /// (drawn on `raised`); never chosen (empty) is the letter on ember.
    static func parseAvatar(_ stored: String) -> Avatar {
        guard !stored.isEmpty else { return Avatar(glyph: "letter", color: "ember") }
        // Incognito's mark: the hat-glasses glyph on sea (plans/batch-10-incognito.md 5.2).
        if stored == Incognito.id { return Avatar(glyph: Incognito.id, color: Incognito.id) }
        let parts = stored.split(separator: ":", omittingEmptySubsequences: false).map(String.init)
        let glyph = parts.first.flatMap { glyphs.contains($0) ? $0 : nil } ?? "letter"
        let color = parts.count > 1 && colors.contains(parts[1]) ? parts[1] : nil
        return Avatar(glyph: glyph, color: color)
    }

    static func formatAvatar(glyph: String, color: String) -> String { "\(glyph):\(color)" }

    /// The name's first character, upper case, as the letter avatar draws it.
    static func initial(_ name: String) -> String {
        name.trimmingCharacters(in: .whitespaces).first.map { String($0).uppercased() } ?? "?"
    }

    /// A new profile's avatar: the letter on the first color no profile uses
    /// (an empty avatar uses ember), else the next in turn.
    static func nextAvatar(_ existing: [String]) -> String {
        let used = Set(existing.map { parseAvatar($0).color })
        let color = colors.first { !used.contains($0) } ?? colors[existing.count % colors.count]
        return formatAvatar(glyph: "letter", color: color)
    }

    /// What choosing a profile does, from a picker tile or the bar's profile menu (plans/nav-profile.md): nothing for
    /// one still syncing, close for the current one (unlocked), its keypad for one with a PIN, else the switch.
    enum Choice: Equatable { case none, close, pin, switchTo }

    static func choice(_ profile: ProfileInfo, in snapshot: ProfilesSnapshot) -> Choice {
        if profile.syncing { return .none }
        if profile.id == snapshot.active && !snapshot.locked { return .close }
        return profile.hasPin ? .pin : .switchTo
    }

    /// "This profile is locked": only while the session is locked and its profile has a PIN, on every path (a Leave
    /// from a chooser entry locks a host that may have none; plans/batch-10-incognito.md 5.4).
    static func lockLine(_ snapshot: ProfilesSnapshot) -> Bool {
        snapshot.locked && snapshot.current?.hasPin == true
    }

    /// At a cold start: the picker when the session is locked or more than
    /// one profile exists, else Home.
    static func startsOnPicker(_ snapshot: ProfilesSnapshot) -> Bool {
        snapshot.locked || snapshot.profiles.count > 1
    }

    /// "4:59" for the wait after too many tries, rounded up so it never shows
    /// 0:00 while still waiting.
    static func waitText(_ ms: Int64) -> String {
        let total = max(0, (ms + 999) / 1000)
        return "\(total / 60):\(String(format: "%02d", total % 60))"
    }

    /// "Too many tries. Try again in 5 minutes.": said once, when the wait starts.
    static func waitSentence(_ ms: Int64) -> String {
        let minutes = max(0, (ms + 59_999) / 60_000)
        return "Too many tries. Try again in \(minutes) \(minutes == 1 ? "minute" : "minutes")."
    }

    /// The profile codes the core sends, in words.
    static func problem(_ code: String?, fallback: String) -> String {
        switch code {
        case "wrongPin": "Wrong PIN"
        case "weakPin": "Pick a less obvious PIN: no repeats, runs or 1234."
        case "badName": "Names are 1 to 30 characters."
        case "tooMany": "This account has as many profiles as it can hold."
        case "notOwner": "Only the main profile can do that."
        case "viewsOpen": "Something is still open. Try again."
        case "unknownProfile": "That profile was removed on another device."
        // The owner's PIN guards the kids profiles: it stays while one exists.
        case "kidsNeedPin": "Kids profiles need this PIN. Remove them first, then remove the PIN."
        case "pinUnreadable": "This PIN was set by a newer version. Remove it, then set a new one."
        // A full disk at the entry reads as a quiet failure: no space words on any screen (plans/no-space-warnings.md).
        case "noSpace", "incognitoFailed": Incognito.couldNotOpen
        case "incognito": Incognito.notAvailable
        default: fallback
        }
    }

    /// The spoken name of a glyph choice.
    static func glyphName(_ glyph: String) -> String {
        switch glyph {
        case "letter": "First letter"
        case "gamepad": "Game controller"
        default: glyph.prefix(1).uppercased() + glyph.dropFirst()
        }
    }

    /// A tile as spoken: "Kid, current profile, locked with a PIN", or "Kid, still syncing".
    static func spoken(_ profile: ProfileInfo, active: String) -> String {
        var parts = [profile.name]
        if profile.id == active { parts.append("current profile") }
        if profile.syncing { parts.append("still syncing") } else if profile.hasPin { parts.append("locked with a PIN") }
        return parts.joined(separator: ", ")
    }

    /// A Manage row's detail: "Main profile · PIN set", or "No PIN".
    static func detail(_ profile: ProfileInfo) -> String {
        var parts: [String] = []
        if profile.isOwner { parts.append("Main profile") }
        if profile.syncing { parts.append("Syncing") } else if profile.hasPin { parts.append("PIN set") }
        return parts.isEmpty ? "No PIN" : parts.joined(separator: " \u{B7} ")
    }
}

/// The PIN changes the core allows (its `set_pin`): your own profile with no
/// PIN can set one; with one, change or remove it, each asking the current
/// PIN first; the owner can remove another profile's PIN; nobody sets
/// another's. A PIN a newer version stored (`pinUnreadable`) can only be
/// removed, without asking it.
enum PinAction: Equatable, Sendable {
    case set, change, remove, removeOther, removeUnreadable

    static func allowed(for target: ProfileInfo, by active: ProfileInfo, unreadable: Bool = false) -> [PinAction] {
        if target.syncing { return [] }
        if target.id == active.id {
            if unreadable { return [.removeUnreadable] }
            return target.hasPin ? [.change, .remove] : [.set]
        }
        return active.isOwner && target.hasPin ? [.removeOther] : []
    }

    /// Removing a PIN with no keypad step in front of it asks first (`.remove` has the keypad).
    var confirms: Bool { self == .removeOther || self == .removeUnreadable }
}

/// One PIN change on the keypad, step by step: which prompt shows, and the
/// `setPin` fields once the steps are done. A second entry that differs from
/// the first starts over from the first entry.
struct PinFlow: Equatable, Sendable {
    enum Step: Equatable, Sendable { case current, new, again }

    /// What goes to `setPin`: the new PIN (nil removes it) and the current one.
    struct Submit: Equatable, Sendable {
        let pin: String?
        let currentPin: String?
    }

    let action: PinAction
    /// This flow's identity: an answer for an earlier flow never touches a newer one.
    let id = UUID()
    private(set) var step: Step
    private(set) var note: String?
    private var current: String?
    private var first: String?

    init(_ action: PinAction) {
        self.action = action
        step = action == .set ? .new : .current
    }

    var prompt: String {
        switch step {
        case .current: "Enter your current PIN"
        case .new: action == .set ? "Choose a PIN" : "Enter the new PIN"
        case .again: "Enter it again"
        }
    }

    /// Takes the four digits of the step on screen; the fields for `setPin`
    /// once the flow is complete, else nil and the next step.
    mutating func enter(_ pin: String) -> Submit? {
        switch step {
        case .current:
            if action == .remove { return Submit(pin: nil, currentPin: pin) }
            current = pin
            step = .new
        case .new:
            first = pin
            note = nil
            step = .again
        case .again:
            if pin == first { return Submit(pin: pin, currentPin: current) }
            first = nil
            note = "The PINs did not match. Choose one again."
            step = .new
        }
        return nil
    }

    /// The core refused the current PIN: ask it again.
    mutating func wrongCurrent() {
        current = nil
        first = nil
        step = action == .set ? .new : .current
    }

    /// The core found the new PIN too obvious: choose another.
    mutating func weak() {
        first = nil
        step = .new
    }

    /// The flow on screen once `setPin` answered for the flow `sent`: closed
    /// when it went through, a step back for a wrong or too obvious PIN. A
    /// newer flow, or none, stays as it is: the page says how the save ended.
    static func settle(_ onScreen: PinFlow?, sent: PinFlow, ok: Bool, code: String?) -> PinFlow? {
        guard let onScreen, onScreen.id == sent.id else { return onScreen }
        if ok { return nil }
        var next = onScreen
        if code == "wrongPin" { next.wrongCurrent() } else if code == "weakPin" { next.weak() }
        return next
    }
}

/// How one PIN try ended, as the keypad shows it.
enum KeypadResult: Equatable, Sendable {
    case ok
    /// "Wrong PIN": the digits clear.
    case wrong
    /// Too many tries: no try until this many milliseconds have passed.
    case wait(Int64)
    case problem(String)

    /// A core refusal sorted: `wrongPin`, `locked` with its wait, else words.
    static func of(code: String?, retryAfterMs: Int64?, message: String) -> KeypadResult {
        if code == "wrongPin" { return .wrong }
        if code == "locked", let retryAfterMs { return .wait(retryAfterMs) }
        return .problem(ProfileRules.problem(code, fallback: message.isEmpty ? "That did not work." : message))
    }
}

/// What a keypad's delete does, as the web's: the key on screen only deletes; the keyboard's Delete, which a keyboard
/// has no other key for, leaves the keypad once there is nothing left to delete.
enum KeypadDelete: Equatable {
    case removeLast, leave, nothing

    static func of(digits: String, onScreen: Bool) -> KeypadDelete {
        if !digits.isEmpty { return .removeLast }
        return onScreen ? .nothing : .leave
    }
}
