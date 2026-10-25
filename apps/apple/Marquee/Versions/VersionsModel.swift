import Foundation
import Observation

/// Live Versions view for one target. Whoever holds the model keeps the view
/// open: the title page while it is in the stack, and the player while it
/// plays, since the core copies candidates when playback subscribes.
@MainActor @Observable
final class VersionsModel {
    let target: PlayTargetInfo
    @ObservationIgnored private let record: LiveRecord<VersionsSnapshot>

    /// `purpose` "download": the versions to keep on this device (plans/offline-downloads.md 3.2).
    let purpose: String?

    /// `bingeGroup`: the playing stream's, so the next episode keeps its release line.
    init(target: PlayTargetInfo, core: CoreClient, bingeGroup: String? = nil, purpose: String? = nil) {
        self.target = target
        self.purpose = purpose
        var params = DeviceCaps.params()
        if let bingeGroup { params["bingeGroup"] = bingeGroup }
        if let purpose {
            params["purpose"] = purpose
            params["itemId"] = target.itemId
        }
        params["kind"] = target.kind
        params["videoId"] = target.videoId
        params["title"] = target.title
        if let season = target.season { params["season"] = season }
        if let episode = target.episode { params["episode"] = episode }
        params.merge(StreamChoice.versionsFacts(
            runtime: target.runtime, releaseInfo: target.releaseInfo, released: target.released,
            episodesInSeason: target.episodesInSeason)) { _, fact in fact }
        record = LiveRecord("versions", params: params, core: core)
        // Once per settled list, the listed share's figures (plans/apple-second-engine.md section 4).
        var said = false
        let id = target.videoId
        record.onSnapshot = { snapshot in
            guard snapshot.done, !said else { return }
            said = true
            let listed = snapshot.rows.count + (snapshot.recommended == nil ? 0 : 1)
            let hidden = (snapshot.hidden?.byReason ?? []).map { "\($0.reason):\($0.count)" }.joined(separator: ",")
            // The title watched stays out of a shipped build's log; walk builds carry DEBUG and keep it.
            #if DEBUG
            coreLog.notice("metric versions listed=\(listed, privacy: .public) hidden=\(hidden, privacy: .public) id=\(id, privacy: .public)")
            #else
            coreLog.notice("metric versions listed=\(listed, privacy: .public) hidden=\(hidden, privacy: .public) id=\(id, privacy: .private)")
            #endif
        }
    }

    var snapshot: VersionsSnapshot? {
        #if DEBUG
        if let fixture = DebugHooks.versionsFixture { return fixture }
        #endif
        return PickWait.fresh(landed: record.landed, retriedAt: retriedAt) ? record.snapshot : nil
    }
    /// The view's open, a command or an op failed (since the last Try again).
    var failure: String? { PickWait.fresh(landed: record.landed, retriedAt: retriedAt) ? record.failure : nil }
    /// What had landed when Try again asked the view again: anything up to it is stale.
    private var retriedAt: Int?
    var sub: UInt64? { record.sub }

    func start() {
        record.start()
    }

    /// Try again when no source could be reached: the view opens again.
    func retry() {
        retriedAt = record.landed
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
    /// within `limit`; no `limit` waits for the core (the view's own
    /// deadlines end its asking), never an app clock.
    func settledPick(tapped: ContinuousClock.Instant = .now, limit: Duration? = .seconds(10)) async -> PlayRoute? {
        start()
        let deadline = limit.map { ContinuousClock.now + $0 }
        var firstPick: ContinuousClock.Instant?
        while deadline.map({ ContinuousClock.now < $0 }) ?? true, !Task.isCancelled {
            if snapshot?.recommended != nil, firstPick == nil { firstPick = .now }
            if PickWait.ends(snapshot: snapshot, failed: failure != nil, settled: firstPick.map { ContinuousClock.now - $0 >= .seconds(3) } ?? false) { break }
            try? await Task.sleep(for: .milliseconds(100))
        }
        return PickWait.key(snapshot).flatMap { play($0, tapped: tapped) }
    }
}
