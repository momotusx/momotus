import Foundation
import Observation

/// A record view (search, discover, library, addons, settings, versions,
/// import, pairing): the core pushes whole snapshots and the last one wins.
///
/// The subscription belongs to the model, not to the view's `.task`: SwiftUI
/// cancels a task when another screen is pushed on top, which would close the
/// view (and lose its state) while the screen is still in the stack. It runs
/// from `start()` until the model is released.
@MainActor @Observable
final class LiveRecord<Snapshot: Decodable & Sendable> {
    let view: String
    private(set) var snapshot: Snapshot?
    /// A failed open, command or op, in words.
    private(set) var failure: String?
    private(set) var sub: UInt64?

    @ObservationIgnored let core: CoreClient
    @ObservationIgnored private let params: [String: any Sendable]
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var generation = -1

    init(_ view: String, params: [String: any Sendable] = [:], core: CoreClient) {
        self.view = view
        self.params = params
        self.core = core
    }

    deinit {
        task?.cancel()
    }

    /// The owning view calls `start()` again when this changes (a reopened core).
    var taskId: Int { core.generation }

    /// Opens the view once per core generation; calling it again is free.
    func start() {
        guard task == nil || generation != core.generation else { return }
        task?.cancel()
        generation = core.generation
        let (core, view, params) = (core, view, params)
        // Holds the model only while applying a batch, so releasing it ends the loop.
        task = Task { [weak self] in
            do {
                let opened = try await core.open(view, params: params, as: (Snapshot.self, Snapshot.self))
                self?.sub = opened.sub
                self?.failure = nil
                for await event in opened.events {
                    guard let self else { return }
                    switch event {
                    case .snapshot(let value): snapshot = value
                    case .updates(let values): if let last = values.last { snapshot = last }
                    }
                }
            } catch is CancellationError {
            } catch {
                self?.failure = error.localizedDescription
            }
            self?.sub = nil
        }
    }

    /// Sends a command to this view; a failure shows in `failure`.
    func command(_ fields: [String: any Sendable]) {
        guard let sub else { return }
        Task {
            do {
                try await core.command(sub, fields)
            } catch {
                failure = error.localizedDescription
            }
        }
    }

    /// Runs a protocol op on the user's behalf; a failure shows in `failure`.
    /// Returns whether it worked.
    @discardableResult
    func perform(_ op: String, _ fields: [String: any Sendable]) async -> Bool {
        do {
            try await core.perform(op, fields)
            failure = nil
            return true
        } catch {
            failure = error.localizedDescription
            return false
        }
    }
}
