import AVFoundation
import Combine
import Observation

/// One playback: the core's playback view (resolve, failover, progress,
/// subtitles) driving an AVPlayer on the loopback URL it hands out.
@MainActor @Observable
final class PlayerModel {
    private(set) var route: PlayRoute
    let player = AVPlayer()
    private(set) var playback: PlaybackSnapshot?
    /// Seconds.
    private(set) var position: Double = 0
    private(set) var duration: Double = 0
    private(set) var playing = false
    private(set) var waiting = true
    private(set) var ended = false
    private(set) var firstFrameMs: Int?
    /// A local player error the core has not answered yet.
    private(set) var notice: String?
    /// The picture's size, for placing subtitles on it rather than on the screen.
    private(set) var videoSize: CGSize = .zero
    private(set) var subtitle: SubtitleTrack?
    private(set) var cues: [WebVTT.Cue] = []
    var rate: Float = 1 {
        didSet { player.defaultRate = rate; if playing { player.rate = rate } }
    }

    /// The Versions view this playback came from; held so the core keeps it.
    private(set) var versions: VersionsModel?
    /// The user dismissed Up Next for this episode.
    private(set) var nextCancelled = false
    /// Up Next could not find a version of the next episode, in words.
    private(set) var nextFailure: String?
    private(set) var startingNext = false

    @ObservationIgnored private let core: CoreClient
    @ObservationIgnored private let prefs: LiveRecord<Prefs>
    @ObservationIgnored private var sub: UInt64?
    @ObservationIgnored private var loadedURL: String?
    /// Bumped per URL so a late error from a dead source is ignored.
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var rendered = false
    @ObservationIgnored private var lastReport = ContinuousClock.now
    /// The player calls the time observer only while this token is held.
    @ObservationIgnored private var timeObserver: Any?
    @ObservationIgnored private var bag: Set<AnyCancellable> = []
    /// Observers of the current item, dropped when another one loads.
    @ObservationIgnored private var itemBag: Set<AnyCancellable> = []
    /// Counts runs: a route change starts the next run before the last one has
    /// stopped, so only the latest may tear the player down.
    @ObservationIgnored private var runs = 0

    /// Up Next appears this many seconds before the end, as on the web.
    static let nextAt: Double = 20

    init(route: PlayRoute, versions: VersionsModel?, core: CoreClient) {
        self.route = route
        self.versions = versions
        self.core = core
        prefs = LiveRecord("settings", core: core)
        observePlayer()
    }

    var taskId: [String] { [route.key, "\(route.versionsSub)", "\(core.generation)"] }

    var next: NextEpisode? { playback?.next }

    /// The card shows near the end of an episode that has a next one.
    var showNext: Bool {
        next != nil && !nextCancelled && duration > 60 && (ended || duration - position <= Self.nextAt)
    }

    /// The user's choice in Settings: start the next episode on its own.
    var autoplayNext: Bool { prefs.snapshot?.autoplayNext ?? true }

    /// Whole seconds until the next episode starts on its own.
    var countdown: Int { max(0, Int((duration - position).rounded(.up))) }

    func cancelNext() {
        nextCancelled = true
    }

    /// Opens Versions for the next episode with this stream's binge group and
    /// plays its pick; if none answers within 6 s, says so.
    func playNext() async {
        guard let next, !startingNext else { return }
        startingNext = true
        defer { startingNext = false }
        report()
        let current = route.target
        let target = PlayTargetInfo(
            kind: current.kind, itemId: current.itemId, videoId: next.videoId, title: current.title,
            poster: current.poster, resumeMs: nil)
        let model = VersionsModel(target: target, core: core, bingeGroup: playback?.bingeGroup)
        model.start()
        let deadline = ContinuousClock.now + .seconds(6)
        while ContinuousClock.now < deadline, model.snapshot?.recommended == nil, model.snapshot?.done != true {
            try? await Task.sleep(for: .milliseconds(100))
        }
        guard let pick = model.snapshot?.recommended, let versionsSub = model.sub else {
            nextFailure = "No version of S\(next.season):E\(next.episode) plays here yet. Pick one from its title page."
            return
        }
        // The new view must exist before the old one goes: the core copies candidates at subscribe.
        versions = model
        route = PlayRoute(versionsSub: versionsSub, key: pick.key, target: target, tapped: .now)
        (loadedURL, rendered, firstFrameMs, playback, ended, nextCancelled, nextFailure) = (nil, false, nil, nil, false, false, nil)
        (position, duration) = (0, 0)
        selectSubtitle(nil)
    }

