import Foundation
import Synchronization
import SwiftUI

/// Incognito (plans/batch-10-incognito.md): a throwaway profile the core keeps in a scope of its own and never lists
/// among the profiles. Its words (section 6), the rules the screens share, and the switch code no view reaches reads
/// (the art cache, the subtitle fetch, the trailer player).
enum Incognito {
    /// The session's profile id while it runs; never a real id (those are 32 hex digits).
    static let id = "incognito"

    /// Incognito as a profile, for every place that shows the current one (2.8, the app face).
    static let profile = ProfileInfo(id: id, name: "Incognito", avatar: id, position: 0, hasPin: false, isOwner: false, syncing: false, kids: false)

    private static let flag = Atomic<Bool>(false)

    /// Whether the session runs Incognito, readable from any thread; `ProfileSession` keeps it.
    static var on: Bool { flag.load(ordering: .relaxed) }

    static func set(_ on: Bool) {
        flag.store(on, ordering: .relaxed)
    }

    // MARK: words (section 6)

    static let name = "Incognito"
    static let spoken = "Profile: Incognito"
    static let opening = "Opening Incognito"
    static let heading = "You're Incognito"
    static let leave = "Leave Incognito"
    static let keptNothing = "Nothing you watch here is kept."
    static let addonsNote = "Changes here last until you leave Incognito."
    static let downloadNote = "This download stays on this device. Every profile can see it in Downloads."
    static let libraryEmpty = "Nothing here yet. Titles you add in Incognito are gone when you leave."
    static let ended = "Incognito ended. Its history and addons were cleared."
    static let trailersOff = "Off in Incognito."
    static let debrid = "Uses this device's debrid account"
    static let endsSoon = "Incognito ends in 1 minute."
    static let stay = "Stay in Incognito"
    static let kidsReason = "Kids profiles use this device, so Incognito needs this PIN."
    /// `noSpace` and `incognitoFailed`: never space words (plans/no-space-warnings.md).
    static let couldNotOpen = "Couldn't open Incognito. Try again."
    static let notAvailable = "Not available in Incognito."

    static func pinTitle(_ name: String) -> String { "Enter the PIN for \(name) to open Incognito" }

    /// Where the intro's last line differs.
    enum Form { case phone, tv, mac }

    struct Intro: Equatable {
        let notKept: [String]
        let seen: [String]
        let ends: String
    }

    /// The intro's two groups and its end line; `debrid` adds the debrid line.
    static func intro(debrid: Bool, form: Form) -> Intro {
        let ends = switch form {
        case .phone: "It ends when you leave, close the app, or after 30 minutes away."
        case .tv: "It ends when you leave, or after 30 minutes away."
        case .mac: "It ends when you leave, close the window, or after 30 minutes without use."
        }
        return Intro(
            notKept: ["What you watch and search.", "Addons and settings you change."],
            seen: ["Downloads stay on this device, and every profile can see them."]
                + (debrid ? ["Your debrid account keeps its own history."] : [])
                + ["Addons, your internet provider and torrent peers can still see what you play."],
            ends: ends)
    }

    // MARK: rules

    /// The phone tab's word under the avatar: the mark over "Incognito" (5.2).
    static func tabWord(_ profile: ProfileInfo?) -> String {
        profile?.id == id ? name : "Profile"
    }

    /// The chooser's tile and the bar menu's row: never in a kids session (Incognito is no way out of one).
    static func offered(_ snapshot: ProfilesSnapshot) -> Bool {
        snapshot.incognito || snapshot.current?.kids != true
    }

    /// The profile whose PIN entering Incognito asks for, by the core's rule (2.7); nil when none is asked.
    static func pinProfile(_ snapshot: ProfilesSnapshot) -> ProfileInfo? {
        snapshot.incognitoPin.flatMap { pin in snapshot.profiles.first { $0.id == pin } }
    }

    /// The PIN was asked by the kids rule, not because the session is locked: the keypad says why (5.1).
    static func kidsReason(_ snapshot: ProfilesSnapshot) -> Bool {
        snapshot.incognitoPin != nil && !snapshot.locked && snapshot.profiles.contains(where: \.kids)
    }

    /// What the tile or the menu row does: close while Incognito runs, its keypad when a PIN is asked, else the switch.
    static func choice(_ snapshot: ProfilesSnapshot) -> ProfileRules.Choice {
        if snapshot.incognito { return .close }
        return pinProfile(snapshot) == nil ? .switchTo : .pin
    }

    /// The title page's Trailer button: offline it cannot play, and in Incognito no trailer plays (5.5).
    static func trailerShown(hasTrailers: Bool, offline: Bool, incognito: Bool) -> Bool {
        hasTrailers && !offline && !incognito
    }

    /// iOS covers the app in the app switcher: always in the background, and when inactive only with the player off
    /// screen (a film keeps its picture under Control Center).
    static func covers(_ phase: ScenePhase, playerOnScreen: Bool) -> Bool {
        switch phase {
        case .background: true
        case .inactive: !playerOnScreen
        default: false
        }
    }

