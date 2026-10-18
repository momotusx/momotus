import Foundation
import Observation

/// The debrid accounts for the app's life: the Keychain records behind the
/// helper, an app-lived `debrid` view, and the restore each core open runs
/// before the app may show anything. `setDebridKeys` is never sent.
@MainActor @Observable
final class DebridSession {
    /// The `debrid` view; it opens on each core only after that core's restore.
    let view: LiveRecord<DebridSnapshot>
    @ObservationIgnored let accounts: DebridAccounts
    /// The core generation whose restore finished: until it matches, nothing of the app shows.
    private(set) var restored = -1
    @ObservationIgnored private let core: CoreClient
    /// Services whose last record write the Keychain refused.
    let health: DebridStoreHealth

    init(core: CoreClient) {
        self.core = core
        let health = DebridStoreHealth()
        self.health = health
        var keystore = ItemDebridKeystore(items: KeychainItems())
        keystore.report = { [health] service, stored in await health.mark(service, stored: stored) }
        accounts = DebridAccounts(
            keystore: keystore,
            core: CoreDebridCalls(core: core),
            connect: { service, key in try await core.perform("setDebridKey", ["service": service, "key": key]) })
        view = LiveRecord("debrid", core: core)
        accounts.log = { coreLog.notice("\($0, privacy: .public)") }
        view.onSnapshot = { [weak self] snapshot in self?.accounts.pushed(snapshot.pending) }
    }

    /// Runs on every core open, a death included, before the app may show anything.
    func opened() async {
        let generation = core.generation
        await accounts.opened {
            // Opened on this core only now, after its restore: its first
            // snapshot's pending records reach the helper as a push.
            view.start()
            return nil
        }
        restored = generation
    }

    /// Whether the open core's accounts are back.
    var ready: Bool { restored == core.generation }

    /// "N debrid services in use", once the view says.
    var inUse: String? { view.snapshot.map { DebridWords.inUse($0.accounts.count) } }

    /// Connects a service with a typed key; its record arrives through the view.
    func setKey(_ service: String, _ key: String) async throws {
        try await core.perform("setDebridKey", ["service": service, "key": key])
    }

    /// Tombstone, core, record, tombstone; fails when the tombstone could not be written.
    func disconnect(_ account: DebridAccount) async throws {
        try await accounts.store.disconnect(service: account.service, generation: account.generation).value
    }
}

/// Whether each service's last record write reached the Keychain, as the cards show it.
@MainActor @Observable
final class DebridStoreHealth {
    private(set) var unstored: Set<String> = []

    func mark(_ service: String, stored: Bool) {
        if stored { unstored.remove(service) } else { unstored.insert(service) }
    }
}

/// The helper's core calls over the app's core client.
struct CoreDebridCalls: DebridCore {
    let core: CoreClient

    func restore(records: [DebridRecord], tombstones: [DebridTombstone]) async throws -> RestoreAnswer {
        let fields: [String: any Sendable] = ["records": try Self.json(records), "tombstones": try Self.json(tombstones)]
        return try await core.call("restoreDebrid", fields, as: RestoreAnswer.self)
    }

    func ack(service: String, generation: String, revision: UInt64) async throws {
        try await core.perform("ackDebrid", ["service": service, "generation": generation, "revision": revision])
    }

    func disconnect(service: String, generation: String) async throws {
        try await core.perform("disconnectDebrid", ["service": service, "generation": generation])
    }

    /// Records go to the core exactly as stored (the request encoder takes JSON values as is).
    private static func json<T: Encodable>(_ value: T) throws -> JSONValue {
        try JSONDecoder().decode(JSONValue.self, from: JSONEncoder().encode(value))
    }
}
