import Foundation
import Observation

/// Live Versions view for one target. Whoever holds the model keeps the view
/// open: the title page while it is in the stack, and the player while it
/// plays, since the core copies candidates when playback subscribes.
@MainActor @Observable
final class VersionsModel {
    let target: PlayTargetInfo
    @ObservationIgnored private let record: LiveRecord<VersionsSnapshot>

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
        params.merge(StreamChoice.versionsFacts(
            runtime: target.runtime, releaseInfo: target.releaseInfo, released: target.released,
            episodesInSeason: target.episodesInSeason)) { _, fact in fact }
        record = LiveRecord("versions", params: params, core: core)
    }

    var snapshot: VersionsSnapshot? {
        #if DEBUG
        if let fixture = DebugHooks.versionsFixture { return fixture }
        #endif
        return record.snapshot
    }
    var failure: String? { record.failure }
    var sub: UInt64? { record.sub }

    func start() {
        record.start()
    }

    /// Try again when no source could be reached: the view opens again.
    func retry() {
        record.retry()
    }

    /// Pins the recommended pick so late results never swap it under the user.
    func freeze() {
        guard StreamChoice.shouldFreeze(open: sub != nil, frozen: snapshot?.frozen) else { return }
        record.command(["type": "freeze"])
    }

    /// What the player needs to start `key` (the recommended pick by default).
    func play(_ key: String? = nil, tapped: ContinuousClock.Instant = .now) -> PlayRoute? {
        guard let sub, let key = key ?? snapshot?.recommended?.key else { return nil }
        return PlayRoute(versionsSub: sub, key: key, target: target, tapped: tapped)
    }

    /// Play pressed while versions are still found: the player opens at once and
    /// starts the pick itself (`PlayRoute.waitsForPick`).
    func pending(tapped: ContinuousClock.Instant = .now, fromStart: Bool = false) -> PlayRoute {
        PlayRoute(versionsSub: sub ?? 0, key: "", target: target, tapped: tapped, fromStart: fromStart)
    }

    /// The pick once it settles, as the web waits for it: every addon has
    /// answered, or 3 s have passed since the first pick; with no source in
    /// reach, the one kept on this device. Nil when there is none by then or
    /// within `limit`.
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
        let kept = snapshot?.empty.flatMap { $0.kind == "unreachable" ? $0.key : nil }
        return play(kept, tapped: tapped)
    }
}
