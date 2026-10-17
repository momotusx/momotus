import Foundation
import Observation

/// What to find versions for: a movie, or one episode of a series.
struct PlayTargetInfo: Hashable, Sendable {
    let kind: String
    /// The movie or series id.
    let itemId: String
    let videoId: String
    /// The movie or series name, never an episode title: ranking uses it to
    /// hide sources for other titles.
    let title: String
    let poster: String?
    let season: Int?
    let episode: Int?
    let resumeMs: Int64?

    init(kind: String, itemId: String, videoId: String, title: String, poster: String?, resumeMs: Int64?) {
        (self.kind, self.itemId, self.videoId, self.title, self.poster, self.resumeMs) =
            (kind, itemId, videoId, title, poster, resumeMs)
        // Episode ids are "<series>:<season>:<episode>".
        let parts = videoId.split(separator: ":")
        (season, episode) = parts.count == 3 ? (Int(parts[1]), Int(parts[2])) : (nil, nil)
    }
}

/// Live Versions view for one target. It stays open while a playback view
/// made from it exists, since the core copies candidates at subscribe time.
@MainActor @Observable
final class VersionsModel {
    let target: PlayTargetInfo
    private(set) var snapshot: VersionsSnapshot?
    private(set) var failure: String?
    private(set) var sub: UInt64?

    @ObservationIgnored private let core: CoreClient
    @ObservationIgnored private var frozen = false

    init(target: PlayTargetInfo, core: CoreClient) {
        self.target = target
        self.core = core
    }

    var taskId: Int { core.generation }

    func run() async {
        var params = DeviceCaps.params()
        params["kind"] = target.kind
        params["videoId"] = target.videoId
        params["title"] = target.title
        if let season = target.season { params["season"] = season }
        if let episode = target.episode { params["episode"] = episode }
        defer { sub = nil }
        do {
            let view = try await core.open("versions", params: params, as: (VersionsSnapshot.self, VersionsSnapshot.self))
            sub = view.sub
            frozen = false
            for await event in view.events {
                switch event {
                case .snapshot(let value): snapshot = value
                case .updates(let values): if let last = values.last { snapshot = last }
                }
            }
        } catch is CancellationError {
        } catch {
            failure = error.localizedDescription
        }
    }

    /// Pins the recommended pick so late results never swap it under the user.
    func freeze() {
        guard let sub, !frozen else { return }
        frozen = true
        Task { try? await core.command(sub, ["type": "freeze"]) }
    }

    /// What the player needs to start `key` (the recommended pick by default).
    func play(_ key: String? = nil) -> PlayRoute? {
        guard let sub, let key = key ?? snapshot?.recommended?.key else { return nil }
        return PlayRoute(versionsSub: sub, key: key, target: target, tapped: .now)
    }
}

/// Opens a playback view for one version.
struct PlayRoute: Hashable, Identifiable {
    let versionsSub: UInt64
    let key: String
    let target: PlayTargetInfo
    /// When the user asked to play, for time to first frame.
    let tapped: ContinuousClock.Instant

    var id: String { "\(versionsSub):\(key)" }
}
