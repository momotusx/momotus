import Foundation
import Observation

/// Live title page: header (details, seasons, play target) plus the episodes
/// of the selected season.
@MainActor @Observable
final class TitleModel {
    let route: TitleRoute
    private(set) var header: TitleHeader?
    private(set) var episodes: [Episode] = []
    /// "More like this" (Trakt's related titles), empty until it lands.
    private(set) var similar: [Tile] = []
    /// A failed open or command, in words.
    private(set) var failure: String?

    @ObservationIgnored private let core: CoreClient
    @ObservationIgnored private var sub: UInt64?
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var generation = -1
    /// Whether the page shows, told to the core once the view is open (`titleVisible`).
    @ObservationIgnored private var visible = false

    init(route: TitleRoute, core: CoreClient) {
        self.route = route
        self.core = core
    }

    deinit {
        task?.cancel()
    }

    /// Keeps the title view open while the page is in the navigation stack
    /// (not only while it is on screen), so Versions and the player pushed on
    /// top do not close it. Calling it again is free.
    func start() {
        guard task == nil || generation != core.generation else { return }
        open()
    }

    func retry() {
        // The same words left up would read as an ignored press, and a repeat failure would not be said again.
        failure = nil
        header = headerAfterRetry(header)
        open()
    }

    private func open() {
        task?.cancel()
        generation = core.generation
        let (core, kind, id) = (core, route.kind, route.id)
        let opened = ContinuousClock.now
        var season = route.season
        // Holds the model only while applying a batch, so releasing it ends the loop.
        task = Task { [weak self] in
            do {
                let view = try await core.open("title", params: ["kind": kind, "id": id], as: (TitleSnapshot.self, TitleUpdate.self))
                self?.sub = view.sub
                self?.failure = nil
                if self?.visible == true { self?.send(["type": "titleVisible", "visible": true]) }
                var timed = false
                for await event in view.events {
                    guard let self else { return }
                    apply(event)
                    // The route's season, once the header lists it.
                    if let wanted = season, header?.seasons.contains(wanted) == true {
                        season = nil
                        if header?.selectedSeason != wanted { selectSeason(wanted) }
                    }
                    if !timed, header?.details != nil {
                        timed = true
                        let ms = Int((ContinuousClock.now - opened) / .milliseconds(1))
                        coreLog.notice("metric title_details_ms=\(ms, privacy: .public) id=\(id, privacy: .public)")
                    }
                }
            } catch is CancellationError {
            } catch {
                self?.failure = ErrorWords.of(error)
            }
            self?.sub = nil
        }
    }

    /// The page shows or stops showing: the core asks TMDB and Trakt only after a second of showing.
    func setVisible(_ value: Bool) {
        guard value != visible else { return }
        visible = value
        send(["type": "titleVisible", "visible": value])
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
        // At once, in call order: a quick Library on then off lands as off.
        core.command(sub, command) { [weak self] error in self?.failure = ErrorWords.of(error) }
    }

    private func apply(_ event: ViewEvent<TitleSnapshot, TitleUpdate>) {
        switch event {
        case .snapshot(let snapshot):
            header = snapshot.header
            episodes = snapshot.episodes
            similar = snapshot.similar
        case .updates(let updates):
            var nextHeader = header
            var nextEpisodes = episodes
            for update in updates {
                switch update {
                case .header(let value): nextHeader = value
                case .episodes(let diffs): nextEpisodes = applyDiffs(nextEpisodes, diffs)
                case .similar(let tiles): if tiles != similar { similar = tiles }
                case .other: break
                }
            }
            if nextHeader != header { header = nextHeader }
            if nextEpisodes != episodes { episodes = nextEpisodes }
        }
    }
}
