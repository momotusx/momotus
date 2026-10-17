import Foundation
import Observation

/// Live Home view state. Rows keep the positions the core gave them at open.
@MainActor @Observable
final class HomeModel {
    enum Phase: Equatable {
        case opening
        case settingUp
        case ready
        case failed(String)
    }

    static let starterAddon = "https://v3-cinemeta.strem.io/manifest.json"
    private static let starterKey = "starterAddonInstalled"

    private(set) var phase = Phase.opening
    private(set) var header = HomeHeader(hero: nil)
    private(set) var rows: [HomeRow] = []
    /// Bumping this restarts `run()` through the view's `.task(id:)`.
    private(set) var generation = 0

    @ObservationIgnored private let core: CoreClient

    init(core: CoreClient) {
        self.core = core
    }

    func reload() {
        generation += 1
    }

    /// Restart key for the view's task: a retry or a reopened core.
    var taskId: [Int] { [generation, core.generation] }

    /// Opens the Home view and applies its batches until the task is cancelled.
    func run() async {
        do {
            let view = try await core.open("home", as: (HomeSnapshot.self, HomeUpdate.self))
            for await event in view.events {
                apply(event)
                if case .snapshot = event, needsStarterAddon {
                    await installStarterAddon()
                }
            }
        } catch is CancellationError {
        } catch {
            phase = .failed(error.localizedDescription)
        }
    }

    /// First launch with no catalogs: install Cinemeta so Home has content.
    private var needsStarterAddon: Bool {
        !UserDefaults.standard.bool(forKey: Self.starterKey)
            && !rows.contains { $0.rowId != "continue-watching" }
    }

    private func installStarterAddon() async {
        phase = .settingUp
        do {
            try await core.perform("installAddon", ["url": Self.starterAddon])
        } catch let error as CoreError where error.message.contains("already installed") {
        } catch {
            phase = .failed("Could not install Cinemeta: \(error.localizedDescription)")
            return
        }
        UserDefaults.standard.set(true, forKey: Self.starterKey)
        // Home follows installed addons on its own: the new rows arrive as updates.
        phase = .ready
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
