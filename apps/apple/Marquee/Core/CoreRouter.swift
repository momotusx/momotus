import Foundation
import Synchronization
import os

let coreLog = Logger(subsystem: "app.marquee", category: "core")

struct CoreError: LocalizedError, Sendable {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}

/// Receives every core message on a core thread (the protocol in
/// crates/mq-bridge). Replies go to the waiter registered for their `id`;
/// view pushes go to the handler routed for their `sub`. Messages arrive in
/// order on one thread, and handlers decode there, off the main thread.
final class CoreRouter: CoreListener {
    typealias Waiter = @Sendable (Result<Data, CoreError>) -> Void
    typealias PushHandler = @Sendable (Data) -> Void

    private struct State {
        var nextId: UInt64 = 1
        var waiters: [UInt64: Waiter] = [:]
        var routes: [UInt64: PushHandler] = [:]
    }

    private struct Head: Decodable {
        let id: UInt64?
        let ok: Bool?
        let error: String?
        let sub: UInt64?
    }

    private let state = Mutex(State())

    func nextId() -> UInt64 {
        state.withLock { state in
            defer { state.nextId += 1 }
            return state.nextId
        }
    }

    func wait(for id: UInt64, _ waiter: @escaping Waiter) {
        state.withLock { $0.waiters[id] = waiter }
    }

    func route(sub: UInt64, _ handler: @escaping PushHandler) {
        state.withLock { $0.routes[sub] = handler }
    }

    func unroute(sub: UInt64) {
        _ = state.withLock { $0.routes.removeValue(forKey: sub) }
    }

    func onMessage(json: String) {
        let data = Data(json.utf8)
        guard let head = try? JSONDecoder().decode(Head.self, from: data) else {
            coreLog.error("unreadable core message: \(json.prefix(200), privacy: .public)")
            return
        }
        if let sub = head.sub {
            state.withLock { $0.routes[sub] }?(data)
        } else if let id = head.id, let waiter = state.withLock({ $0.waiters.removeValue(forKey: id) }) {
            waiter(head.ok == true ? .success(data) : .failure(CoreError(head.error ?? "request failed")))
        }
    }
}
