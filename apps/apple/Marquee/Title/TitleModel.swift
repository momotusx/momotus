import Foundation
import Observation

/// Live title page: header (details, seasons, play target) plus the episodes
/// of the selected season.
@MainActor @Observable
final class TitleModel {
    let route: TitleRoute
    private(set) var header: TitleHeader?
    private(set) var episodes: [Episode] = []
    /// A failed open or command, in words.
    private(set) var failure: String?

    @ObservationIgnored private let core: CoreClient
    @ObservationIgnored private var sub: UInt64?

    init(route: TitleRoute, core: CoreClient) {
        self.route = route
        self.core = core
    }

    /// Restart key for the view's task: a retry or a reopened core.
    private(set) var retries = 0
    var taskId: [Int] { [retries, core.generation] }

    func retry() {
        retries += 1
    }

    func run() async {
        let opened = ContinuousClock.now
        defer { sub = nil }
        do {
            let view = try await core.open(
                "title", params: ["kind": route.kind, "id": route.id], as: (TitleSnapshot.self, TitleUpdate.self))
            sub = view.sub
            failure = nil
            var timed = false
            for await event in view.events {
                apply(event)
                if !timed, header?.details != nil {
                    timed = true
                    let ms = (ContinuousClock.now - opened) / .milliseconds(1)
                    coreLog.notice("metric title_details_ms=\(Int(ms), privacy: .public) id=\(self.route.id, privacy: .public)")
                }
            }
        } catch is CancellationError {
        } catch {
            failure = error.localizedDescription
        }
    }

    func selectSeason(_ season: Int) {
        send(["type": "selectSeason", "season": season])
    }

    func setInLibrary(_ value: Bool) {
        send(["type": "setInLibrary", "value": value])
    }

    func setWatched(_ videoId: String, _ value: Bool) {
        send(["type": "setWatched", "videoId": videoId, "value": value])
    }

    private func send(_ command: [String: any Sendable]) {
        guard let sub else { return }
        Task {
            do {
                try await core.command(sub, command)
            } catch {
                failure = error.localizedDescription
            }
        }
    }

    private func apply(_ event: ViewEvent<TitleSnapshot, TitleUpdate>) {
        switch event {
        case .snapshot(let snapshot):
            header = snapshot.header
            episodes = snapshot.episodes
        case .updates(let updates):
            var nextHeader = header
            var nextEpisodes = episodes
            for update in updates {
                switch update {
                case .header(let value): nextHeader = value
                case .episodes(let diffs): nextEpisodes = applyDiffs(nextEpisodes, diffs)
                }
            }
            if nextHeader != header { header = nextHeader }
            if nextEpisodes != episodes { episodes = nextEpisodes }
        }
    }
}
