import Foundation
import XCTest

/// Replays apps/shared/debrid-storage-vectors.json, the cases every app's
/// helper passes, and checks that credentials never print.
@MainActor
final class DebridStoreTests: XCTestCase {
    private struct Failure: Error {}

    /// Disconnects that failed, as "service generation", in the order they failed.
    @MainActor private final class Errors {
        var list: [String] = []
    }

    @MainActor private final class FakeKeystore: DebridKeystore {
        /// One record per service, in the order services first appeared.
        var stored: [DebridRecord] = []
        var stones: [DebridTombstone] = []
        var log: [String] = []
        var failing: [String: Int] = [:]
        var holding: String?
        private var waiter: CheckedContinuation<Void, Never>?

        var waiting: Bool { waiter != nil }

        func release() {
            XCTAssertNotNil(waiter, "release with nothing held")
            let held = waiter
            waiter = nil
            held?.resume()
        }

        private func gate(_ call: String) async throws {
            if holding == call {
                holding = nil
                await withCheckedContinuation { waiter = $0 }
            }
            if let left = failing[call], left > 0 {
                failing[call] = left - 1
                log.append("\(call) failed")
                throw Failure()
            }
        }

        func records() async throws -> [DebridRecord] { stored }

        func tombstones() async throws -> [DebridTombstone] { stones }

        func writeRecord(_ record: DebridRecord) async throws {
            try await gate("writeRecord")
            if let index = stored.firstIndex(where: { $0.service == record.service }) {
                stored[index] = record
            } else {
                stored.append(record)
            }
            log.append("writeRecord \(record.generation) \(record.revision)")
        }

        func deleteRecord(service: String, generation: String) async throws {
            try await gate("deleteRecord")
            stored.removeAll { $0.service == service && $0.generation == generation }
            log.append("deleteRecord \(generation)")
        }

        func writeTombstone(_ tombstone: DebridTombstone) async throws {
            try await gate("writeTombstone")
            stones.append(tombstone)
            log.append("writeTombstone \(tombstone.generation)")
        }

        func deleteTombstone(_ tombstone: DebridTombstone) async throws {
            try await gate("deleteTombstone")
            stones.removeAll { $0.generation == tombstone.generation }
            log.append("deleteTombstone \(tombstone.generation)")
        }
    }

    /// Answers `restore` the way the core does.
    @MainActor private final class FakeCore: DebridCore {
        var acks: [String] = []
        var disconnects: [String] = []
        var restores: [[[String]]] = []

        func restore(records: [DebridRecord], tombstones: [DebridTombstone]) async throws -> RestoreAnswer {
            restores.append([records.map(DebridStoreTests.id), tombstones.map(DebridStoreTests.stone)])
            let gone = Set(tombstones.map(\.generation))
            let handed = Set(records.map(\.generation))
            return RestoreAnswer(
                refused: records.filter { gone.contains($0.generation) }.map(\.generation),
                tombstones: tombstones.filter { !handed.contains($0.generation) })
        }

        func ack(service: String, generation: String, revision: UInt64) async throws {
            acks.append("\(service) \(generation) \(revision)")
        }

        func disconnect(service: String, generation: String) async throws {
            disconnects.append("\(service) \(generation)")
        }
    }

    nonisolated private static func id(_ record: DebridRecord) -> String {
        "\(record.service) \(record.generation) \(record.revision)"
    }

    nonisolated private static func stone(_ tombstone: DebridTombstone) -> String {
        "\(tombstone.service) \(tombstone.generation)"
    }

    private func decode<T: Decodable>(_ value: Any?, as type: T.Type) throws -> T {
        try JSONDecoder().decode(T.self, from: JSONSerialization.data(withJSONObject: value ?? []))
    }

    /// Lets queued work run until the helper is idle or waits on a held call.
    private func drain(_ store: DebridStore, _ keystore: FakeKeystore) async {
        for _ in 0..<10_000 {
            if !store.busy || keystore.waiting { break }
            await Task.yield()
        }
        // Then the callers awaiting what finished (errors, retries, cleanup).
        for _ in 0..<50 { await Task.yield() }
    }

