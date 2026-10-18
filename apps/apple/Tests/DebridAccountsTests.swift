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

    func read(_ account: String) -> Data? { lock.withLock { values[account] } }
    func write(_ account: String, _ data: Data) -> Bool {
        if slowWrites > 0 { Thread.sleep(forTimeInterval: slowWrites) }
        lock.withLock { values[account] = data }
        return true
    }
    func delete(_ account: String) -> Bool { lock.withLock { values[account] = nil }; return true }
    func accounts(prefix: String) -> [String] { lock.withLock { values.keys.filter { $0.hasPrefix(prefix) }.sorted() } }
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

    nonisolated func restore(records: [DebridRecord], tombstones: [DebridTombstone]) async throws -> RestoreAnswer {
        await note(records)
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
    #expect(keystore.legacy().map(\.service) == ["torbox"])
    // The next start (a new core) moves what is left.
    core.refuse = []
    await first.opened { nil }
    await first.store.idle()
    #expect(try await keystore.records().map(\.service) == ["realdebrid", "torbox"])
    #expect(keystore.legacy().isEmpty)
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
