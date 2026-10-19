import Foundation
#if !os(tvOS)
import LocalAuthentication
#endif
import Observation

/// The sync account on this device: one per app. The keystore record lives
/// in the Keychain; every sync op gets it and may hand back a new one, which
/// is stored before anything else happens.
///
/// Every op on the record runs in turn (one at a time, in the order asked),
/// in tasks owned by this model, never by a view: leaving a page or a scene
/// change cannot cancel an op between the core's answer and storing its
/// record. A record that comes back is kept only if it is not older than the
/// one stored (`revision`). Rounds run in one worker: asking while a round
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
    /// What went wrong in the last round, in words, and what the user can do about it.
    private(set) var problem: SyncProblem?
    /// The pairing attempt. It belongs here, not to the page, so its join is
    /// stored the moment it arrives, whether or not the page is on screen.
    private(set) var pairing: LiveRecord<PairingSnapshot>?
    /// The last join: in the Keychain (true), for this session only (false), or none yet.
    private(set) var joinStored: Bool?
    /// Joined with a recovery kit and no new kit made since: the page asks for one.
    private(set) var recovered = UserDefaults.standard.bool(forKey: "syncRecovered") {
        didSet { UserDefaults.standard.set(recovered, forKey: "syncRecovered") }
    }

    @ObservationIgnored private let core: CoreClient
    @ObservationIgnored private var scheduled: Task<Void, Never>?
    @ObservationIgnored private var worker: Task<Void, Never>?
    @ObservationIgnored private var pending = false
    /// The last op in line; the next one starts when it ends.
    @ObservationIgnored private var line: Task<Void, Never>?
    /// Bumped by every join and leave. An op from before it stores and shows nothing.
    /// The main actor is the lock: the bump, the store and each op's check
    /// after its await never interleave.
    @ObservationIgnored private var account = 0
    /// A record the Keychain refused, held in memory: nothing is sent until it is stored.
    @ObservationIgnored private var unstored: JSONValue?

    init(core: CoreClient) {
        self.core = core
    }

    // MARK: Rounds

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

    /// Syncs, going again while the core asks to (a round that changed the
    /// record publishes nothing until it is stored).
    private func rounds() async {
        for _ in 0..<5 {
            guard joined else { return }
            let mine = account
            do {
                let report: SyncReport = try await withRecord("syncNow") { $0.record }
                // A join or a leave replaced the account while this round ran: its answer is not ours.
                guard account == mine else { return }
                lastReport = report
                lastSynced = .now
                problem = nil
                // The 12 characters to compare come from the device list: keep it as fresh as the round.
                if report.fingerprint != devices?.fingerprint { try? await loadDevices() }
                if !report.again { return }
            } catch {
                guard account == mine else { return }
                problem = SyncProblem(error, action: .round)
                return
            }
        }
    }

    // MARK: Joining

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

    /// The first device of a new account.
    func createAccount(server: String, invite: String, name: String) async throws {
        var fields: [String: any Sendable] = ["server": server, "name": name]
        if !invite.isEmpty { fields["invite"] = invite }
        let record: JSONValue = try await core.call("createSyncAccount", fields, as: JSONValue.self)
        join(record)
    }

    /// A new device joining with a recovery kit; the page then asks for a new kit.
    func recover(server: String, kit: String, name: String) async throws {
        let record: JSONValue = try await core.call("recover", ["server": server, "kit": kit, "name": name], as: JSONValue.self)
        join(record, recovered: true)
    }

    /// A join replaces any account before it (the pages ask first), then its
    /// first round runs. When the Keychain refuses the record, the join lasts
    /// for this session and sync waits until the record is stored.
    private func join(_ record: JSONValue, recovered: Bool = false) {
        account += 1
        // A new account's record is stored as it is: revisions compare only within an account.
        let stored = SyncRecord.store(record)
        unstored = stored ? nil : record
        joinStored = stored
        joined = true
        self.recovered = recovered
        lastReport = nil
        lastSynced = nil
        devices = nil
        problem = stored ? nil : SyncProblem(code: "storage", message: "")
        request()
    }

    // MARK: The account's devices

    /// The device list, fresh from the sync server.
    @discardableResult
    func loadDevices() async throws -> SyncDevices {
        let mine = account
        let list: SyncDevices = try await withRecord("syncDevices") { $0.record }
        if account == mine { devices = list }
        return list
    }

    /// Vouches for the device that shows `qr` and hands it the account.
    /// Returns the six digits the new device must show too.
    func approvePairing(_ qr: String) async throws -> String {
        struct Approved: Decodable, Sendable { let code: String; let record: JSONValue }
        let approved: Approved = try await withRecord("approvePairing", ["qr": qr]) { $0.record }
        // The new device joins once it confirms; the list catches up on the next look.
        try? await loadDevices()
        return approved.code
    }

    /// A recovery kit, to show once: nothing here keeps it. Behind the OS
    /// authentication prompt, as the protocol asks; never on a TV.
    func createRecoveryKit() async throws -> String {
        #if os(tvOS)
        throw CoreError("A TV does not make recovery kits.", code: "unconfirmed")
        #else
        try await confirmOwner(SyncWords.authKit)
        struct Made: Decodable, Sendable { let kit: String; let record: JSONValue }
        let made: Made = try await withRecord("createRecoveryKit") { $0.record }
        recovered = false
        try? await loadDevices()
        return made.kit
        #endif
    }

    /// What reporting `lost` would drop: its devices (ticked) and its kits (always).
    func lostPreview(_ lost: String) async throws -> LostPreview {
        try await withRecord("lostPreview", ["lost": lost]) { (_: LostPreview) in nil }
    }

    /// Removes a device or a kit. It has landed once this returns; a list that
    /// cannot be fetched right after does not undo it.
    func removeDevice(_ device: String) async throws {
        try await confirmOwner(SyncWords.authRemove)
        let _: JSONValue = try await withRecord("removeDevice", ["device": device]) { $0 }
        try? await loadDevices()
    }

    /// Drops `lost` and the devices it added except `keep`; their kits go too.
    func reportLost(_ lost: String, keep: [String]) async throws {
        try await confirmOwner(SyncWords.authLost)
        let _: JSONValue = try await withRecord("reportLost", ["lost": lost, "keep": keep]) { $0 }
        try? await loadDevices()
    }

    /// Leaves the account. Without `force` the core refuses while edits are unsent.
    func signOut(force: Bool) async throws {
        let _: JSONValue = try await withRecord("signOut", ["force": force]) { _ in nil }
        forget()
        problem = nil
    }

    /// After `fork`: the sync server's device list wins over this device's.
    func useServerRoster() async throws {
        let _: JSONValue = try await withRecord("useServerRoster", ["drop": [String]()]) { $0 }
        problem = nil
        request()
    }

    /// Drops the account record here only, when the relay cannot take this
    /// device back (wrong relay, entries lost). The library stays.
    func forgetHere() {
        forget()
        problem = nil
    }

    // MARK: The record

    /// Runs one op on the record, in turn. A record in the answer is stored
    /// before the next op runs; `dropped` (this device was removed) forgets
    /// the account here.
    private func withRecord<R: Decodable & Sendable>(
        _ op: String, _ fields: [String: any Sendable] = [:], record recordIn: @escaping @Sendable (R) -> JSONValue?
    ) async throws -> R {
        try await inTurn { [self] in
            let mine = account
            guard let record = try storedRecord() else { throw CoreError("this device has not joined the account", code: "notJoined") }
            var all = fields
            all["record"] = record
            do {
                let result: R = try await core.call(op, all, as: R.self)
                if let next = recordIn(result), mine == account, !keep(next) {
                    throw CoreError("the record was not stored", code: "storage")
                }
                return result
            } catch let error as CoreError where mine == account && (error.code == "dropped" || error.code == "fork") {
                if error.code == "dropped" { forget() }
                problem = SyncProblem(error, action: .round)
                throw error
            }
        }
    }

    /// One op after another, in the order asked. The work runs in its own
    /// task, so a caller that goes away does not stop it half way.
    private func inTurn<T: Sendable>(_ work: @escaping @MainActor () async throws -> T) async throws -> T {
        let before = line
        let run = Task { @MainActor in
            await before?.value
            return try await work()
        }
        line = Task { _ = await run.result }
        return try await run.value
    }

    /// The account's record from the Keychain. One held in memory is stored
    /// first, and while the Keychain refuses it nothing is sent.
    private func storedRecord() throws -> JSONValue? {
        if let held = unstored {
            guard SyncRecord.store(held) else {
                problem = SyncProblem(code: "storage", message: "")
                throw CoreError("the record was not stored", code: "storage")
            }
            unstored = nil
        }
        return SyncRecord.load()
    }

    /// Stores a record the core handed back, unless the stored one is newer.
    /// When the Keychain refuses it, it is held here and sync pauses until it is stored.
    private func keep(_ next: JSONValue) -> Bool {
        let record = SyncRules.newer(SyncRecord.load(), next)
        guard SyncRecord.store(record) else {
            unstored = record
            problem = SyncProblem(code: "storage", message: "")
            return false
        }
        unstored = nil
        return true
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
        recovered = false
    }

    /// The OS prompt (passcode, Face ID or Touch ID) before changing the
    /// account's devices or making a kit, as the protocol asks. A TV has no
    /// way to check who is there: its pages ask a second time instead.
    private func confirmOwner(_ reason: String) async throws {
        #if !os(tvOS)
        let context = LAContext()
        var error: NSError?
        guard context.canEvaluatePolicy(.deviceOwnerAuthentication, error: &error) else {
            throw CoreError("no passcode", code: "noPasscode")
        }
        let ok = (try? await context.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason)) ?? false
        guard ok else { throw CoreError("not confirmed", code: "unconfirmed") }
        #endif
    }
}
