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
    private(set) var chosen = false {
        didSet { if chosen { joinedWho = nil } }
    }
    /// A switch under way: the profile asked for. The gate is shut until it ends.
    private(set) var switching: String?
    /// Why the last switch failed; the picker that takes over shows it.
    private(set) var switchProblem: CoreError?
    /// Bumped by every switch and by the device's wipe: the profile's screens
    /// are built again from nothing.
    private(set) var epoch = 0
    /// A switch request is with the core: no second switch starts, and Cancel
    /// waits for its answer (the core may still commit it).
    private(set) var inFlight = false
    /// A profile removed on another device, and how many times one was: the
    /// picker closes its keypad and says so.
    private(set) var gone: (id: String, count: Int)?
    /// Incognito runs (plans/batch-10-incognito.md): from the snapshot, and from the moment its entry answered.
    private(set) var incognito = false {
        didSet { Incognito.set(incognito) }
    }
    /// Incognito was just entered: its intro shows over the app until Continue (5.1).
    private(set) var intro = false
    /// The core warned that Incognito ends in a minute without use (the Mac's notice, 2.7).
    private(set) var endsSoon = false
    /// The last Incognito entry came from a chooser tile (Leave goes back there), not the bar's menu.
    private(set) var incognitoFromChooser = true

    @ObservationIgnored private let core: CoreClient
    @ObservationIgnored private let sync: SyncModel
    /// Its view is device-wide state the switch closes, and a removed profile's record goes.
    @ObservationIgnored private let trakt: TraktModel?
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

    init(core: CoreClient, sync: SyncModel, trakt: TraktModel? = nil) {
        self.core = core
        self.sync = sync
        self.trakt = trakt
        view = LiveRecord("profiles", core: core)
        view.onSnapshot = { [weak self] snapshot in self?.saw(snapshot) }
        core.onProfileGone = { [weak self] id in self?.removed(id) }
        core.onIncognito = { [weak self] event in self?.heard(event) }
        view.start()
        // A profiles view that failed to open (at launch or after a restart)
        // is opened again every 2 seconds; the root says so meanwhile.
        retrying = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(2))
                guard let self else { return }
                if self.view.failure != nil { self.view.reopen() }
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
        incognito = snapshot.incognito
        if !snapshot.incognito {
            intro = false
            endsSoon = false
        }
        // A lock starts the profile on Home again, as a switch does.
        if snapshot.locked { resumeOnboarding = false }
        guard !decided else { return }
        decided = true
        if !ProfileRules.startsOnPicker(snapshot) { chosen = true }
    }

    private func removed(_ id: String) {
        // A removed profile's keystore entries go too (also for one removed before this session).
        trakt?.profileGone(id)
        gone = (id, (gone?.count ?? 0) + 1)
        // The owner gone is the device's wipe (D13's removal; no other op removes
        // the owner): the session goes on as the new owner with its views closed
        // by the core, so the screens are built again and first run shows at once.
        if id == (snapshot?.profiles.first(where: \.isOwner)?.id ?? "default") {
            resumeOnboarding = true
            epoch += 1
        }
    }

    /// `{"incognito": "endsSoon"}` shows the Mac's warning; `ended` needs nothing here: the snapshot that follows
    /// locks the session, so the chooser shows with the ended line at once.
    private func heard(_ event: String) {
        endsSoon = event == "endsSoon" && incognito
    }

    /// The root: the profile's screens appeared, or left.
    func shell(onScreen: Bool) {
        shellOnScreen = onScreen
    }

    /// The current profile, unlocked: the start picker's answer.
    func markChosen() {
        chosen = true
    }

    /// A first-run join into an account with two or more profiles or a PIN (plans/first-screens.md 2, D2): the start
    /// picker shows over the still poster wall, with the line that says how this device signed in.
    private(set) var joinedWho: String?

    func askAfterJoin(line: String) {
        joinedWho = line
        // First run ended with the join: the profile picked next opens Home, never first run again.
        resumeOnboarding = false
        chosen = false
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
        var fields: [String: any Sendable] = ["profile": id]
        if let pin { fields["pin"] = pin }
        return await change(id, op: "switchProfile", fields)
    }

    /// Enters Incognito from the session's profile (plans/batch-10-incognito.md 2.7) as a switch does; `fromChooser`
    /// sends Leave back to the chooser. Its intro shows once it opens.
    @discardableResult
    func enterIncognito(pin: String?, fromChooser: Bool) async -> Bool {
        var fields: [String: any Sendable] = ["profile": Incognito.id, "fromChooser": fromChooser]
        incognitoFromChooser = fromChooser
        if let pin { fields["pin"] = pin }
        return await change(Incognito.id, op: "switchProfile", fields) {
            self.incognito = true
            self.intro = true
        }
    }

    /// Leaves Incognito for where it was entered, never with a PIN (5.3): the gate shuts as for a switch, so its
    /// screens close first.
    func leaveIncognito() {
        guard let host = snapshot?.incognitoHost else { return }
        Task {
            await change(host, op: "leaveIncognito", [:]) { self.incognito = false }
        }
    }

    /// The intro's Continue.
    func continueIntro() {
        intro = false
    }

    /// The person is here: the warning goes, and the core hears `active` (the notice's button, or any input).
    func stay() {
        endsSoon = false
        core.lifecycle("active")
    }

    /// The person read the ended notice over Home (5.4).
    func ackEnded() {
        Task { try? await core.perform("ackIncognitoEnded") }
    }

    /// The Mac's window closed: Incognito ends as closing the app ends it (5.3). No screen is left to reset, so the
    /// gate does not shut for it; views still closing are waited for.
    func leaveOnClose() {
        guard incognito else { return }
        intro = false
        Task {
            for attempt in 1...10 {
                do {
                    try await core.perform("leaveIncognito")
                    return
                } catch let error as CoreError where error.code == "viewsOpen" && attempt < 10 {
                    try? await Task.sleep(for: .milliseconds(100))
                } catch {
                    coreLog.error("leave on close: \((error as? CoreError)?.code ?? "error", privacy: .public)")
                    return
                }
            }
        }
    }

    /// A switch or a Leave: the gate shuts for `target` (the profile it opens), then `op` runs once the views closed;
    /// `done` runs on its success, before the gate opens.
    @discardableResult
    private func change(_ target: String, op: String, _ fields: [String: any Sendable], done: (@MainActor () -> Void)? = nil) async -> Bool {
        guard !inFlight else { return false }
        // Out from now until the last answer: a second ask, or Cancel, waits.
        inFlight = true
        if switching == nil { chosenBefore = chosen }
        resumeOnboarding = false
        switching = target
        switchProblem = nil
        epoch += 1
        // Device-level views a profile does not own still count as open.
        sync.endPairing()
        trakt?.end()
        // The session's own task: the caller's view leaves with the gate.
        let work = Task { await runSwitch(op, fields, done: done) }
        return await work.value
    }

    private func runSwitch(_ op: String, _ fields: [String: any Sendable], done: (@MainActor () -> Void)?) async -> Bool {
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
                try await core.perform(op, fields)
                done?()
                // A Find addons card's state belongs to the profile that pressed it.
                InstallCards.shared.forgetAll()
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
        // Deleted here: its Trakt record goes at once, not only when the core says it is gone.
        if op == "deleteProfile", let id = fields["profile"] as? String { trakt?.profileGone(id) }
    }
}
