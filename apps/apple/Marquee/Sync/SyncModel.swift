import Foundation
#if !os(tvOS)
import LocalAuthentication
#endif
import Observation

/// The sync account on this device: one per app. The keystore record lives
/// in the Keychain; every sync op gets it and may hand back a new one, which
/// is stored before anything else happens.
///
/// Rounds run in one worker owned by this model, never by a view, a timer or
/// the scene phase: those only ask for a round, so nothing can cancel a round
/// between the core's answer and storing its record. Asking while a round
/// runs keeps one more for after it.
@MainActor @Observable
final class SyncModel {
    /// A record is stored, or held for this session: this device is in a sync account.
    private(set) var joined = SyncRecord.load() != nil
    /// A round runs or is asked for.
    private(set) var syncing = false
    private(set) var lastReport: SyncReport?
    private(set) var lastSynced: Date?
    private(set) var devices: SyncDevices?
    /// What went wrong, in words, and what the user can do about it.
    private(set) var problem: SyncProblem?
    /// The pairing attempt. It belongs here, not to the page, so its join is
    /// stored the moment it arrives, whether or not the page is on screen.
    private(set) var pairing: LiveRecord<PairingSnapshot>?
    /// The last join: in the Keychain (true), for this session only (false), or none yet.
    private(set) var joinStored: Bool?

    @ObservationIgnored private let core: CoreClient
    @ObservationIgnored private var scheduled: Task<Void, Never>?
    @ObservationIgnored private var worker: Task<Void, Never>?
    @ObservationIgnored private var pending = false
    /// Bumped by every join. A round from before it stores and shows nothing.
    /// The main actor is the lock: the bump, the store and each round's check
    /// after its await never interleave.
    @ObservationIgnored private var account = 0
    /// A record the Keychain refused, held in memory: nothing is sent until it is stored.
    @ObservationIgnored private var unstored: JSONValue?

    init(core: CoreClient) {
        self.core = core
    }

    /// Asks for a round: now, or once the running one ends.
    func request() {
        pending = true
        guard worker == nil else { return }
        syncing = true
        worker = Task {
            while pending {
                pending = false
                await rounds()
            }
            syncing = false
            worker = nil
        }
    }

    /// Asks for a round a few seconds after an edit, so a burst of edits goes
    /// in one round. Cancelling cancels the wait only, never a round.
    func schedule() {
        guard joined else { return }
        scheduled?.cancel()
        scheduled = Task {
            try? await Task.sleep(for: .seconds(5))
            guard !Task.isCancelled else { return }
            request()
        }
    }

    /// Syncs, storing each new record and going again while the core asks to
    /// (a round that changed the record publishes nothing until it is stored).
    private func rounds() async {
        let mine = account
        guard var record = storedRecord() else { return }
        for _ in 0..<5 {
            do {
                let report: SyncReport = try await call("syncNow", record)
                // A join replaced the account while this round ran: its answer is not ours.
                guard account == mine else { return }
                lastReport = report
                lastSynced = .now
                if let next = report.record {
                    guard keep(next) else { return }
                    record = next
                }
                problem = nil
                if !report.again { break }
            } catch {
                guard account == mine else { return }
                handle(error)
                break
            }
        }
    }

    /// Starts pairing with a device that already syncs; a new attempt replaces the last.
    func startPairing(server: String, name: String) {
        let attempt = LiveRecord<PairingSnapshot>("pairing", params: ["server": server, "name": name], core: core)
        attempt.onSnapshot = { [weak self, weak attempt] snapshot in
            guard case .joined(let record) = snapshot, let self, let attempt, self.pairing === attempt, self.joinStored == nil else { return }
            self.join(record)
        }
        joinStored = nil
        pairing = attempt
        attempt.start()
    }

    /// Drops the attempt ("Try again", "New code").
    func endPairing() {
        pairing = nil
    }

    /// A join replaces any account before it, then its first round runs. When
    /// the Keychain refuses the record, the join lasts for this session and
    /// sync waits until the record is stored.
    private func join(_ record: JSONValue) {
        account += 1
        let stored = SyncRecord.store(record)
        unstored = stored ? nil : record
        joinStored = stored
        joined = true
        lastReport = nil
        lastSynced = nil
        devices = nil
        problem = stored ? nil : SyncProblem(code: "storage", message: "")
        request()
    }

