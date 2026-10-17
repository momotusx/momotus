import Foundation
#if !os(tvOS)
import LocalAuthentication
#endif
import Observation

/// The sync account on this device: one per app. The keystore record lives
/// in the Keychain; every sync op gets it and may hand back a new one, which
/// is stored before anything else happens.
@MainActor @Observable
final class SyncModel {
    /// A record is stored: this device is in a sync account.
    private(set) var joined = SyncRecord.load() != nil
    private(set) var syncing = false
    private(set) var lastReport: SyncReport?
    private(set) var lastSynced: Date?
    private(set) var devices: SyncDevices?
    /// What went wrong, in words, and what the user can do about it.
    private(set) var problem: SyncProblem?

    @ObservationIgnored private let core: CoreClient
    @ObservationIgnored private var scheduled: Task<Void, Never>?

    init(core: CoreClient) {
        self.core = core
    }

    /// Syncs, storing each new record and going again while the core asks to
    /// (a round that changed the record publishes nothing until it is stored).
    func syncNow() async {
        guard !syncing, var record = SyncRecord.load() else { return }
        syncing = true
        defer { syncing = false }
        for _ in 0..<5 {
            do {
                let report: SyncReport = try await call("syncNow", record)
                if let next = report.record {
                    SyncRecord.store(next)
                    record = next
                }
                lastReport = report
                lastSynced = .now
                problem = nil
                if !report.again { break }
            } catch {
                handle(error)
                break
            }
        }
    }

    /// Syncs a few seconds after an edit, so a burst of edits goes in one round.
    func schedule() {
        guard joined else { return }
        scheduled?.cancel()
        scheduled = Task {
            try? await Task.sleep(for: .seconds(5))
            guard !Task.isCancelled else { return }
            await syncNow()
        }
    }

    /// After pairing: the new record is already stored.
    func didJoin() async {
        joined = true
        await syncNow()
        await loadDevices()
    }

    func loadDevices() async {
        guard let record = SyncRecord.load() else { return }
        do {
            let list: SyncDevices = try await call("syncDevices", record)
            if let next = list.record { SyncRecord.store(next) }
            devices = list
        } catch {
            handle(error)
        }
    }

    /// A recovery kit replaces a lost device; it is shown once and not kept.
    /// Behind the OS authentication prompt, as the protocol asks.
    func createRecoveryKit() async -> String? {
        guard let record = SyncRecord.load() else { return nil }
        guard await authenticate("create a recovery kit") else { return nil }
        struct Kit: Decodable { let kit: String; let record: JSONValue }
        do {
            let made: Kit = try await call("createRecoveryKit", record)
            SyncRecord.store(made.record)
            await loadDevices()
            return made.kit
        } catch {
            handle(error)
            return nil
        }
    }

    /// Leaves the account. Without `force` the core refuses while edits are unsent.
    func signOut(force: Bool) async {
        guard let record = SyncRecord.load() else { return }
        do {
            try await core.perform("signOut", ["record": record, "force": force])
            forget()
        } catch {
            handle(error)
        }
    }

    /// The relay's device list wins over this device's (after a `fork`).
    func useServerRoster() async {
        guard let record = SyncRecord.load() else { return }
        do {
            let next: JSONValue = try await core.request("useServerRoster", ["record": record, "drop": [String]()]) {
                try JSONDecoder().decode(Envelope<JSONValue>.self, from: $0).result
            }
            SyncRecord.store(next)
            problem = nil
            await syncNow()
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
        SyncRecord.delete()
        joined = false
        devices = nil
        lastReport = nil
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
