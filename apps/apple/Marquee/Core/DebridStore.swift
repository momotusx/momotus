// Keeps the platform keystore in step with the core's debrid accounts
// (plans/core-protocol.md, Debrid; plans/c10-handoff.md A1). Every keystore
// change for a service runs through one serial queue, in the order the core
// produced the records and the user acted. The shared cases live in
// apps/shared/debrid-storage-vectors.json; the web and Android helpers replay
// the same file.

/// The platform keystore as the debrid helper sees it.
protocol DebridKeystore: Sendable {
    func records() async throws -> [DebridRecord]
    func tombstones() async throws -> [DebridTombstone]
    /// One record per service: replaces whatever the service had.
    func writeRecord(_ record: DebridRecord) async throws
    /// Removes the service's record only when it has this generation.
    func deleteRecord(service: String, generation: String) async throws
    func writeTombstone(_ tombstone: DebridTombstone) async throws
    func deleteTombstone(_ tombstone: DebridTombstone) async throws
}

/// The core calls the helper makes: `restoreDebrid`, `ackDebrid`, `disconnectDebrid`.
protocol DebridCore: Sendable {
    func restore(records: [DebridRecord], tombstones: [DebridTombstone]) async throws -> RestoreAnswer
    func ack(service: String, generation: String, revision: UInt64) async throws
    func disconnect(service: String, generation: String) async throws
}

enum DebridStoreError: Error {
    /// A record delete reported success but the record is still stored.
    case recordStillStored
}

@MainActor
final class DebridStore {
    private let keystore: any DebridKeystore
    private let core: any DebridCore
    private var queues: [String: [Job]] = [:]
    private var pumps: [String: Task<Void, Never>] = [:]
    /// Generations with a tombstone written or queued: never written again.
    private var gone: Set<String> = []
    /// Generations whose tombstone was stored once: never writable again, and a
    /// repeated disconnect goes on from there without a new tombstone (core and
    /// cleanup are idempotent), so no later failure can reopen them.
    private var committed: Set<String> = []
    /// Tombstones whose record delete failed; retried with the next push.
    private var unfinished: [DebridTombstone] = []
    private var disconnecting: [String: Task<Void, any Error>] = [:]

    init(keystore: any DebridKeystore, core: any DebridCore) {
        self.keystore = keystore
        self.core = core
    }

    /// True while any service has queued or running work.
    var busy: Bool { !pumps.isEmpty }

    /// At start, before any view: finish interrupted disconnects, then restore.
    func start() async throws {
        for tombstone in try await keystore.tombstones() {
            gone.insert(tombstone.generation)
            committed.insert(tombstone.generation)
            do { try await finish(tombstone) } catch { remember(tombstone) }
        }
        // Records with a tombstone go too: the core refuses them, and a tombstone
        // whose record is still stored is never handed back as deletable.
        let answer = try await core.restore(records: try await keystore.records(), tombstones: try await keystore.tombstones())
        for tombstone in answer.tombstones {
            try? await keystore.deleteTombstone(tombstone)
        }
    }

    /// Every `debrid` view push: store each pending record, then acknowledge it.
    func pending(_ records: [DebridRecord]) {
        let interrupted = unfinished
        unfinished.removeAll()
        for tombstone in interrupted {
            let job = enqueue(tombstone.service, tombstone.generation, write: false) { [self] in try await retry(tombstone) }
            Task { [self] in
                do { try await job.value() } catch { remember(tombstone) }
            }
        }
        // A failed write is not acknowledged; the next push carries it again.
        for record in records {
            enqueue(record.service, record.generation, write: true) { [self] in try await save(record) }
        }
    }

    /// Disconnects one generation: tombstone, core, record, then the tombstone.
    /// Fails when the tombstone cannot be written: nothing changed, the
    /// account stays connected.
    @discardableResult
    func disconnect(service: String, generation: String) -> Task<Void, any Error> {
        // One disconnect per generation at a time: a second request shares the
        // first, so a failure cannot reopen the generation under a later one.
        if let running = disconnecting[generation] { return running }
        let job = disconnectNow(service: service, generation: generation)
        let done = Task { [self] in
            defer { disconnecting[generation] = nil }
            try await job.value()
        }
        disconnecting[generation] = done
        return done
    }

    /// Returns once every queue is idle (tests, shutdown).
    func idle() async {
        while let pump = pumps.values.first {
            await pump.value
        }
    }

