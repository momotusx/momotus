import Foundation
import Synchronization
import os

let coreLog = Logger(subsystem: "app.marquee", category: "core")

struct CoreError: LocalizedError, Sendable {
    let message: String
    /// Stable name for the errors an app acts on (sync errors); never parse `message`.
    var code: String?
    init(_ message: String, code: String? = nil) {
        self.message = message
        self.code = code
    }
    var errorDescription: String? { message }
}

/// Receives every core message on a core thread (the protocol in
/// plans/core-protocol.md). Replies go to the waiter registered for their
/// `id`; view pushes go to the handler routed for their `sub`. Messages
/// arrive in order on one thread, and handlers decode there, off the main
/// thread. One router per open core.
final class CoreRouter: CoreListener {
    typealias Waiter = @Sendable (Result<Data, CoreError>) -> Void
    typealias PushHandler = @Sendable (Data) -> Void

    private struct State {
        var nextId: UInt64 = 1
        var waiters: [UInt64: Waiter] = [:]
        var routes: [UInt64: PushHandler] = [:]
        /// Pushes for a sub whose subscribe reply has not been handled yet.
        var early: [UInt64: [Data]] = [:]
        var highestRouted: UInt64 = 0
        var onDied: (@Sendable (String) -> Void)?
    }

    private struct Head: Decodable {
        let id: UInt64?
        let ok: Bool?
        let error: String?
        let code: String?
        let sub: UInt64?
        let core: String?
        let reason: String?
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

    /// Routes a sub's pushes, first replaying any that arrived early. Call it
    /// from a waiter (on the core thread) so no push can slip in between.
    func route(sub: UInt64, _ handler: @escaping PushHandler) {
        let early = state.withLock { state in
            state.routes[sub] = handler
            state.highestRouted = max(state.highestRouted, sub)
            return state.early.removeValue(forKey: sub) ?? []
        }
        early.forEach(handler)
    }

    func unroute(sub: UInt64) {
        _ = state.withLock { $0.routes.removeValue(forKey: sub) }
    }

    func onDied(_ handler: @escaping @Sendable (String) -> Void) {
        state.withLock { $0.onDied = handler }
    }

    /// Answers every waiting request with `error` (the core never opened).
    func fail(_ error: CoreError) {
        let waiters = state.withLock { state in
            defer { state.waiters.removeAll() }
            return Array(state.waiters.values)
        }
        waiters.forEach { $0(.failure(error)) }
    }

    func onMessage(json: String) {
        let data = Data(json.utf8)
        guard let head = try? JSONDecoder().decode(Head.self, from: data) else {
            // Its size only: a message can carry addon links and their keys.
            coreLog.error("unreadable core message, \(json.utf8.count, privacy: .public) bytes")
            return
        }
        if let sub = head.sub {
            push(sub, data)
        } else if let id = head.id {
            let waiter = state.withLock { $0.waiters.removeValue(forKey: id) }
            waiter?(head.ok == true ? .success(data) : .failure(CoreError(head.error ?? "request failed", code: head.code)))
        } else if head.core == "died" {
            let reason = head.reason ?? "unknown"
            coreLog.fault("core died: \(reason, privacy: .public)")
            let (waiters, onDied) = state.withLock { state in
                defer { state.waiters.removeAll() }
                return (Array(state.waiters.values), state.onDied)
            }
            waiters.forEach { $0(.failure(CoreError("The core stopped: \(reason)"))) }
            onDied?(reason)
        } else if head.ok == false {
            coreLog.error("core rejected a request: \(head.error ?? "", privacy: .public)")
        }
    }

    private func push(_ sub: UInt64, _ data: Data) {
        let handler: PushHandler? = state.withLock { state in
            if let handler = state.routes[sub] { return handler }
            // Subs only grow, so one above every routed sub is early; one
            // below belongs to a closed view and is dropped.
            if sub > state.highestRouted, state.early[sub, default: []].count < 64 {
                state.early[sub, default: []].append(data)
            }
            return nil
        }
        handler?(data)
    }
}
