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
    /// One more for each snapshot or failure that lands: what came in after a moment (a retry) is newer.
    @ObservationIgnored private(set) var landed = 0

    @ObservationIgnored let core: CoreClient
    @ObservationIgnored private let params: [String: any Sendable]
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var generation = -1
    /// Counts opens: a run that was replaced (a retry, a new core) touches nothing.
    @ObservationIgnored private var runs = 0
    /// Sees every snapshot as it lands, whether or not a view shows it (pairing stores its join here).
    @ObservationIgnored var onSnapshot: ((Snapshot) -> Void)?

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

    /// Opens the view again after it failed to open.
    func retry() {
        task?.cancel()
        task = nil
        start()
    }

    /// Opens the view once per core generation; calling it again is free.
    func start() {
        guard task == nil || generation != core.generation else { return }
        task?.cancel()
        generation = core.generation
        runs += 1
        let (core, view, params, mine, run) = (core, view, params, core.generation, runs)
        // Holds the model only while applying a batch, so releasing it ends the loop.
        task = Task { [weak self] in
            do {
                let opened = try await core.open(view, params: params, as: (Snapshot.self, Snapshot.self))
                do {
                    // Held only here and per batch: releasing the model ends the loop.
                    guard let self, run == runs, mine == core.generation else { return }
                    sub = opened.sub
                    failure = nil
                }
                for await event in opened.events {
                    // An event a stopped core left behind is never applied as the new one's.
                    guard let self, run == runs, mine == core.generation else { return }
                    switch event {
                    case .snapshot(let value): snapshot = value
                    case .updates(let values): if let last = values.last { snapshot = last }
                    }
                    landed += 1
                    if let snapshot { onSnapshot?(snapshot) }
                }
            } catch is CancellationError {
            } catch {
                if run == self?.runs, mine == core.generation {
                    self?.failure = error.localizedDescription
                    self?.landed += 1
                }
            }
            if run == self?.runs { self?.sub = nil }
        }
    }

    /// Sends a command to this view at once, so commands reach the core in the order they are sent
    /// (Search's `remember` before the `setQuery` that clears the query) and none waits on another;
    /// a failure shows in `failure`.
    func command(_ fields: [String: any Sendable]) {
        // A number from a core that was replaced could name another view on the new one.
        guard let sub, generation == core.generation else { return }
        core.command(sub, fields) { [weak self] error in self?.failure = error.localizedDescription }
    }

    /// Runs a protocol op at once, in call order with every other sent after it (a setting changed
    /// twice lands as last changed); a failure shows in `failure`, an answer clears it.
    func performNow(_ op: String, _ fields: [String: any Sendable]) {
        core.sendNow(op, fields) { [weak self] error in self?.failure = error?.localizedDescription }
    }

    /// Runs a protocol op on the user's behalf; a failure shows in `failure`.
    /// Returns whether it worked.
    @discardableResult
    func perform(_ op: String, _ fields: [String: any Sendable]) async -> Bool {
        do {
            try await core.perform(op, fields)
            failure = nil
            return true
        } catch is CancellationError {
            return false
        } catch {
            failure = error.localizedDescription
            return false
        }
    }
}
