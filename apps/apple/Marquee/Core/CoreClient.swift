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

    init() {
        openCore()
    }

    var version: String { core?.version() ?? "unavailable" }

    /// Opens the core again after it stopped; open views resubscribe.
    func restart() {
        core = nil
        openCore()
        generation += 1
    }

    private func openCore() {
        let router = CoreRouter()
        router.onDied { [weak self] reason in
            Task { @MainActor in self?.failure = "The core stopped (\(reason))." }
        }
        do {
            let dir = Self.dataDirectory()
            core = try MarqueeCore.open(
                dataDir: dir.path(percentEncoded: false),
                profile: "default",
                deviceId: DeviceId.current(fallbackDirectory: dir),
                listener: router
            )
            self.router = router
            failure = nil
        } catch {
            failure = "The core could not open: \(error)"
        }
    }

    /// Opens a view (`home`, `title`, `versions`, ...). The first event is the snapshot.
    func open<S: Decodable & Sendable, U: Decodable & Sendable>(
        _ view: String, params: [String: any Sendable] = [:], as types: (S.Type, U.Type)
    ) async throws -> CoreView<S, U> {
        let (events, sink) = AsyncStream.makeStream(of: ViewEvent<S, U>.self)
        let router = router
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
            Task { @MainActor in self?.send("unsubscribe", ["sub": sub]) }
        }
        return CoreView(sub: sub, events: events)
    }

    func command(_ sub: UInt64, _ command: [String: any Sendable]) async throws {
        try await request("command", ["sub": sub, "command": command]) { _ in }
    }

    /// An op from the protocol's op table whose result is not needed.
    func perform(_ op: String, _ fields: [String: any Sendable] = [:]) async throws {
        try await request(op, fields) { _ in }
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
        guard let core else { throw CoreError(failure ?? "The core is not open.") }
        let router = router
        let id = router.nextId()
        let text = try Self.encode(op: op, id: id, fields: fields)
        return try await withCheckedThrowingContinuation { continuation in
            router.wait(for: id) { result in
                switch result {
                case .success(let data): continuation.resume(with: Result { try decode(data) })
                case .failure(let error): continuation.resume(throwing: error)
                }
            }
            core.send(json: text)
        }
    }

    /// Fire and forget: the reply is dropped by the router.
    private func send(_ op: String, _ fields: [String: any Sendable]) {
        guard let core, let text = try? Self.encode(op: op, id: router.nextId(), fields: fields) else { return }
        core.send(json: text)
    }

    private static func encode(op: String, id: UInt64, fields: [String: any Sendable]) throws -> String {
        var message: [String: Any] = fields
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
