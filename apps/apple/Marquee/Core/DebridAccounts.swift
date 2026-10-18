import Foundation

// The device's debrid accounts as records (plans/c10-handoff.md, phase 3a,
// revised), as Android's DebridAccounts and SecretDebridKeystore have them.

/// Small named secrets: the Keychain in the app, a dictionary in tests.
protocol SecretItems: Sendable {
    /// Nil when there is no such item; throws when the store could not be
    /// read, which never counts as absent.
    func read(_ account: String) throws -> Data?
    /// False when the store refused it.
    func write(_ account: String, _ data: Data) -> Bool
    /// False when the store refused it; an absent item is removed already.
    func delete(_ account: String) -> Bool
    /// The names of the stored items that start with `prefix`; throws when
    /// the store could not list them.
    func accounts(prefix: String) throws -> [String]
}

enum DebridKeystoreError: Error {
    case refused
}

/// The debrid keystore over secret items: one item per record (a service
/// holds one) and one per tombstone. A service's record or tombstone and its
/// legacy key change as one recoverable unit: once a record or a tombstone
/// for a service is stored, its legacy key is ignored and deleted (after the
/// write, and again at every start), and a tombstone goes only after the
/// legacy key did. So a half-finished move from legacy keys never overwrites
/// a later sign-in or brings back a disconnect, a crash between the two
/// writes included, and a disconnect is refused only when its tombstone
/// itself was not stored.
struct ItemDebridKeystore: DebridKeystore {
    let items: any SecretItems

    private static func record(_ service: String) -> String { "debrid.record.\(service)" }
    private static func tombstone(_ t: DebridTombstone) -> String { "debrid.tombstone.\(t.service).\(t.generation)" }
    /// The key a service had before records (the old Debrid screen stored it here).
    static func legacy(_ service: String) -> String { "debrid.\(service)" }

    // In the services' order, the order the legacy keys kept: `restoreDebrid`
    // makes the first the default, so a refresh never changes which one it is.
    // A read or a listing that fails throws: a record that could not be read
    // is never taken for one that is gone, so its tombstone stays.
    func records() async throws -> [DebridRecord] {
        let found = try items.accounts(prefix: "debrid.record.").compactMap { try items.read($0) }
            .compactMap { try? JSONDecoder().decode(DebridRecord.self, from: $0) }
        return Self.inServiceOrder(found)
    }

    func tombstones() async throws -> [DebridTombstone] {
        try items.accounts(prefix: "debrid.tombstone.").compactMap { try items.read($0) }
            .compactMap { try? JSONDecoder().decode(DebridTombstone.self, from: $0) }
    }

    /// `(service, key)` pairs from before records, still to move, in the
    /// services' order. A service with a record or a tombstone has moved (or
    /// was disconnected): its key is skipped and deleted. When the records or
    /// tombstones cannot be read, nothing moves: one of them may be newer.
    func legacy() async -> [(service: String, key: String)] {
        guard let stored = try? await records(), let buried = try? await tombstones() else { return [] }
        let taken = Set(stored.map(\.service) + buried.map(\.service))
        var left: [(service: String, key: String)] = []
        for service in DebridWords.services {
            guard let data = try? items.read(Self.legacy(service.id)) else { continue }
            if taken.contains(service.id) {
                _ = items.delete(Self.legacy(service.id))
            } else {
                left.append((service.id, String(decoding: data, as: UTF8.self)))
            }
        }
        return left
    }

    func writeRecord(_ record: DebridRecord) async throws {
        guard items.write(Self.record(record.service), try JSONEncoder().encode(record)) else { throw DebridKeystoreError.refused }
        // The record is stored: from now on its legacy key is ignored, and
        // deleted here or at the next start.
        _ = items.delete(Self.legacy(record.service))
    }

    func deleteRecord(service: String, generation: String) async throws {
        let name = Self.record(service)
        guard let data = try items.read(name), (try? JSONDecoder().decode(DebridRecord.self, from: data))?.generation == generation else { return }
        guard items.delete(name) else { throw DebridKeystoreError.refused }
    }

