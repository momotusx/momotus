import Foundation
import Observation

/// What an open view delivers, in core order: the snapshot first, then one
/// element per core batch. Record views push whole snapshots (apply the last);
/// Home and Title push diffs (apply all, in order).
enum ViewEvent<Snapshot: Sendable, Update: Sendable>: Sendable {
    case snapshot(Snapshot)
    case updates([Update])
}

/// An open core view. Ending iteration of `events` (or cancelling the task
/// that iterates it) unsubscribes.
struct CoreView<Snapshot: Sendable, Update: Sendable>: Sendable {
    let sub: UInt64
    let events: AsyncStream<ViewEvent<Snapshot, Update>>
}

private struct Reply<R: Decodable>: Decodable { let result: R }
private struct Opened<S: Decodable>: Decodable {
    let sub: UInt64
    let snapshot: S
}
private struct Push<U: Decodable>: Decodable { let updates: [U] }

/// The app's handle on the Rust core: request and reply pairing by id, views
/// as async streams, commands, and reopening the core if it dies.
@MainActor @Observable
final class CoreClient {
    /// Why the core could not open or stopped; the UI shows it instead of content.
    private(set) var failure: String?
    /// Bumped when the core is opened again; views key their tasks on it.
    private(set) var generation = 0

    @ObservationIgnored private var router = CoreRouter()
    @ObservationIgnored private var core: MarqueeCore?
    /// Messages sent while the core opens, in order; they go out once it is open.
    @ObservationIgnored private var queued: [String] = []
    /// A profile removed on another device (the session coordinator listens).
    @ObservationIgnored var onProfileGone: ((String) -> Void)?

    init() {
        openCore()
    }

    var version: String { core?.version() ?? "unavailable" }