    private func disconnectNow(service: String, generation: String) -> Job {
        gone.insert(generation)
        for job in queues[service] ?? [] where job.write && job.generation == generation {
            job.dropped = true
        }
        let tombstone = DebridTombstone(service: service, generation: generation)
        // Settled without running (cancellation), or failing to write its
        // tombstone, it leaves the generation writable again, unless an earlier
        // disconnect stored a tombstone for it: that generation stays gone.
        let reopen = { @MainActor [self] in
            if !committed.contains(generation) { gone.remove(generation) }
        }
        return enqueue(service, generation, write: false, discard: reopen) { [self] in
            // An earlier disconnect stored its tombstone (its core call or cleanup
            // may have failed since): go on from there.
            if !committed.contains(generation) {
                do {
                    try await keystore.writeTombstone(tombstone)
                } catch {
                    reopen()
                    throw error
                }
                committed.insert(generation)
            }
            // The tombstone is durable now: from here any failure, cancellation
            // included, is remembered and the rest (core, then record and
            // tombstone) runs again with the next push.
            do {
                try await core.disconnect(service: service, generation: generation)
            } catch {
                remember(tombstone)
                throw error
            }
            do {
                try await finish(tombstone)
                unfinished.removeAll { $0.generation == generation }
            } catch {
                remember(tombstone)
                if error is CancellationError { throw error }
            }
        }
    }

    /// An interrupted disconnect: tell the core again (it is idempotent), then clean up.
    private func retry(_ tombstone: DebridTombstone) async throws {
        try await core.disconnect(service: tombstone.service, generation: tombstone.generation)
        try await finish(tombstone)
    }

    private func save(_ record: DebridRecord) async throws {
        let stored = try await keystore.records().first { $0.service == record.service }
        let newer = stored.map { $0.generation == record.generation && $0.revision > record.revision } ?? false
        if !newer { try await keystore.writeRecord(record) }
        try await core.ack(service: record.service, generation: record.generation, revision: record.revision)
    }

    /// Deletes the record, reads back that it is gone, then the tombstone.
    private func finish(_ tombstone: DebridTombstone) async throws {
        try await keystore.deleteRecord(service: tombstone.service, generation: tombstone.generation)
        let left = try await keystore.records().contains {
            $0.service == tombstone.service && $0.generation == tombstone.generation
        }
        if left { throw DebridStoreError.recordStillStored }
        try await keystore.deleteTombstone(tombstone)
    }

    private func remember(_ tombstone: DebridTombstone) {
        unfinished.removeAll { $0.generation == tombstone.generation }
        unfinished.append(tombstone)
    }

    @discardableResult
    private func enqueue(
        _ service: String, _ generation: String, write: Bool, discard: @escaping @MainActor () -> Void = {},
        _ run: @escaping @MainActor () async throws -> Void
    ) -> Job {
        let job = Job(generation: generation, write: write, run: run, discard: discard)
        // Checked with the insert, in one actor step: a write of a generation a
        // disconnect marked gone is never queued behind its cleanup.
        if write && gone.contains(generation) {
            job.settle(.success(()))
            return job
        }
        queues[service, default: []].append(job)
        if pumps[service] == nil {
            // Registered before it runs, so its own exit can remove it. A Swift
            // task runs its body even when cancelled before it starts, so the
            // pump's own exit paths always get to unregister it.
            pumps[service] = Task { [self] in await pump(service) }
        }
        return job
    }

    /// A pump ended by cancellation: unregister it and settle what it left
    /// queued, undoing what queueing did, so `idle` returns, later work starts
    /// a new pump, and a disconnect that never ran does not keep its
    /// generation marked gone.
    private func abandon(_ service: String) {
        let left = queues.removeValue(forKey: service) ?? []
        pumps[service] = nil
        for job in left {
            job.discard()
            job.settle(.failure(CancellationError()))
        }
    }

    private func pump(_ service: String) async {
        while true {
            // In the same step as the empty check: a job queued later starts a new pump.
            guard let job = queues[service]?.first else {
                queues[service] = nil
                pumps[service] = nil
                return
            }
            queues[service]?.removeFirst()
            if job.dropped {
                job.settle(.success(()))
                continue
            }
            do {
                try await job.run()
                job.settle(.success(()))
            } catch is CancellationError {
                // The work is being cancelled: the pump stops; `abandon` settles
                // what is left queued.
                job.settle(.failure(CancellationError()))
                abandon(service)
                return
            } catch {
                job.settle(.failure(error))
            }
        }
    }

    /// One keystore change waiting in a service's queue, and its outcome.
    @MainActor private final class Job {
        let generation: String
        let write: Bool
        let run: @MainActor () async throws -> Void
        /// Undoes what queueing it did, when it is settled without running (cancellation).
        let discard: @MainActor () -> Void
        var dropped = false
        private var result: Result<Void, any Error>?
        private var waiters: [CheckedContinuation<Void, any Error>] = []

        init(generation: String, write: Bool, run: @escaping @MainActor () async throws -> Void, discard: @escaping @MainActor () -> Void) {
            (self.generation, self.write, self.run, self.discard) = (generation, write, run, discard)
        }

        func settle(_ outcome: Result<Void, any Error>) {
            result = outcome
            for waiter in waiters { waiter.resume(with: outcome) }
            waiters.removeAll()
        }

        func value() async throws {
            if let result { return try result.get() }
            try await withCheckedThrowingContinuation { waiters.append($0) }
        }
    }
}