    /// The account's record from the Keychain. One held in memory is stored
    /// first, and while the Keychain refuses it nothing is sent.
    private func storedRecord() -> JSONValue? {
        if let held = unstored {
            guard SyncRecord.store(held) else {
                problem = SyncProblem(code: "storage", message: "")
                return nil
            }
            unstored = nil
        }
        return SyncRecord.load()
    }

    /// Stores a record the core handed back. When the Keychain refuses it, it
    /// is held here and sync pauses until it is stored.
    private func keep(_ next: JSONValue) -> Bool {
        guard SyncRecord.store(next) else {
            unstored = next
            problem = SyncProblem(code: "storage", message: "")
            return false
        }
        unstored = nil
        return true
    }

    func loadDevices() async {
        let mine = account
        guard let record = storedRecord() else { return }
        do {
            let list: SyncDevices = try await call("syncDevices", record)
            guard account == mine else { return }
            if let next = list.record { guard keep(next) else { return } }
            devices = list
        } catch {
            guard account == mine else { return }
            handle(error)
        }
    }

    /// A recovery kit replaces a lost device; it is shown once and not kept.
    /// Behind the OS authentication prompt, as the protocol asks.
    func createRecoveryKit() async -> String? {
        guard let record = storedRecord() else { return nil }
        guard await authenticate("create a recovery kit") else { return nil }
        let mine = account
        struct Kit: Decodable { let kit: String; let record: JSONValue }
        do {
            let made: Kit = try await call("createRecoveryKit", record)
            guard account == mine else { return nil }
            // The kit exists on the server now: it is shown even if its record waits to be stored.
            if keep(made.record) { await loadDevices() }
            return made.kit
        } catch {
            handle(error)
            return nil
        }
    }

    /// Leaves the account. Without `force` the core refuses while edits are unsent.
    func signOut(force: Bool) async {
        guard let record = storedRecord() else { return }
        do {
            try await core.perform("signOut", ["record": record, "force": force])
            forget()
        } catch {
            handle(error)
        }
    }

    /// The relay's device list wins over this device's (after a `fork`).
    func useServerRoster() async {
        let mine = account
        guard let record = storedRecord() else { return }
        do {
            let next: JSONValue = try await core.request("useServerRoster", ["record": record, "drop": [String]()]) {
                try JSONDecoder().decode(Envelope<JSONValue>.self, from: $0).result
            }
            guard account == mine, keep(next) else { return }
            problem = nil
            request()
        } catch {
            handle(error)
        }
    }

    private func call<R: Decodable & Sendable>(_ op: String, _ record: JSONValue) async throws -> R {
        try await core.request(op, ["record": record]) { try JSONDecoder().decode(Envelope<R>.self, from: $0).result }
    }

    /// Drops the account record here only, when the relay cannot take this
    /// device back (wrong relay, entries lost). The library stays.
    func forgetHere() {
        forget()
        problem = nil
    }

    private func forget() {
        account += 1
        SyncRecord.delete()
        unstored = nil
        joined = false
        joinStored = nil
        pairing = nil
        devices = nil
        lastReport = nil
        lastSynced = nil
    }

    private func handle(_ error: Error) {
        let core = error as? CoreError
        if core?.code == "dropped" { forget() }
        problem = SyncProblem(code: core?.code, message: error.localizedDescription)
    }

    private func authenticate(_ reason: String) async -> Bool {
        #if os(tvOS)
        // Apple TV cannot check who is in front of it, so it never shows a kit.
        return false
        #else
        let context = LAContext()
        var error: NSError?
        guard context.canEvaluatePolicy(.deviceOwnerAuthentication, error: &error) else {
            problem = SyncProblem(code: nil, message: "Set a device passcode first: creating a recovery kit asks for it.")
            return false
        }
        do {
            return try await context.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason)
        } catch {
            return false
        }
        #endif
    }
}

private struct Envelope<R: Decodable>: Decodable { let result: R }