    private func replay(_ testCase: [String: Any]) async throws {
        let keystore = FakeKeystore()
        let start = testCase["keystore"] as? [String: Any] ?? [:]
        keystore.stored = try decode(start["records"], as: [DebridRecord].self)
        keystore.stones = try decode(start["tombstones"], as: [DebridTombstone].self)
        let core = FakeCore()
        let errors = Errors()
        var store = DebridStore(keystore: keystore, core: core)
        let name = testCase["name"] as? String ?? "?"

        for step in testCase["steps"] as? [[String: Any]] ?? [] {
            let text = { (key: String) in step[key] as? String ?? "" }
            let strings = { (key: String) in step[key] as? [String] }
            switch text("op") {
            case "pending":
                store.pending(try decode(step["records"], as: [DebridRecord].self))
            case "disconnect":
                let label = "\(text("service")) \(text("generation"))"
                let done = store.disconnect(service: text("service"), generation: text("generation"))
                Task {
                    do { try await done.value } catch { errors.list.append(label) }
                }
            case "hold":
                keystore.holding = text("call")
            case "release":
                keystore.release()
            case "fail":
                keystore.failing[text("call")] = step["times"] as? Int ?? 0
            case "settle":
                await store.idle()
            case "restart":
                store = DebridStore(keystore: keystore, core: core)
                try await store.start()
            case "expect":
                if let want = strings("log") { XCTAssertEqual(keystore.log, want, "\(name): log") }
                if let want = strings("records") { XCTAssertEqual(keystore.stored.map(Self.id), want, "\(name): records") }
                if let want = strings("tombstones") { XCTAssertEqual(keystore.stones.map(Self.stone), want, "\(name): tombstones") }
                if let want = strings("acks") { XCTAssertEqual(core.acks, want, "\(name): acks") }
                if let want = strings("disconnects") { XCTAssertEqual(core.disconnects, want, "\(name): disconnects") }
                if let want = strings("errors") { XCTAssertEqual(errors.list, want, "\(name): errors") }
                if let want = step["restores"] as? [[String: [String]]] {
                    XCTAssertEqual(core.restores, want.map { [$0["records"] ?? [], $0["tombstones"] ?? []] }, "\(name): restores")
                }
            case let op:
                XCTFail("\(name): unknown op \(op)")
            }
            // Time passes between steps: a queued job starts.
            await drain(store, keystore)
        }
    }

