import Foundation
import Observation

/// The session's profile for the app's life (plans/m11-profiles.md, P1 apps
/// A, B and v2 3 to 6): one `profiles` view kept open across switches and
/// locks, whether profile content may show, and the switch.
///
/// While the gate is shut the app shows only the picker: the screens of the
/// old profile have left the hierarchy, so their views close before the core
/// is asked to switch. The switch runs in this object's own task, never a
/// view's, so a picker that leaves with the gate cannot stop it half way.
@MainActor @Observable
final class ProfileSession {
    let view: LiveRecord<ProfilesSnapshot>
    /// The start picker was answered this launch (decided on the first snapshot).
    private(set) var chosen = false
    /// A switch under way: the profile asked for. The gate is shut until it ends.
    private(set) var switching: String?
    /// Why the last switch failed; the picker that takes over shows it.
    private(set) var switchProblem: CoreError?
    /// Bumped by every switch: the profile's screens are built again from nothing.
    private(set) var epoch = 0
    /// A switch request is with the core: no second switch starts, and Cancel
    /// waits for its answer (the core may still commit it).
    private(set) var inFlight = false
    /// A profile removed on another device, and how many times one was: the
    /// picker closes its keypad and says so.
    private(set) var gone: (id: String, count: Int)?

    @ObservationIgnored private let core: CoreClient
    @ObservationIgnored private let sync: SyncModel
    @ObservationIgnored private var decided = false
    /// What `chosen` was when the switch under way started: Cancel puts it back,
    /// so a start picker that was never answered asks again.
    @ObservationIgnored private var chosenBefore = false
    /// The profile's screens were on onboarding: a restarted core opens it
    /// again; a switch or a lock starts on Home.
    @ObservationIgnored var resumeOnboarding = false
    @ObservationIgnored private var retrying: Task<Void, Never>?
    /// The profile's screens are on screen: the root says when they appear and
    /// when they have left, and a switch waits for the second.
    @ObservationIgnored private var shellOnScreen = false
    /// The core generation the last snapshot came from: after a restart
    /// nothing of the profile shows until the new core says which one runs.
    private var seen = -1

    init(core: CoreClient, sync: SyncModel) {
        self.core = core
        self.sync = sync
        view = LiveRecord("profiles", core: core)
        view.onSnapshot = { [weak self] snapshot in self?.saw(snapshot) }
        core.onProfileGone = { [weak self] id in self?.removed(id) }
        view.start()
        // A profiles view that failed to open (at launch or after a restart)
        // is opened again every 2 seconds; the root says so meanwhile.
        retrying = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(2))
                guard let self else { return }
                if self.view.failure != nil { self.view.retry() }
            }
        }
    }

    /// Why the profiles view did not open, while nothing of a profile can show.
    var loadProblem: String? { snapshot == nil ? view.failure : nil }

    var snapshot: ProfilesSnapshot? { seen == core.generation ? view.snapshot : nil }

    /// Whether the app may show profile content, or only the picker. Locked
    /// always wins; a stopped core shuts it too, until the new one says.
    var open: Bool {
        guard core.failure == nil, let snapshot else { return false }
        return !snapshot.locked && chosen && switching == nil
    }

    private func saw(_ snapshot: ProfilesSnapshot) {
        seen = core.generation
        // A lock starts the profile on Home again, as a switch does.
        if snapshot.locked { resumeOnboarding = false }
        guard !decided else { return }
        decided = true
        if !ProfileRules.startsOnPicker(snapshot) { chosen = true }
    }

    private func removed(_ id: String) {
        gone = (id, (gone?.count ?? 0) + 1)
    }

    /// The root: the profile's screens appeared, or left.
    func shell(onScreen: Bool) {
        shellOnScreen = onScreen
    }

    /// The current profile, unlocked: the start picker's answer.
    func markChosen() {
        chosen = true
    }

    /// Unlocks the locked session as its own profile; throws the core's refusal.
    func unlock(pin: String?) async throws {
        try await core.perform("unlockProfile", pin.map { ["pin": $0] } ?? [:])
        chosen = true
        switchProblem = nil
    }

    /// The owner forgot the PIN: cleared behind the device's own lock (asked
    /// by the caller; the core takes it from any session), then the owner
    /// opens: unlocked when it is the locked session's own profile, else by
    /// a switch.
    func resetOwnerPin(owner: String) async throws {
        try await core.perform("resetOwnerPin")
        if snapshot?.active == owner, snapshot?.locked == true {
            try await unlock(pin: nil)
        } else if await !switchTo(owner, pin: nil) {
            throw switchProblem ?? CoreError("The switch did not finish.")
        }
    }

    /// Shuts the gate, so every screen of the old profile leaves and closes
    /// its views, then switches, waiting while views finish closing
    /// (`viewsOpen`, up to 10 tries 100 ms apart). A refusal leaves the gate
    /// shut with `switchProblem` for the picker; `cancelSwitch` goes back.
    @discardableResult
    func switchTo(_ id: String, pin: String?) async -> Bool {
        guard !inFlight else { return false }
        // Out from now until the last answer: a second ask, or Cancel, waits.
        inFlight = true
        if switching == nil { chosenBefore = chosen }
        resumeOnboarding = false
        switching = id
        switchProblem = nil
        epoch += 1
        // Device-level views a profile does not own still count as open.
        sync.endPairing()
        // The session's own task: the caller's view leaves with the gate.
        let work = Task { await runSwitch(id, pin: pin) }
        return await work.value
    }

    private func runSwitch(_ id: String, pin: String?) async -> Bool {
        var fields: [String: any Sendable] = ["profile": id]
        if let pin { fields["pin"] = pin }
        defer { inFlight = false }
        // The old profile's screens have left (the root acknowledges it), so
        // their views are closed or closing before the core is asked.
        for _ in 0..<50 where shellOnScreen {
            try? await Task.sleep(for: .milliseconds(100))
        }
        // Still there after 5 s: the switch is not sent; the picker says so, with Cancel.
        if shellOnScreen {
            coreLog.error("switch: the old profile's screens did not leave in 5 s")
            switchProblem = CoreError("The old profile's screens did not close.", code: "viewsOpen")
            return false
        }
        for attempt in 1...10 {
            do {
                try await core.perform("switchProfile", fields)
                chosen = true
                switching = nil
                return true
            } catch let error as CoreError where error.code == "viewsOpen" && attempt < 10 {
                try? await Task.sleep(for: .milliseconds(100))
            } catch let error as CoreError {
                if error.code == "viewsOpen" {
                    coreLog.error("switch: views still open: \((error.views ?? []).joined(separator: ", "), privacy: .public)")
                }
                switchProblem = error
                return false
            } catch {
                switchProblem = CoreError("The switch did not finish.")
                return false
            }
        }
        return false
    }

    /// A failed switch given up: the app is as it was before it, the current
    /// profile open if it was, else the start picker asking again.
    func cancelSwitch() {
        // A request still with the core is never undone here: its answer decides.
        guard !inFlight else { return }
        switching = nil
        switchProblem = nil
        chosen = chosenBefore
    }

    /// Profile ops that answer the new snapshot (create, update, delete, setPin).
    func perform(_ op: String, _ fields: [String: any Sendable]) async throws {
        try await core.perform(op, fields)
    }
}
