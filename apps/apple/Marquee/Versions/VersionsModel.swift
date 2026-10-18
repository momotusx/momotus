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

    /// `season` and `episode` when the caller knows them (the core's next
    /// episode); else read from an id shaped "<series>:<season>:<episode>".
    init(
        kind: String, itemId: String, videoId: String, title: String, poster: String?, resumeMs: Int64?,
        season: Int? = nil, episode: Int? = nil
    ) {
        (self.kind, self.itemId, self.videoId, self.title, self.poster, self.resumeMs) =
            (kind, itemId, videoId, title, poster, resumeMs)
        let parts = videoId.split(separator: ":")
        let parsed = parts.count == 3 ? (Int(parts[1]), Int(parts[2])) : (nil, nil)
        (self.season, self.episode) = (season ?? parsed.0, episode ?? parsed.1)
    }
}

/// Live Versions view for one target. Whoever holds the model keeps the view
/// open: the title page while it is in the stack, and the player while it
/// plays, since the core copies candidates when playback subscribes.
@MainActor @Observable
final class VersionsModel {
    let target: PlayTargetInfo
    @ObservationIgnored private let record: LiveRecord<VersionsSnapshot>
    @ObservationIgnored private var frozen = false

    /// `bingeGroup`: the playing stream's, so the next episode keeps its release line.
    init(target: PlayTargetInfo, core: CoreClient, bingeGroup: String? = nil) {
        self.target = target
        var params = DeviceCaps.params()
        if let bingeGroup { params["bingeGroup"] = bingeGroup }
        params["kind"] = target.kind
        params["videoId"] = target.videoId
        params["title"] = target.title
        if let season = target.season { params["season"] = season }
        if let episode = target.episode { params["episode"] = episode }
        record = LiveRecord("versions", params: params, core: core)
    }

    var snapshot: VersionsSnapshot? { record.snapshot }
    var failure: String? { record.failure }
    var sub: UInt64? { record.sub }

    func start() {
        record.start()
    }

    /// Pins the recommended pick so late results never swap it under the user.
    func freeze() {
        guard sub != nil, !frozen else { return }
        frozen = true
        record.command(["type": "freeze"])
    }

    /// What the player needs to start `key` (the recommended pick by default).
    func play(_ key: String? = nil, tapped: ContinuousClock.Instant = .now) -> PlayRoute? {
        guard let sub, let key = key ?? snapshot?.recommended?.key else { return nil }
        return PlayRoute(versionsSub: sub, key: key, target: target, tapped: tapped)
    }

    /// The pick once it settles, as the web waits for it: every addon has
    /// answered, or 3 s have passed since the first pick. Nil when there is
    /// none by then or within `limit`.
    func settledPick(tapped: ContinuousClock.Instant = .now, limit: Duration = .seconds(10)) async -> PlayRoute? {
        start()
        let deadline = ContinuousClock.now + limit
        var firstPick: ContinuousClock.Instant?
        while ContinuousClock.now < deadline, !Task.isCancelled, snapshot?.done != true {
            if snapshot?.recommended != nil {
                let first = firstPick ?? .now
                firstPick = first
                if ContinuousClock.now - first >= .seconds(3) { break }
            }
            try? await Task.sleep(for: .milliseconds(100))
        }
        return play(tapped: tapped)
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