    func testEverySharedCasePasses() async throws {
        let url = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .appending(path: "../../shared/debrid-storage-vectors.json")
            .standardizedFileURL
        let file = try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any]
        let cases = file?["cases"] as? [[String: Any]] ?? []
        XCTAssertEqual(cases.count, 10, "cases in \(url.path)")
        for testCase in cases {
            try await replay(testCase)
        }
    }

    /// A keystore and core that callers on any thread share (the racing cases).
    private actor Shared: DebridKeystore, DebridCore {
        var stored: [String: DebridRecord] = [:]
        var stones: [DebridTombstone] = []
        var cancelWrites = false

        init(_ record: DebridRecord?) {
            if let record { stored[record.service] = record }
        }

        func setCancelWrites(_ on: Bool) { cancelWrites = on }
        func records() -> [DebridRecord] { Array(stored.values) }
        func tombstones() -> [DebridTombstone] { stones }

        func writeRecord(_ record: DebridRecord) throws {
            if cancelWrites { throw CancellationError() }
            stored[record.service] = record
        }

        func deleteRecord(service: String, generation: String) {
            if stored[service]?.generation == generation { stored[service] = nil }
        }

        func writeTombstone(_ tombstone: DebridTombstone) { stones.append(tombstone) }
        func deleteTombstone(_ tombstone: DebridTombstone) { stones.removeAll { $0.generation == tombstone.generation } }
        func restore(records: [DebridRecord], tombstones: [DebridTombstone]) -> RestoreAnswer { RestoreAnswer(refused: [], tombstones: []) }
        func ack(service: String, generation: String, revision: UInt64) {}
        func disconnect(service: String, generation: String) {}
    }

    /// Holds callers until it opens, so two tasks start their calls together.
    private actor Gate {
        private var opened = false
        private var waiters: [CheckedContinuation<Void, Never>] = []

        func wait() async {
            if opened { return }
            await withCheckedContinuation { waiters.append($0) }
        }

        func open() {
            opened = true
            for waiter in waiters { waiter.resume() }
            waiters.removeAll()
        }
    }

    nonisolated private static func record(_ generation: String, _ revision: UInt64) -> DebridRecord {
        DebridRecord(service: "torbox", generation: generation, revision: revision, credential: .key("k\(revision)"))
    }

    /// Waits up to five seconds for every queue to go idle without spinning on a stuck pump.
    private func settled(_ store: DebridStore) async -> Bool {
        for _ in 0..<5_000 {
            if !store.busy { return true }
            try? await Task.sleep(for: .milliseconds(1))
        }
        return false
    }

    func testAPushRacingADisconnectNeverOutlivesIt() async {
        for round in 0..<300 {
            let generation = "g\(round)"
            let shared = Shared(Self.record(generation, 1))
            let store = DebridStore(keystore: shared, core: shared)
            let gate = Gate()
            // Two tasks off the main actor, released together: either may reach the store first.
            let push = Task.detached {
                await gate.wait()
                await store.pending([Self.record(generation, 2)])
            }
            let drop = Task.detached {
                await gate.wait()
                try? await store.disconnect(service: "torbox", generation: generation).value
            }
            await gate.open()
            await push.value
            await drop.value
            let idle = await settled(store)
            XCTAssertTrue(idle, "round \(round): queues idle")
            let left = await shared.stored["torbox"]
            let stones = await shared.stones
            XCTAssertNil(left, "round \(round): the record outlived its disconnect")
            XCTAssertEqual(stones, [], "round \(round): tombstones")
            if left != nil || !idle { return }
        }
    }

    func testACancelledWriteLeavesTheQueueUsable() async {
        let shared = Shared(nil)
        let store = DebridStore(keystore: shared, core: shared)
        await shared.setCancelWrites(true)
        // Two writes queued: the first is cancelled, the one behind it is settled too.
        store.pending([Self.record("g1", 1)])
        store.pending([Self.record("g1", 1)])
        let stopped = await settled(store)
        XCTAssertTrue(stopped, "the cancelled pump unregisters, so idle returns")
        await shared.setCancelWrites(false)
        store.pending([Self.record("g1", 2)])
        let written = await settled(store)
        XCTAssertTrue(written, "later work starts a new pump")
        let revision = await shared.stored["torbox"]?.revision
        XCTAssertEqual(revision, 2)
        if stopped && written { await store.idle() }
    }

    func testCredentialsNeverPrint() throws {
        let secrets = ["KEY-9f3a", "CLIENT-ID-77", "CLIENT-SECRET-55", "REFRESH-TOKEN-13", "ACCESS-TOKEN-21"]
        let key = DebridRecord(service: "torbox", generation: "g1", revision: 1, credential: .key(secrets[0]))
        let oauth = DebridRecord(
            service: "realdebrid", generation: "g2", revision: 4,
            credential: .oauth(
                clientId: secrets[1], clientSecret: secrets[2], refreshToken: secrets[3], accessToken: secrets[4],
                issuedAtMs: 1_760_000_000_000, expiresInMs: 3_600_000))
        for record in [key, oauth] {
            var dumped = ""
            dump(record, to: &dumped)
            var credentialDump = ""
            dump(record.credential, to: &credentialDump)
            let printed = [
                record.description, record.debugDescription, String(describing: record), String(reflecting: record), "\(record)",
                record.credential.description, record.credential.debugDescription, String(reflecting: record.credential),
                "\([record])", dumped, credentialDump,
            ]
            for text in printed {
                for secret in secrets {
                    XCTAssertFalse(text.contains(secret), "\(secret) printed in: \(text)")
                }
            }
            XCTAssertTrue(record.description.contains(record.generation))
        }
        // The JSON the keystore holds still carries the secret, tagged by kind.
        let json = try String(decoding: JSONEncoder().encode(key), as: UTF8.self)
        XCTAssertTrue(json.contains("\"kind\":\"key\"") && json.contains(secrets[0]))
        XCTAssertEqual(try JSONDecoder().decode(DebridRecord.self, from: Data(json.utf8)), key)
        let round = try JSONDecoder().decode(DebridRecord.self, from: JSONEncoder().encode(oauth))
        XCTAssertEqual(round, oauth)
    }
}
