import Foundation
import Observation

/// What an open view delivers, in core order: the snapshot first, then one
/// element per core batch.
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
/// as async streams, and commands. One per process.
@MainActor @Observable
final class CoreClient {
    /// Why the core could not open; the UI shows it instead of content.
    private(set) var openError: String?

    @ObservationIgnored private let router = CoreRouter()
    @ObservationIgnored private let core: MarqueeCore?

    init() {
        do {
            core = try MarqueeCore.open(
                dataDir: Self.dataDirectory().path(percentEncoded: false),
                profile: "default",
                deviceId: Self.deviceId(),
                listener: router
            )
        } catch {
            core = nil
            openError = String(describing: error)
        }
    }

    var version: String { core?.version() ?? "unavailable" }

    /// Opens a view (`home`, `title`). The first event is the snapshot.
    func open<S: Decodable & Sendable, U: Decodable & Sendable>(
        _ view: String, params: [String: String] = [:], as types: (S.Type, U.Type)
    ) async throws -> CoreView<S, U> {
        let (events, sink) = AsyncStream.makeStream(of: ViewEvent<S, U>.self)
        let router = router
        let sub = try await request("subscribe", ["view": view, "params": params]) { data in
            let opened = try JSONDecoder().decode(Reply<Opened<S>>.self, from: data).result
            // Route pushes before handing over the snapshot: the next message
            // on this thread may already be a push for this view.
            router.route(sub: opened.sub) { data in
                do {
                    sink.yield(.updates(try JSONDecoder().decode(Push<U>.self, from: data).updates))
                } catch {
                    // A lost batch would leave the list out of step with the core.
                    coreLog.error("undecodable \(view, privacy: .public) update: \(error, privacy: .public)")
                    sink.finish()
                }
            }
            sink.yield(.snapshot(opened.snapshot))
            return opened.sub
        }
        sink.onTermination = { [weak self] _ in
            router.unroute(sub: sub)
            Task { @MainActor in self?.fireAndForget("unsubscribe", ["sub": sub]) }
        }
        return CoreView(sub: sub, events: events)
    }

    func command(_ sub: UInt64, _ command: [String: any Sendable]) async throws {
        try await request("command", ["sub": sub, "command": command]) { _ in }
    }

    func installAddon(url: String) async throws {
        try await request("installAddon", ["url": url]) { _ in }
    }

    /// Sends one request; `decode` runs on the core thread with the reply.
    @discardableResult
    private func request<R: Sendable>(
        _ op: String, _ fields: [String: any Sendable], decode: @escaping @Sendable (Data) throws -> R
    ) async throws -> R {
        guard let core else { throw CoreError(openError ?? "core is not open") }
        let id = router.nextId()
        let text = try Self.encode(op: op, id: id, fields: fields)
        let router = router
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

    private func fireAndForget(_ op: String, _ fields: [String: any Sendable]) {
        guard let core, let text = try? Self.encode(op: op, id: router.nextId(), fields: fields) else { return }
        core.send(json: text)
    }

    private static func encode(op: String, id: UInt64, fields: [String: any Sendable]) throws -> String {
        var message: [String: Any] = fields
        message["id"] = id
        message["op"] = op
        return String(decoding: try JSONSerialization.data(withJSONObject: message), as: UTF8.self)
    }

    private static func dataDirectory() -> URL {
        #if os(tvOS)
        // tvOS only offers purgeable local storage; sync is the durable copy.
        let base = URL.cachesDirectory
        #else
        let base = URL.applicationSupportDirectory
        #endif
        return base.appending(path: Bundle.main.bundleIdentifier ?? "Marquee", directoryHint: .isDirectory)
    }

    /// Stable per-install id for the core's sync clock.
    private static func deviceId() -> UInt64 {
        let defaults = UserDefaults.standard
        if let stored = defaults.object(forKey: "deviceId") as? Int { return UInt64(bitPattern: Int64(stored)) }
        let id = UInt64.random(in: 1...UInt64.max)
        defaults.set(Int(Int64(bitPattern: id)), forKey: "deviceId")
        return id
    }
}