    /// The second subtitle's fetch: in Incognito a session that keeps nothing on disk (no `Cache.db` entry names the
    /// title's `/sub/` path).
    static func subtitleSession(incognito: Bool) -> URLSession {
        incognito ? ephemeral : .shared
    }

    private static let ephemeral = URLSession(configuration: .ephemeral)

    /// When the window was last given a playing video's progress; the Mac's idle sender reads it.
    @MainActor static var videoAt: ContinuousClock.Instant?

    /// The player reported playing progress (every 5 s while a video plays).
    @MainActor static func videoPlaying() {
        videoAt = .now
    }
}

/// The Mac's idle sender (2.7): `idle` after 60 seconds with no input and no video playing, `active` on the next
/// input. The window's events and a clock tick drive it; it says what to send, once per change.
struct IdleClock {
    static let after: Duration = .seconds(60)

    private(set) var idle = false
    private var last: ContinuousClock.Instant

    init(now: ContinuousClock.Instant = .now) {
        last = now
    }

    /// Input in the window: `active` when it was idle.
    mutating func input(at now: ContinuousClock.Instant) -> String? {
        last = now
        guard idle else { return nil }
        idle = false
        return "active"
    }

    /// The window was hidden or the screen slept: `idle` at once, unless a video plays.
    mutating func away(playing: Bool) -> String? {
        guard !idle, !playing else { return nil }
        idle = true
        return "idle"
    }

    /// A tick: `idle` once a minute passed with no input; a playing video counts as use.
    mutating func tick(at now: ContinuousClock.Instant, playing: Bool) -> String? {
        if playing { last = now }
        guard !idle, now - last >= Self.after else { return nil }
        idle = true
        return "idle"
    }
}

/// The Mac's one-minute warning at the page's foot (B r3 P2-1): the line and "Stay in Incognito", which sits in the
/// window's key loop (Tab reaches it) and never takes focus itself. Said once as it appears.
struct IncognitoEndsSoon: View {
    let stay: () -> Void

    var body: some View {
        HStack(spacing: Theme.Space.s) {
            Text(Incognito.endsSoon)
                .textFont(Theme.type.body, weight: .semibold)
                .foregroundStyle(Theme.Palette.text)
            Button(Incognito.stay, action: stay)
                .buttonStyle(PillButtonStyle())
                // Tab reaches it whatever the Mac's keyboard navigation setting; nothing focuses it for the person.
                .focusable(interactions: .activate)
        }
        .padding(.horizontal, Theme.Space.m)
        .padding(.vertical, Theme.Space.s)
        .flatCard()
        .accessibilityElement(children: .contain)
        .onAppear { AccessibilityNotification.Announcement(Incognito.endsSoon).post() }
    }
}

#if os(macOS)
/// The Mac's Incognito idle sender (2.7): `idle` after 60 seconds with no input in the app's windows and no video
/// playing, at once when the app is hidden or the screens sleep, and `active` on the next input; the wake is sent as
/// `active` too. Only while Incognito runs (`running`); outside it the core ignores both. Any input answers the
/// one-minute warning, as its button does (`stay`).
@MainActor final class IncognitoIdle {
    private var clock = IdleClock()
    private let running: () -> Bool
    private let warned: () -> Bool
    private let stay: () -> Void
    private let send: (String) -> Void
    private var monitor: Any?
    // ponytail: one for the app's life, never removed; a second one would need a stop.
    private var observers: [NSObjectProtocol] = []
    private var ticking: Task<Void, Never>?

    init(running: @escaping () -> Bool, warned: @escaping () -> Bool, stay: @escaping () -> Void, send: @escaping (String) -> Void) {
        (self.running, self.warned, self.stay, self.send) = (running, warned, stay, send)
        let events: NSEvent.EventTypeMask = [.keyDown, .leftMouseDown, .rightMouseDown, .otherMouseDown, .scrollWheel, .mouseMoved]
        monitor = NSEvent.addLocalMonitorForEvents(matching: events) { [weak self] event in
            self?.input()
            return event
        }
        let workspace = NSWorkspace.shared.notificationCenter
        observe(workspace, NSWorkspace.didWakeNotification) { $0.woke() }
        observe(workspace, NSWorkspace.screensDidSleepNotification) { $0.away() }
        observe(.default, NSApplication.didHideNotification) { $0.away() }
        ticking = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(5))
                self?.tick()
            }
        }
    }

    private func observe(_ center: NotificationCenter, _ name: Notification.Name, _ run: @escaping @Sendable @MainActor (IncognitoIdle) -> Void) {
        let observer = center.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { if let self { run(self) } }
        }
        observers.append(observer)
    }

    private var playing: Bool {
        Incognito.videoAt.map { ContinuousClock.now - $0 < .seconds(15) } ?? false
    }

    private func emit(_ event: String?) {
        guard let event, running() else { return }
        send(event)
    }

    func input() {
        let event = clock.input(at: .now)
        if warned() { stay() } else { emit(event) }
    }

    private func away() { emit(clock.away(playing: playing)) }

    private func tick() { emit(clock.tick(at: .now, playing: playing)) }

    /// The Mac slept: the core weighs the wall clock against the time it went idle before anything else (2.7).
    private func woke() {
        _ = clock.input(at: .now)
        emit("active")
    }
}
#endif
