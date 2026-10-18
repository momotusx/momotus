import Foundation
import Testing

// The debrid accounts over a keystore of named items (the Keychain's
// stand-in: unit tests run without the app's entitlements), as Android's
// DebridAccountsTest has them.

/// Named items in memory.
private final class MemoryItems: SecretItems, @unchecked Sendable {
    private let lock = NSLock()
    private var values: [String: Data] = [:]
    /// Each write takes this long: a write still on its way when a restore reads.
    var slowWrites: TimeInterval = 0

    init(legacy: [(String, String)] = []) {
        for (service, key) in legacy { values[ItemDebridKeystore.legacy(service)] = Data(key.utf8) }
    }

    /// Reads of these names that fail before the store answers again (a locked or busy Keychain).
    var failReads: [String: Int] = [:]
    /// Listings that fail before the store answers again.
    var failListings = 0
    func read(_ account: String) throws -> Data? {
        try lock.withLock {
            if let left = failReads[account], left > 0 {
                failReads[account] = left - 1
                throw DebridKeystoreError.refused
            }
            return values[account]
        }
    }
    func write(_ account: String, _ data: Data) -> Bool {
        if slowWrites > 0 { Thread.sleep(forTimeInterval: slowWrites) }
        lock.withLock { values[account] = data }
        return true
    }
    /// Deletes of these names are refused (a Keychain that says no).
    var refuseDeletes: Set<String> = []
    func delete(_ account: String) -> Bool {
        if refuseDeletes.contains(account) { return false }
        lock.withLock { values[account] = nil }
        return true
    }
    func accounts(prefix: String) throws -> [String] {
        try lock.withLock {
            if failListings > 0 {
                failListings -= 1
                throw DebridKeystoreError.refused
            }
            return values.keys.filter { $0.hasPrefix(prefix) }.sorted()
        }
    }
}

/// The core as the helper and the move see it: `setDebridKey` makes a new
/// generation whose record arrives as a push.
@MainActor
private final class FakeCore: DebridCore, @unchecked Sendable {
    var refuse: Set<String> = []
    var connected: [String] = []
    var push: ([DebridRecord]) -> Void = { _ in }
    /// What each restore was handed.
    var restores: [[DebridRecord]] = []
    private var next = 0

    /// Notes what it restores: as the core, never a generation with a tombstone.
    nonisolated func restore(records: [DebridRecord], tombstones: [DebridTombstone]) async throws -> RestoreAnswer {
        await note(records.filter { record in !tombstones.contains { $0.generation == record.generation } })
        return RestoreAnswer(refused: [], tombstones: [])
    }

    private func note(_ records: [DebridRecord]) { restores.append(records) }

    nonisolated func ack(service: String, generation: String, revision: UInt64) async throws {}
    nonisolated func disconnect(service: String, generation: String) async throws {}

    func connect(_ service: String, _ key: String) async throws {
        if refuse.contains(service) { throw DebridKeystoreError.refused }
        connected.append(service)
        next += 1
        push([record(service, "gen\(next)")])
    }
}

private func record(_ service: String, _ generation: String) -> DebridRecord {
    DebridRecord(service: service, generation: generation, revision: 1, credential: .key("k-\(service)"))
}

@MainActor
private func accounts(_ items: MemoryItems, _ core: FakeCore) -> (DebridAccounts, ItemDebridKeystore) {
    let keystore = ItemDebridKeystore(items: items)
    let accounts = DebridAccounts(keystore: keystore, core: core, connect: { try await core.connect($0, $1) })
    core.push = { accounts.pushed($0) }
    return (accounts, keystore)
}

@MainActor @Test func aHalfFinishedMoveGoesOnAtTheNextStart() async throws {
    let items = MemoryItems(legacy: [("realdebrid", "k1"), ("torbox", "k2")])
    let core = FakeCore()
    core.refuse = ["torbox"]
    let (first, keystore) = accounts(items, core)
    await first.opened { nil }
    await first.store.idle()
    #expect(try await keystore.records().map(\.service) == ["realdebrid"])
    #expect(await keystore.legacy().map(\.service) == ["torbox"])
    // The next start (a new core) moves what is left.
    core.refuse = []
    await first.opened { nil }
    await first.store.idle()
    #expect(try await keystore.records().map(\.service) == ["realdebrid", "torbox"])
    #expect(await keystore.legacy().isEmpty)
    #expect(core.connected == ["realdebrid", "torbox"])
}

@MainActor @Test func aSignInOrDisconnectBetweenStartsIsNeverUndone() async throws {
    let items = MemoryItems(legacy: [("realdebrid", "k1"), ("torbox", "k2"), ("alldebrid", "k3")])
    let core = FakeCore()
    core.refuse = ["torbox"]
    let (accounts, keystore) = accounts(items, core)
    await accounts.opened { nil }
    await accounts.store.idle()
    // Between the two starts: TorBox signs in with a code, AllDebrid is disconnected.
    accounts.pushed([record("torbox", "code")])
    try await accounts.store.disconnect(service: "alldebrid", generation: "old").value
    await accounts.store.idle()
    core.refuse = []
    core.connected = []
    await accounts.opened { nil }
    await accounts.store.idle()
    // Neither key comes back: the sign-in stays, AllDebrid stays gone.
    #expect(core.connected.isEmpty)
    #expect(try await keystore.records().map(\.generation) == ["gen1", "code"])
    #expect(try await keystore.tombstones().isEmpty)
}

