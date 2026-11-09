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
    /// Bumped when the stream server came back on another port: the URLs
    /// handed out before are dead, and a player opens its source again.
    private(set) var serverMoves = 0

    @ObservationIgnored private var router = CoreRouter()
    @ObservationIgnored private var core: MarqueeCore?
    /// Messages sent while the core opens, in order; they go out once it is open.
    @ObservationIgnored private var queued: [String] = []
    /// A profile removed on another device (the session coordinator listens).
    @ObservationIgnored var onProfileGone: ((String) -> Void)?
    /// An Incognito push, `ended` or `endsSoon` (the session coordinator listens).
    @ObservationIgnored var onIncognito: ((String) -> Void)?
    @ObservationIgnored private let directory: URL

    /// Opens the core in the app's data directory; tests pass a directory of their own.
    init(directory: URL = CoreClient.dataDirectory()) {
        self.directory = directory
        CoreClient.excludeFromBackup(dataDirectory: directory)
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
        // Weak: the router keeps this handler, so a strong hold would keep every replaced router.
        router.onDied { [weak self, weak router] reason in
            Task { @MainActor in
                guard let self, let router, self.router === router else { return }
                // Falling behind is no crash: a new core opens at once.
                if reason.hasPrefix("overflow") { self.restart() } else { self.failure = ErrorWords.of(kind: "coreStopped") }
            }
        }
        router.onProfileGone { [weak self] profile in
            Task { @MainActor in self?.onProfileGone?(profile) }
        }
        router.onIncognito { [weak self] event in
            Task { @MainActor in self?.onIncognito?(event) }
        }
        (self.router, core, queued, failure) = (router, nil, [], nil)
        let dir = directory
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
                // Error reports first (plans/error-reports.md 5.1): who the app is, before anything queued.
                if Diagnostics.wanted {
                    send("reportContext", Diagnostics.context(coreVersion: core.version()))
                    #if canImport(MetricKit) && !os(tvOS)
                    Diagnostics.listen { [weak self] report in self?.send("report", ["report": report]) }
                    #endif
                }
                for text in queued { core.send(json: text) }
                queued = []
            case .failure(let error):
                // Its reason goes to the log; the screen says it in words.
                coreLog.fault("core could not open: \(String(describing: error), privacy: .public)")
                let words = ErrorWords.of(kind: "coreNotStarted")
                failure = words
                queued = []
                router.fail(CoreError(words, kind: "coreNotStarted"))
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
                    coreLog.named { $0.error("undecodable \(view, privacy: .public) update: \(error, privacy: .public)") }
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

    /// An op delivered at once, so it reaches the core in the order called, before anything sent after
    /// it (a `Task` around `request` starts whenever the main actor gets to it). Nothing waits on an
    /// earlier reply. `done` gets nil when the core answers, else the failure, or the encoding error
    /// when a field cannot be sent; a reply from before a profile switch is dropped, as `request` drops it.
    /// Each call waits in the router until its reply: for user-rate senders, not per-tick ones.
    func sendNow(_ op: String, _ fields: [String: any Sendable], done: @escaping @MainActor @Sendable (CoreError?) -> Void) {
        if core == nil, let failure { done(CoreError(failure, kind: "said")); return }
        let router = router
        let id = router.nextId()
        let text: String
        do {
            text = try Self.encode(op: op, id: id, fields: fields)
        } catch {
            done(error as? CoreError ?? CoreError(error.localizedDescription))
            return
        }
        router.wait(for: id) { result in
            switch result {
            case .success: Task { @MainActor in done(nil) }
            case .failure(let error) where error.code == "switched": break
            case .failure(let error): Task { @MainActor in done(error) }
            }
        }
        deliver(text)
    }

    /// A view's command by `sendNow`: in call order, a failure to `failed`.
    func command(_ sub: UInt64, _ command: [String: any Sendable], failed: @escaping @MainActor @Sendable (CoreError) -> Void) {
        sendNow("command", ["sub": sub, "command": command]) { error in if let error { failed(error) } }
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

    /// `lifecycle background`, awaited: the core answers once running Trakt
    /// scrobbles are sent, or after 4 s. False when it did not answer within
    /// `limit` (the app's time in the background is short: it never waits longer).
    @discardableResult
    func lifecycleBackground(within limit: Duration = .milliseconds(4500)) async -> Bool {
        await withCheckedContinuation { (done: CheckedContinuation<Bool, Never>) in
            let first = FirstAnswer(done)
            Task { first.give((try? await self.request("lifecycle", ["event": "background"]) { _ in }) != nil) }
            Task {
                try? await Task.sleep(for: limit)
                first.give(false)
            }
        }
    }

    /// The first of several answers resumes; the others are dropped.
    @MainActor private final class FirstAnswer {
        private var done: CheckedContinuation<Bool, Never>?

        init(_ done: CheckedContinuation<Bool, Never>) {
            self.done = done
        }

        func give(_ answer: Bool) {
            done?.resume(returning: answer)
            done = nil
        }
    }

    /// `lifecycle foreground`, on every return to the front: the core binds its
    /// stream server again when the system took the listener while the app
    /// was suspended (review apple-journey P1-1).
    func foreground() async {
        guard let reply = try? await call("lifecycle", ["event": "foreground"], as: ForegroundReply.self) else { return }
        if reply.moved { serverMoves += 1 }
    }

    /// Tells the core about app and system events (`foreground`, `background`,
    /// `memoryPressure`, `networkChanged`).
    func lifecycle(_ event: String, _ fields: [String: any Sendable] = [:]) {
        send("lifecycle", fields.merging(["event": event]) { _, event in event })
    }

    /// Sends one request; `decode` runs on the core thread with the reply.
    @discardableResult
    func request<R: Sendable>(
        _ op: String, _ fields: [String: any Sendable], decode: @escaping @Sendable (Data) throws -> R
    ) async throws -> R {
        if core == nil, let failure { throw CoreError(failure, kind: "said") }
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
        // A value JSON cannot hold (a Date, a struct) throws here: JSONSerialization would stop the app.
        guard JSONSerialization.isValidJSONObject(message) else {
            throw CoreError("\(op) has a field the core cannot read", code: "unencodable")
        }
        return String(decoding: try JSONSerialization.data(withJSONObject: message), as: UTF8.self)
    }

    /// Where the core keeps torrent data: `torrents` in its data directory
    /// (mq-runtime `Core::open`), up to 6 GiB.
    nonisolated static func torrentDirectory(in dataDirectory: URL) -> URL {
        dataDirectory.appending(path: "torrents", directoryHint: .isDirectory)
    }

    /// Where the core keeps downloads (plans/offline-downloads.md 4.2): `downloads` in its data directory.
    nonisolated static func downloadDirectory(in dataDirectory: URL) -> URL {
        dataDirectory.appending(path: "downloads", directoryHint: .isDirectory)
    }

    /// Where Incognito's scopes live while they run (plans/batch-10-incognito.md 2.3): the core never removes it.
    nonisolated static func incognitoDirectory(in dataDirectory: URL) -> URL {
        dataDirectory.appending(path: "incognito", directoryHint: .isDirectory)
    }

    /// Torrent data is a cache, and downloads are videos the device can fetch again: both are made
    /// before the core opens and left out of iCloud and Time Machine backups (engine review r3 P3-9,
    /// plans/offline-downloads.md 4.2), and so is Incognito's folder, which no backup may keep. tvOS keeps the
    /// whole data directory under Caches already.
    nonisolated static func excludeFromBackup(dataDirectory: URL) {
        for var folder in [torrentDirectory(in: dataDirectory), downloadDirectory(in: dataDirectory), incognitoDirectory(in: dataDirectory)] {
            try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            var values = URLResourceValues()
            values.isExcludedFromBackup = true
            try? folder.setResourceValues(values)
        }
    }

    static func dataDirectory() -> URL {
        #if DEBUG
        // `-MQDataDir <path>`: a walk's own library, apart from the app's.
        if let path = UserDefaults.standard.string(forKey: "MQDataDir") { return URL(filePath: path, directoryHint: .isDirectory) }
        #endif
        #if os(tvOS)
        // tvOS only offers purgeable local storage; sync is the durable copy.
        let base = URL.cachesDirectory
        #else
        let base = URL.applicationSupportDirectory
        #endif
        return base.appending(path: Bundle.main.bundleIdentifier ?? "Marquee", directoryHint: .isDirectory)
    }
}

/// `lifecycle foreground`'s answer; an older core sends `{}`.
struct ForegroundReply: Decodable, Sendable {
    let serverMoved: Bool?

    var moved: Bool { serverMoved == true }
}
