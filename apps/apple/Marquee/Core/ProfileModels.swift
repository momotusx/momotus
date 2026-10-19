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

    var current: ProfileInfo? { profiles.first { $0.id == active } }
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

    struct Avatar: Equatable {
        let glyph: String
        /// nil draws on `raised`.
        let color: String?
    }

    /// An unknown or missing glyph is the letter; an unknown color is nil
    /// (drawn on `raised`); never chosen (empty) is the letter on ember.
    static func parseAvatar(_ stored: String) -> Avatar {
        guard !stored.isEmpty else { return Avatar(glyph: "letter", color: "ember") }
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
        case "pinUnreadable": "This PIN was set by a newer version. Remove it, then set a new one."
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