@Test func recordsComeBackInTheServicesOrder() async throws {
    let keystore = ItemDebridKeystore(items: MemoryItems())
    for service in ["premiumize", "torbox", "alldebrid", "realdebrid"] {
        try await keystore.writeRecord(record(service, service))
    }
    // A refresh rewrites one; the order stays the services'.
    try await keystore.writeRecord(DebridRecord(service: "torbox", generation: "torbox", revision: 2, credential: .key("k")))
    #expect(try await keystore.records().map(\.service) == ["realdebrid", "torbox", "alldebrid", "premiumize"])
}

@MainActor @Test func pushesWaitForThatCoresRestore() async throws {
    let items = MemoryItems()
    let core = FakeCore()
    let (accounts, keystore) = accounts(items, core)
    accounts.pushed([record("torbox", "early")])
    await accounts.store.idle()
    #expect(try await keystore.records().isEmpty)
    await accounts.opened { [record("torbox", "early")] }
    await accounts.store.idle()
    #expect(try await keystore.records().map(\.generation) == ["early"])
    // A core going away: its pushes wait for the next core's restore.
    accounts.pause()
    accounts.pushed([record("premiumize", "late")])
    await accounts.store.idle()
    #expect(try await keystore.records().map(\.service) == ["torbox"])
}

@MainActor @Test func aRestartWaitsForTheWriteTheLastCoreLeftQueued() async throws {
    let items = MemoryItems()
    let core = FakeCore()
    let (accounts, keystore) = accounts(items, core)
    await accounts.opened { nil }
    try await keystore.writeRecord(record("torbox", "G1"))
    // G2 replaces G1, its write still on its way when the next core opens.
    items.slowWrites = 0.3
    accounts.pushed([record("torbox", "G2")])
    await accounts.opened { nil }
    #expect(core.restores.last?.map(\.generation) == ["G2"])
}

@MainActor @Test func aStoredTombstoneDisconnectsEvenWhenTheLegacyKeyStays() async throws {
    // AllDebrid half-moved: its record is stored, its old key still there, and
    // the Keychain refuses to delete that key for now.
    let items = MemoryItems(legacy: [("alldebrid", "k3")])
    let core = FakeCore()
    let (accounts, keystore) = accounts(items, core)
    items.refuseDeletes = [ItemDebridKeystore.legacy("alldebrid")]
    try await keystore.writeRecord(record("alldebrid", "A"))
    // The tombstone is stored, so the disconnect is not refused.
    try await accounts.store.disconnect(service: "alldebrid", generation: "A").value
    await accounts.store.idle()
    #expect(try await keystore.records().isEmpty)
    // The tombstone stays while the old key does: that key is never moved back.
    #expect(try await keystore.tombstones().map(\.generation) == ["A"])
    #expect(await keystore.legacy().isEmpty)
    // The next start, with the Keychain willing again, clears both.
    items.refuseDeletes = []
    await accounts.opened { nil }
    await accounts.store.idle()
    #expect(core.connected.isEmpty)
    #expect(try await keystore.tombstones().isEmpty)
    #expect(try items.read(ItemDebridKeystore.legacy("alldebrid")) == nil)
}

@MainActor @Test func aCrashBetweenTheRecordAndTheLegacyDeleteNeverBringsTheOldKeyBack() async throws {
    // TorBox signed in with a code; the app stopped before its old key was deleted.
    let items = MemoryItems(legacy: [("torbox", "old")])
    let core = FakeCore()
    let (accounts, keystore) = accounts(items, core)
    items.refuseDeletes = [ItemDebridKeystore.legacy("torbox")]
    try await keystore.writeRecord(record("torbox", "code"))
    items.refuseDeletes = []
    #expect(try items.read(ItemDebridKeystore.legacy("torbox")) != nil)
    await accounts.opened { nil }
    await accounts.store.idle()
    #expect(core.connected.isEmpty)
    #expect(try await keystore.records().map(\.generation) == ["code"])
    #expect(try items.read(ItemDebridKeystore.legacy("torbox")) == nil)
}

@MainActor @Test func aRecordThatCannotBeReadAtStartIsNeverRestoredPastItsTombstone() async throws {
    // TorBox G was disconnected: its tombstone is stored, its record not deleted yet.
    let items = MemoryItems()
    let core = FakeCore()
    let (accounts, keystore) = accounts(items, core)
    try await keystore.writeRecord(record("torbox", "G"))
    try await keystore.writeTombstone(DebridTombstone(service: "torbox", generation: "G"))
    // The next start: the first two reads of G's record fail, the third answers.
    items.failReads = ["debrid.record.torbox": 2]
    await accounts.opened { nil }
    await accounts.store.idle()
    #expect(!core.restores.joined().contains { $0.generation == "G" })
    // G is gone for good: deleted once it could be read, then its tombstone.
    #expect(try await keystore.records().isEmpty)
    #expect(try await keystore.tombstones().isEmpty)
}

@MainActor @Test func aKeychainThatCannotListMovesNoLegacyKey() async throws {
    // TorBox has a record from a code sign-in and still an old key.
    let items = MemoryItems(legacy: [("torbox", "old")])
    let keystore = ItemDebridKeystore(items: items)
    items.refuseDeletes = [ItemDebridKeystore.legacy("torbox")]
    try await keystore.writeRecord(record("torbox", "code"))
    items.refuseDeletes = []
    // The records cannot be listed: the old key is not taken for one to move.
    items.failListings = 1
    #expect(await keystore.legacy().isEmpty)
    // Listed again, the record keeps the key from moving and the key goes.
    #expect(await keystore.legacy().isEmpty)
    #expect(try items.read(ItemDebridKeystore.legacy("torbox")) == nil)
}