    #if DEBUG
    /// Hands the router the message a panicking core sends (`-MQKillCoreAfter`).
    func debugDie() {
        router.onMessage(json: #"{"core":"died","reason":"debug: a simulated panic"}"#)
    }
    #endif

    /// Opens the core again after it stopped; open views resubscribe.
    func restart() {
        openCore()
        generation += 1
    }

    /// Opens the core off the main thread, so the first frame never waits for
    /// it: until it is open, requests and messages queue in order behind it.
    private func openCore() {
        let router = CoreRouter()
        router.onDied { [weak self] reason in
            Task { @MainActor in
                guard let self, self.router === router else { return }
                // Falling behind is no crash: a new core opens at once.
                if reason.hasPrefix("overflow") { self.restart() } else { self.failure = "The core stopped (\(reason))." }
            }
        }
        router.onProfileGone { [weak self] profile in
            Task { @MainActor in self?.onProfileGone?(profile) }
        }
        (self.router, core, queued, failure) = (router, nil, [], nil)
        let dir = Self.dataDirectory()
        // Started here, not from a main-actor task, so it runs while the first
        // frame is built rather than after it.
        let opening = Task.detached(priority: .userInitiated) {
            let start = ContinuousClock.now
            let opened = Result {
                try MarqueeCore.open(
                    dataDir: dir.path(percentEncoded: false),
                    profile: "default",
                    deviceId: DeviceId.current(fallbackDirectory: dir),
                    listener: router
                )
            }
            return (opened, (ContinuousClock.now - start) / .milliseconds(1))
        }
        Task {
            let (opened, took) = await opening.value
            // A newer open took over (a restart): this one is dropped.
            guard self.router === router else { return }
            switch opened {
            case .success(let core):
                coreLog.notice("""
                    metric core_open_s=\(Launch.elapsed(), format: .fixed(precision: 3), privacy: .public) \
                    core_open_ms=\(took, format: .fixed(precision: 1), privacy: .public)
                    """)
                self.core = core
                for text in queued { core.send(json: text) }
                queued = []
            case .failure(let error):
                let reason = "The core could not open: \(error)"
                failure = reason
                queued = []
                router.fail(CoreError(reason))
            }
        }
    }

    /// Hands a message to the core, or queues it while the core opens.
    private func deliver(_ text: String) {
        if let core {
            core.send(json: text)
        } else if failure == nil {
            queued.append(text)
        }
    }

    /// Opens a view (`home`, `title`, `versions`, ...). The first event is the snapshot.
    func open<S: Decodable & Sendable, U: Decodable & Sendable>(
        _ view: String, params: [String: any Sendable] = [:], as types: (S.Type, U.Type)
    ) async throws -> CoreView<S, U> {
        let (events, sink) = AsyncStream.makeStream(of: ViewEvent<S, U>.self)
        let router = router
        #if DEBUG
        // Which views subscribe to which core: a restart must open only `profiles` before its snapshot.
        coreLog.notice("debug: subscribe \(view, privacy: .public) on core \(self.generation, privacy: .public)")
        #endif
        let sub = try await request("subscribe", ["view": view, "params": params]) { data in
            let opened = try JSONDecoder().decode(Reply<Opened<S>>.self, from: data).result
            sink.yield(.snapshot(opened.snapshot))
            // Runs on the core thread before the next message, so pushes
            // (including any that arrived early) follow the snapshot in order.
            router.route(sub: opened.sub) { data in
                do {
                    sink.yield(.updates(try JSONDecoder().decode(Push<U>.self, from: data).updates))
                } catch {
                    // A lost batch would leave the view out of step with the core.
                    coreLog.error("undecodable \(view, privacy: .public) update: \(error, privacy: .public)")
                    sink.finish()
                }
            }
            return opened.sub
        }
        sink.onTermination = { [weak self] _ in
            router.unroute(sub: sub)
            // Only to the core that made it: numbers start at 1 again on a new
            // core, where this one could name another view.
            Task { @MainActor in
                guard let self, self.router === router else { return }
                self.send("unsubscribe", ["sub": sub])
            }
        }
        return CoreView(sub: sub, events: events)
    }

    func command(_ sub: UInt64, _ command: [String: any Sendable]) async throws {
        try await request("command", ["sub": sub, "command": command]) { _ in }
    }

    /// A command sent at once without waiting for its reply, so it reaches the
    /// core before anything sent after it (an unsubscribe, for example).
    func commandNow(_ sub: UInt64, _ command: [String: any Sendable]) {
        send("command", ["sub": sub, "command": command])
    }

    /// An op from the protocol's op table whose result is not needed.
    func perform(_ op: String, _ fields: [String: any Sendable] = [:]) async throws {
        try await request(op, fields) { _ in }
    }

    /// An op whose `result` decodes as `R`.
    func call<R: Decodable & Sendable>(_ op: String, _ fields: [String: any Sendable] = [:], as type: R.Type) async throws -> R {
        try await request(op, fields) { try JSONDecoder().decode(Reply<R>.self, from: $0).result }
    }

    /// Round trip to the core's request loop in milliseconds, or nil when it does not answer.
    func ping() async -> Int? {
        let start = ContinuousClock.now
        guard (try? await request("ping", [:]) { _ in }) != nil else { return nil }
        return Int((ContinuousClock.now - start) / .milliseconds(1))
    }

    /// Tells the core about app and system events (`foreground`, `background`,
    /// `memoryPressure`, `networkChanged`).
    func lifecycle(_ event: String) {
        send("lifecycle", ["event": event])
    }

    /// Sends one request; `decode` runs on the core thread with the reply.
    @discardableResult
    func request<R: Sendable>(
        _ op: String, _ fields: [String: any Sendable], decode: @escaping @Sendable (Data) throws -> R
    ) async throws -> R {
        if core == nil, let failure { throw CoreError(failure) }
        let router = router
        let id = router.nextId()
        let text = try Self.encode(op: op, id: id, fields: fields)
        return try await withCheckedThrowingContinuation { continuation in
            router.wait(for: id) { result in
                switch result {
                case .success(let data): continuation.resume(with: Result { try decode(data) })
                // A reply from before a profile switch: settled as cancelled, never shown.
                case .failure(let error) where error.code == "switched": continuation.resume(throwing: CancellationError())
                case .failure(let error): continuation.resume(throwing: error)
                }
            }
            deliver(text)
        }
    }

    /// Fire and forget: the reply is dropped by the router.
    private func send(_ op: String, _ fields: [String: any Sendable]) {
        guard let text = try? Self.encode(op: op, id: router.nextId(), fields: fields) else { return }
        deliver(text)
    }

    static func encode(op: String, id: UInt64, fields: [String: any Sendable]) throws -> String {
        // Opaque records (the sync account) go back exactly as the core gave them.
        var message: [String: Any] = fields.mapValues { ($0 as? JSONValue)?.foundation ?? $0 }
        message["id"] = id
        message["op"] = op
        return String(decoding: try JSONSerialization.data(withJSONObject: message), as: UTF8.self)
    }

    static func dataDirectory() -> URL {
        #if os(tvOS)
        // tvOS only offers purgeable local storage; sync is the durable copy.
        let base = URL.cachesDirectory
        #else
        let base = URL.applicationSupportDirectory
        #endif
        return base.appending(path: Bundle.main.bundleIdentifier ?? "Marquee", directoryHint: .isDirectory)
    }
}