    func writeTombstone(_ tombstone: DebridTombstone) async throws {
        guard items.write(Self.tombstone(tombstone), try JSONEncoder().encode(tombstone)) else { throw DebridKeystoreError.refused }
        // Stored: the disconnect goes on even when the legacy key stays for now.
        _ = items.delete(Self.legacy(tombstone.service))
    }

    func deleteTombstone(_ tombstone: DebridTombstone) async throws {
        // While the legacy key stays, so does the tombstone that makes it ignored.
        guard items.delete(Self.legacy(tombstone.service)) else { throw DebridKeystoreError.refused }
        guard items.delete(Self.tombstone(tombstone)) else { throw DebridKeystoreError.refused }
    }

    /// Known services first, in the services' order; any other after them, as stored.
    static func inServiceOrder(_ records: [DebridRecord]) -> [DebridRecord] {
        let rank = Dictionary(uniqueKeysWithValues: DebridWords.services.enumerated().map { ($1.id, $0) })
        return records.enumerated()
            .sorted { (rank[$0.element.service] ?? Int.max, $0.offset) < (rank[$1.element.service] ?? Int.max, $1.offset) }
            .map(\.element)
    }
}

/// The device's debrid accounts for the app's life: one `DebridStore` across
/// every core the app opens. Each core open runs `opened` before the app may
/// show anything: the helper's `start()` (interrupted disconnects finished,
/// `restoreDebrid`), then what the `debrid` view holds, then the legacy keys
/// one at a time. Pushes reach the helper only once that core's `start()`
/// returned.
@MainActor
final class DebridAccounts {
    let store: DebridStore
    private let keystore: ItemDebridKeystore
    /// `setDebridKey`: one connection by key; its record arrives through the view.
    private let connect: (String, String) async throws -> Void
    /// Error kinds only: never a key, a token or a code.
    var log: (String) -> Void = { _ in }
    private var ready = false

    /// `report`: whether each service's pending record reached the Keychain, as
    /// soon as a save fails or succeeds (the cards say "Kept for this session").
    init(
        keystore: ItemDebridKeystore, core: any DebridCore,
        report: @escaping @Sendable (_ service: String, _ stored: Bool) async -> Void = { _, _ in },
        retryAfter: Duration = .seconds(10),
        connect: @escaping (String, String) async throws -> Void
    ) {
        self.keystore = keystore
        self.connect = connect
        store = DebridStore(keystore: keystore, core: core, report: report, retryAfter: retryAfter)
    }

    /// The core is going away: its views' pushes wait for the next core's restore.
    func pause() {
        ready = false
    }

    /// A core opened. `current` reads the pending records the `debrid` view
    /// holds now. A restore that fails twice is logged and the app goes on:
    /// records stay in the keystore for the next start.
    func opened(current: () -> [DebridRecord]?) async {
        ready = false
        // What the last core left queued is stored first: the restore then
        // reads the newest record of each service, never one a queued write
        // was about to replace.
        await store.idle()
        for attempt in 1...2 {
            do {
                try await store.start()
                break
            } catch {
                log("debrid restore failed (attempt \(attempt)): \(type(of: error))")
            }
        }
        ready = true
        if let pending = current() { store.pending(pending) }
        // Records already on their way land first: a service they store moves no old key.
        await store.idle()
        let records = (try? await keystore.records().count) ?? 0
        let tombstones = (try? await keystore.tombstones().count) ?? 0
        let legacy = await keystore.legacy().count
        log("debrid: \(records) records restored, \(tombstones) tombstones left, \(legacy) legacy keys to move")
        await moveLegacy()
    }

    /// Every push of the `debrid` view.
    func pushed(_ pending: [DebridRecord]) {
        if ready { store.pending(pending) }
    }

    /// Keys from before records, one at a time: each becomes a connection
    /// whose record, once stored, drops the key. A failure stops here; the
    /// rest moves at the next start.
    private func moveLegacy() async {
        for (service, key) in await keystore.legacy() {
            do {
                try await connect(service, key)
            } catch {
                log("a legacy \(service) key did not move yet: \(type(of: error))")
                return
            }
        }
    }
}
