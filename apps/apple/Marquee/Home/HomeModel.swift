import Foundation
import Observation

/// Live Home view state. Rows keep the positions the core gave them at open.
@MainActor @Observable
final class HomeModel {
    enum Phase: Equatable {
        case opening
        case ready
        case failed(String)
    }

    private(set) var phase = Phase.opening
    private(set) var header = HomeHeader(heroes: [])
    private(set) var rows: [HomeRow] = []
    /// Bumping this restarts `run()` through the view's `.task(id:)`.
    private(set) var generation = 0

    /// First run and the "New recommended addons" card.
    let offer: DefaultsOffer
    /// Remove from Continue Watching, with its Undo.
    let removal: ContinueRemoval
    @ObservationIgnored private let core: CoreClient
    /// The open Home view, for its commands.
    @ObservationIgnored private var sub: UInt64?

    init(core: CoreClient) {
        self.core = core
        offer = DefaultsOffer(core: core)
        removal = ContinueRemoval { try await core.perform("removeFromContinue", ["itemId": $0.id]) }
    }

    func reload() {
        generation += 1
    }

    /// One failed row's Try again: only its catalog is asked again (the row
    /// turns loading, then answers or fails again), the rest of Home and the
    /// focus stay. Without an open view, or if the core refuses, all of Home
    /// opens again.
    func retry(_ row: HomeRow) {
        guard let sub else { return reload() }
        Task {
            do {
                try await core.command(sub, row.retryCommand)
            } catch {
                reload()
            }
        }
    }

    /// Restart key for the view's task: a retry or a reopened core.
    var taskId: [Int] { [generation, core.generation] }

    /// Opens the Home view and applies its batches until the task is cancelled.
    func run() async {
        do {
            let view = try await core.open("home", as: (HomeSnapshot.self, HomeUpdate.self))
            sub = view.sub
            for await event in view.events {
                apply(event)
            }
        } catch is CancellationError {
        } catch {
            phase = .failed(error.localizedDescription)
        }
    }

    /// No addon with catalogs: Home offers setup instead of rows (the first run).
    var needsSetup: Bool {
        phase == .ready && !rows.contains { $0.rowId != "continue-watching" }
    }

    /// Nothing answered: one "could not load" with one Try again, no hero.
    var allFailed: Bool {
        phase == .ready && header.hero == nil && rows.allFailed
    }

    /// Applies one core batch with a single assignment per property.
    private func apply(_ event: ViewEvent<HomeSnapshot, HomeUpdate>) {
        switch event {
        case .snapshot(let snapshot):
            header = snapshot.header
            rows = snapshot.rows
            phase = .ready
        case .updates(let updates):
            var nextHeader = header
            var nextRows = rows
            for update in updates {
                switch update {
                case .header(let value): nextHeader = value
                case .rows(let diffs): nextRows = applyDiffs(nextRows, diffs)
                }
            }
            if nextHeader != header { header = nextHeader }
            if nextRows != rows { rows = nextRows }
        }
        logLaunchMetrics()
    }

    @ObservationIgnored private var logged: Set<String> = []

    /// Cold start numbers, from process start: Home's first snapshot and first art.
    private func logLaunchMetrics() {
        if logged.insert("home_snapshot").inserted {
            coreLog.notice("metric home_snapshot_s=\(Launch.elapsed(), format: .fixed(precision: 3), privacy: .public)")
        }
        if rows.contains(where: { !$0.tiles.isEmpty }), logged.insert("home_first_tiles").inserted {
            coreLog.notice("metric home_first_tiles_s=\(Launch.elapsed(), format: .fixed(precision: 3), privacy: .public)")
        }
    }
}
