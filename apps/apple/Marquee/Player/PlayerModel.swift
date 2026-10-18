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
        // The Lock Screen extrapolates the playhead from the rate: it hears every change at once.
        didSet { player.defaultRate = rate; if playing { player.rate = rate }; publishNowPlaying() }
    }

    /// The Versions view this playback came from; held so the core keeps it.
    private(set) var versions: VersionsModel?
    /// The user dismissed Up Next for this episode.
    private(set) var nextCancelled = false
    /// Up Next could not find a version of the next episode, in words.
    private(set) var nextFailure: String?
    /// The next episode's Versions, kept when nothing played so the end card can open them.
    private(set) var nextVersions: VersionsModel?
    private(set) var startingNext = false
    /// Where Skip credits was pressed: Up Next shows from there, and seeking
    /// back before it hides the card again.
    private(set) var skippedAt: Int64?
    /// The file's audio tracks by name, and the index of the one playing.
    private(set) var audioTracks: [String] = []
    private(set) var audioTrack: Int?
    @ObservationIgnored private var audioGroup: AVMediaSelectionGroup?

    @ObservationIgnored private let core: CoreClient
    @ObservationIgnored private let prefs: LiveRecord<Prefs>
    @ObservationIgnored private var sub: UInt64?
    @ObservationIgnored private var loadedURL: String?
    /// The core's attempt number of the source that plays, named in failOver so
    /// the core ignores a late error, even from another source of the same key.
    @ObservationIgnored private var loadedAttempt: Int64?
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
    /// Version keys whose duration the core has heard: it looks up skip
    /// segments once per version, and a failover is a new key.
    @ObservationIgnored private var durationSent: Set<String> = []
    /// The Lock Screen and Control Center side of this playback.
    @ObservationIgnored private let nowPlaying = NowPlaying()
    @ObservationIgnored private var publishedDuration: Double = 0

    init(route: PlayRoute, versions: VersionsModel?, core: CoreClient) {
        self.route = route
        self.versions = versions
        self.core = core
        prefs = LiveRecord("settings", core: core)
        // A video pauses in the background by default; the sound goes on with the screen locked.
        player.audiovisualBackgroundPlaybackPolicy = .continuesIfPossible
        observePlayer()
    }

    var taskId: [String] { [route.key, "\(route.versionsSub)", "\(core.generation)"] }

    var next: NextEpisode? { playback?.next }

    /// The card shows near the end of an episode that has a next one; at the
    /// end the end card takes over.
    var showNext: Bool {
        guard !nextCancelled, !showEndCard, let playback else { return false }
        return playback.offersNext(
            positionMs: Int64(position * 1000), durationMs: Int64(duration * 1000), ended: ended, skippedAt: skippedAt)
    }

    /// Never a black screen: once the item ends, or the next episode has no
    /// version, a card offers the way on.
    var showEndCard: Bool { ended || nextFailure != nil }

    func selectAudio(_ index: Int) {
        guard let group = audioGroup, group.options.indices.contains(index) else { return }
        player.currentItem?.select(group.options[index], in: group)
        audioTrack = index
    }

    /// Reads the audio tracks once the file is open.
    private func loadAudio(_ item: AVPlayerItem, generation: Int) {
        (audioTracks, audioTrack, audioGroup) = ([], nil, nil)
        let asset = item.asset
        Task { [weak self] in
            guard let group = try? await asset.loadMediaSelectionGroup(for: .audible),
                  let self, generation == self.generation else { return }
            audioGroup = group
            // A track tagged "und" (undetermined) or not at all would read "Unknown language".
            audioTracks = group.options.enumerated().map { index, option in
                let tag = option.extendedLanguageTag ?? option.locale?.identifier ?? ""
                if !tag.isEmpty, !tag.hasPrefix("und") { return option.displayName }
                return group.options.count == 1 ? "Default" : "Track \(index + 1)"
            }
            audioTrack = item.currentMediaSelection.selectedMediaOption(in: group).flatMap { group.options.firstIndex(of: $0) }
        }
    }

    /// Plays this one again from the start.
    func playAgain() {
        (nextFailure, nextVersions, nextCancelled, skippedAt) = (nil, nil, false, nil)
        seek(to: 0)
        player.play()
    }

    /// The segment under the playhead, while Up Next is not on screen.
    var segment: Segment? {
        showNext ? nil : playback?.segment(at: Int64(position * 1000))
    }

    /// Manual only: the viewer presses Skip, the player never jumps by itself.
    func skip(_ segment: Segment) {
        if segment.endsFile(durationMs: Int64(duration * 1000)) {
            if next != nil { skippedAt = segment.startMs } else { seek(to: max(0, duration - 0.05)) }
        } else {
            seek(to: Double(segment.endMs) / 1000)
        }
    }

    /// The user's choice in Settings: start the next episode on its own.
    var autoplayNext: Bool { prefs.snapshot?.autoplayNext ?? true }

    /// Whole seconds until the next episode starts on its own.
    var countdown: Int { max(0, Int((duration - position).rounded(.up))) }

    func cancelNext() {
        nextCancelled = true
    }

    /// Opens Versions for the next episode with this stream's binge group and
    /// plays its pick once it settles; if there is none, says so.
    func playNext() async {
        guard let next, !startingNext else { return }
        startingNext = true
        defer { startingNext = false }
        report()
        let current = route.target
        let target = PlayTargetInfo(
            kind: current.kind, itemId: current.itemId, videoId: next.videoId, title: current.title,
            poster: current.poster, resumeMs: nil, season: next.season, episode: next.episode)
        let model = VersionsModel(target: target, core: core, bingeGroup: playback?.bingeGroup)
        guard let pick = await model.settledPick() else {
            player.pause()
            nextVersions = model
            let words = model.snapshot.flatMap { $0.done ? VersionWords.none($0) : nil }
            nextFailure = words ?? "No version answered yet."
            return
        }
        // The new view must exist before the old one goes: the core copies candidates at subscribe.
        versions = model
        route = pick
        (loadedURL, rendered, firstFrameMs, playback, ended, nextCancelled, nextFailure, nextVersions) = (nil, false, nil, nil, false, false, nil, nil)
        (position, duration, skippedAt) = (0, 0, nil)
        selectSubtitle(nil)
    }

    func run() async {
        runs += 1
        let run = runs
        durationSent = []
        prefs.start()
        nowPlaying.stop()
        nowPlaying.start(self)
        Task { await nowPlaying.loadArtwork(route.target.poster) }
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
            DispatchQueue.main.async { MainActor.assumeIsolated { self?.report(); self?.publishNowPlaying() } }
        }
    }

    func skip(_ seconds: Double) { seek(to: position + seconds) }

    /// The video surface drew its first frame.
    func firstFrame() {
        guard !rendered else { return }
        rendered = true
        let ms = Int((ContinuousClock.now - route.tapped) / .milliseconds(1))
        firstFrameMs = ms
        // The key's kind only ("url", "bt", "rel"): a "url:" key is the stream link, which can carry an addon's key.
        let kind = String(route.key.prefix { $0 != ":" })
        coreLog.notice("metric first_frame_ms=\(ms, privacy: .public) key=\(kind, privacy: .public)")
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
            var start = loadedURL == nil ? Double(snapshot.resumeMs ?? 0) / 1000 : position
            #if DEBUG
            if loadedURL == nil, let at = DebugHooks.takeSeek() { start = at }
            #endif
            load(url, key: snapshot.key ?? route.key, attempt: snapshot.attempt, at: start)
        case "failed":
            player.pause()
            notice = nil
        default:
            break
        }
    }

    /// `key`: the version this URL plays, for its `duration`; `attempt`: the
    /// core's number for this source, for `failOver`.
    private func load(_ url: String, key: String, attempt: Int64?, at start: Double) {
        guard let address = URL(string: url) else { return }
        generation += 1
        let generation = generation
        loadedURL = url
        loadedAttempt = attempt
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
        // The core looks up skip segments once it knows this version's length.
        item.publisher(for: \.duration)
            .receive(on: DispatchQueue.main)
            .sink { [weak self] time in
                MainActor.assumeIsolated {
                    guard let self, time.isNumeric, time.seconds > 0, generation == self.generation, let sub = self.sub,
                          self.durationSent.insert(key).inserted else { return }
                    let ms = Int(time.seconds * 1000)
                    Task { try? await self.core.command(sub, ["type": "duration", "key": key, "durationMs": ms]) }
                }
            }
            .store(in: &itemBag)
        player.replaceCurrentItem(with: item)
        loadAudio(item, generation: generation)
        if start > 1 { player.seek(to: CMTime(seconds: start, preferredTimescale: 600), toleranceBefore: .zero, toleranceAfter: .zero) }
        NowPlaying.activate()
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
        var command: [String: any Sendable] = ["type": "failOver", "error": reason]
        if let loadedAttempt { command["attempt"] = loadedAttempt }
        Task { [command] in try? await core.command(sub, command) }
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
                    self?.publishNowPlaying()
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
        if duration != publishedDuration { publishNowPlaying() }
        if playing, ContinuousClock.now - lastReport > .seconds(5) { report() }
    }

    /// Saves progress in the core. Nothing before the first frame: a position
    /// of zero then would wipe the resume point.
    private func report() {
        guard rendered, let sub, duration > 0, position > 0 else { return }
        lastReport = .now
        core.commandNow(sub, ["type": "progress", "positionMs": Int(position * 1000), "durationMs": Int(duration * 1000)])
    }

    /// The Lock Screen's title, episode, length, playhead and speed (0 while paused).
    private func publishNowPlaying() {
        let target = route.target
        publishedDuration = duration
        let episode = target.season.flatMap { season in target.episode.map { "S\(season):E\($0)" } }
        nowPlaying.update(title: target.title, episode: episode, duration: duration, position: position, rate: playing ? rate : 0)
    }

    /// Saves the position before the view goes away (Back, close).
    func close() {
        report()
        player.pause()
    }

    private func stop() {
        report()
        nowPlaying.stop()
        NowPlaying.deactivate()
        itemBag = []
        player.pause()
        player.replaceCurrentItem(with: nil)
        sub = nil
    }
}