    func run() async {
        runs += 1
        let run = runs
        prefs.start()
        defer { if run == runs { stop() } }
        let target = route.target
        var params: [String: any Sendable] = [
            "versionsSub": route.versionsSub, "key": route.key, "kind": target.kind,
            "itemId": target.itemId, "videoId": target.videoId, "name": target.title,
        ]
        if let poster = target.poster { params["poster"] = poster }
        if let season = target.season { params["season"] = season }
        if let episode = target.episode { params["episode"] = episode }
        do {
            let view = try await core.open("playback", params: params, as: (PlaybackSnapshot.self, PlaybackSnapshot.self))
            sub = view.sub
            for await event in view.events {
                switch event {
                case .snapshot(let value): apply(value)
                case .updates(let values): if let last = values.last { apply(last) }
                }
            }
        } catch is CancellationError {
        } catch {
            notice = error.localizedDescription
        }
    }

    /// Plays another version of the same video from where this one is.
    func switchVersion(_ key: String) {
        guard key != route.key else { return }
        report()
        route = PlayRoute(versionsSub: route.versionsSub, key: key, target: route.target, tapped: .now)
        (loadedURL, rendered, firstFrameMs, playback) = (nil, false, nil, nil)
    }

    func togglePlay() {
        if playing { player.pause(); report() } else { if ended { seek(to: 0) }; player.play() }
    }

    func seek(to seconds: Double) {
        let target = CMTime(seconds: max(0, min(seconds, duration > 0 ? duration : seconds)), preferredTimescale: 600)
        position = target.seconds
        ended = false
        player.seek(to: target, toleranceBefore: .zero, toleranceAfter: .zero) { [weak self] done in
            guard done else { return }
            DispatchQueue.main.async { MainActor.assumeIsolated { self?.report() } }
        }
    }

    func skip(_ seconds: Double) { seek(to: position + seconds) }

    /// The video surface drew its first frame.
    func firstFrame() {
        guard !rendered else { return }
        rendered = true
        let ms = Int((ContinuousClock.now - route.tapped) / .milliseconds(1))
        firstFrameMs = ms
        coreLog.notice("metric first_frame_ms=\(ms, privacy: .public) key=\(self.route.key, privacy: .public)")
        #if DEBUG
        if let at = DebugHooks.takeSeek() { seek(to: at) }
        #endif
    }

    func selectSubtitle(_ track: SubtitleTrack?) {
        subtitle = track
        cues = []
        guard let track, let url = URL(string: track.url) else { return }
        Task {
            guard let (data, _) = try? await URLSession.shared.data(from: url), subtitle == track else { return }
            cues = WebVTT.parse(String(decoding: data, as: UTF8.self))
        }
    }

    var subtitleText: String? { WebVTT.text(at: position, in: cues) }

    // MARK: core and player plumbing

    private func apply(_ snapshot: PlaybackSnapshot) {
        playback = snapshot
        switch snapshot.phase {
        case "ready":
            guard let url = snapshot.url, url != loadedURL else { return }
            if loadedURL == nil, !rendered {
                let ms = Int((ContinuousClock.now - route.tapped) / .milliseconds(1))
                let host = URL(string: url)?.host() ?? "?"
                coreLog.notice("metric playback_ready_ms=\(ms, privacy: .public) host=\(host, privacy: .public)")
            }
            // The first source starts at the saved position; a failover resumes where the last one stopped.
            let start = loadedURL == nil ? Double(snapshot.resumeMs ?? 0) / 1000 : position
            load(url, at: start)
        case "failed":
            player.pause()
            notice = nil
        default:
            break
        }
    }

