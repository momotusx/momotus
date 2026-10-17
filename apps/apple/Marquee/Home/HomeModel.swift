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

    /// Opens the Home view and applies its batches until the task is cancelled.
    func run() async {
        do {
            let view = try await core.open("home", as: (HomeSnapshot.self, HomeUpdate.self))
            for await event in view.events {
                apply(event)
                if case .snapshot = event, needsStarterAddon {
                    await installStarterAddon()
                    return
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
            try await core.installAddon(url: Self.starterAddon)
        } catch let error as CoreError where error.message.contains("already installed") {
        } catch {
            phase = .failed("Could not install Cinemeta: \(error.localizedDescription)")
            return
        }
        UserDefaults.standard.set(true, forKey: Self.starterKey)
        // Rows are fixed when a Home view opens, so reopen to get the new ones.
        reload()
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
    }
}