    private func load(_ url: String, at start: Double) {
        guard let address = URL(string: url) else { return }
        generation += 1
        let generation = generation
        loadedURL = url
        notice = nil
        itemBag = []
        let item = AVPlayerItem(url: address)
        item.publisher(for: \.status)
            .receive(on: DispatchQueue.main)
            .sink { [weak self] status in
                MainActor.assumeIsolated {
                    guard let self, status == .failed, generation == self.generation else { return }
                    self.sourceFailed(item.error)
                }
            }
            .store(in: &itemBag)
        item.publisher(for: \.presentationSize)
            .receive(on: DispatchQueue.main)
            .sink { [weak self] size in MainActor.assumeIsolated { self?.videoSize = size } }
            .store(in: &itemBag)
        player.replaceCurrentItem(with: item)
        if start > 1 { player.seek(to: CMTime(seconds: start, preferredTimescale: 600), toleranceBefore: .zero, toleranceAfter: .zero) }
        player.play()
    }

    /// The player cannot play this source: stop it, then ask the core for the next.
    private func sourceFailed(_ error: Error?) {
        let reason = error?.localizedDescription ?? "the player could not open this source"
        coreLog.error("source failed: \(reason, privacy: .public)")
        notice = "This source failed: \(reason). Trying the next one."
        generation += 1
        player.pause()
        player.replaceCurrentItem(with: nil)
        loadedURL = nil
        guard let sub else { return }
        Task { try? await core.command(sub, ["type": "failOver", "error": reason]) }
    }

    /// For the model's lifetime: the player goes with it, so nothing removes these.
    private func observePlayer() {
        timeObserver = player.addPeriodicTimeObserver(forInterval: CMTime(value: 1, timescale: 4), queue: .main) { [weak self] time in
            MainActor.assumeIsolated { self?.tick(time) }
        }
        player.publisher(for: \.timeControlStatus)
            .receive(on: DispatchQueue.main)
            .sink { [weak self] status in
                MainActor.assumeIsolated {
                    self?.playing = status == .playing
                    self?.waiting = status == .waitingToPlayAtSpecifiedRate
                }
            }
            .store(in: &bag)
        NotificationCenter.default.publisher(for: AVPlayerItem.didPlayToEndTimeNotification)
            .receive(on: DispatchQueue.main)
            .sink { [weak self] note in
                MainActor.assumeIsolated {
                    guard let self, (note.object as? AVPlayerItem) === self.player.currentItem else { return }
                    self.ended = true
                    self.report()
                    if self.next != nil, self.autoplayNext, !self.nextCancelled {
                        Task { await self.playNext() }
                    }
                }
            }
            .store(in: &bag)
    }

    private func tick(_ time: CMTime) {
        guard time.isNumeric else { return }
        position = time.seconds
        if let total = player.currentItem?.duration, total.isNumeric { duration = total.seconds }
        if playing, ContinuousClock.now - lastReport > .seconds(5) { report() }
    }

    /// Saves progress in the core. Nothing before the first frame: a position
    /// of zero then would wipe the resume point.
    private func report() {
        guard rendered, let sub, duration > 0, position > 0 else { return }
        lastReport = .now
        core.commandNow(sub, ["type": "progress", "positionMs": Int(position * 1000), "durationMs": Int(duration * 1000)])
    }

    /// Saves the position before the view goes away (Back, close).
    func close() {
        report()
        player.pause()
    }

    private func stop() {
        report()
        itemBag = []
        player.pause()
        player.replaceCurrentItem(with: nil)
        sub = nil
    }
}
