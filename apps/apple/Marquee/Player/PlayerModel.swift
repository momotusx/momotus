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
    /// The file's own subtitles (a stream's WebVTT renditions, a file's text
    /// tracks) by name, and the index of the one on. AVKit draws them in the
    /// viewer's caption style; an addon's subtitle is drawn by the app instead.
    private(set) var textTracks: [String] = []
    private(set) var textTrack: Int?
    @ObservationIgnored private var textGroup: AVMediaSelectionGroup?
    @ObservationIgnored private var textOptions: [AVMediaSelectionOption] = []

    @ObservationIgnored private let core: CoreClient
    @ObservationIgnored private let prefs: LiveRecord<Prefs>
    @ObservationIgnored private var sub: UInt64?
    @ObservationIgnored private var loadedURL: String?
    /// Where a failed source stopped, so the next one continues there.
    @ObservationIgnored private var stoppedAt: Double?
    /// Where a seek in flight goes (the source's start or the viewer's), until it
    /// lands: the player may still report where it was, and a failure keeps this.
    @ObservationIgnored private var pendingStart: Double?
    /// Bumped per seek, so only the latest seek's completion hands over.
    @ObservationIgnored private var seekToken = 0
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

    /// Turns one of the file's own subtitles on, in place of an addon's.
    func selectText(_ index: Int) {
        guard let group = textGroup, textOptions.indices.contains(index) else { return }
        selectSubtitle(nil)
        player.currentItem?.select(textOptions[index], in: group)
        textTrack = index
    }

    /// Reads the audio and subtitle tracks once the file is open.
    private func loadTracks(_ item: AVPlayerItem, generation: Int) {
        (audioTracks, audioTrack, audioGroup) = ([], nil, nil)
        (textTracks, textTrack, textGroup, textOptions) = ([], nil, nil, [])
        let asset = item.asset
        Task { [weak self] in
            guard let group = try? await asset.loadMediaSelectionGroup(for: .audible),
                  let self, generation == self.generation else { return }
            audioGroup = group
            audioTracks = Self.names(group.options, one: "Default", many: "Track")
            audioTrack = item.currentMediaSelection.selectedMediaOption(in: group).flatMap { group.options.firstIndex(of: $0) }
        }
        Task { [weak self] in
            guard let group = try? await asset.loadMediaSelectionGroup(for: .legible),
                  let self, generation == self.generation else { return }
            // A forced-only track shows by itself where the film needs it; it is not a choice.
            let options = AVMediaSelectionGroup.mediaSelectionOptions(
                from: group.options, withoutMediaCharacteristics: [.containsOnlyForcedSubtitles])
            (textGroup, textOptions) = (group, options)
            textTracks = Self.names(options, one: "Subtitles", many: "Subtitles")
            textTrack = item.currentMediaSelection.selectedMediaOption(in: group).flatMap { options.firstIndex(of: $0) }
            #if DEBUG
            if let index = DebugHooks.textTrack { selectText(index) }
            #endif
        }
    }

    /// The options' language names. One tagged "und" (undetermined) or not at
    /// all would read "Unknown language", so it is named by its place instead.
    private static func names(_ options: [AVMediaSelectionOption], one: String, many: String) -> [String] {
        options.enumerated().map { index, option in
            let tag = option.extendedLanguageTag ?? option.locale?.identifier ?? ""
            if !tag.isEmpty, !tag.hasPrefix("und") { return option.displayName }
            return options.count == 1 ? one : "\(many) \(index + 1)"
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
        (loadedAttempt, stoppedAt, pendingStart) = (nil, nil, nil)
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
        (loadedAttempt, stoppedAt, pendingStart) = (nil, nil, nil)
    }

    func togglePlay() {
        if playing { player.pause(); report() } else { if ended { seek(to: 0) }; player.play() }
    }

    func seek(to seconds: Double) {
        let target = CMTime(seconds: max(0, min(seconds, duration > 0 ? duration : seconds)), preferredTimescale: 600)
        position = target.seconds
        // The viewer's seek replaces a start that has not landed yet, and stays pending itself.
        pendingStart = target.seconds
        ended = false
        seekToken += 1
        let (token, generation) = (seekToken, generation)
        player.seek(to: target, toleranceBefore: .zero, toleranceAfter: .zero) { [weak self] done in
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard let self, let landed = startLanded(
                        finished: done, sameItem: generation == self.generation && token == self.seekToken,
                        pending: self.pendingStart
                    ) else { return }
                    self.position = landed
                    self.pendingStart = nil
                    self.report()
                    self.publishNowPlaying()
                }
            }
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
        if textTrack != nil, let group = textGroup, group.allowsEmptySelection {
            player.currentItem?.select(nil, in: group)
            textTrack = nil
        }
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
            var start = loadedURL == nil
                ? sourceStart(stoppedAt: stoppedAt, resumeMs: snapshot.resumeMs)
                : stoppedPosition(pendingStart: pendingStart, position: position)
            stoppedAt = nil
            #if DEBUG
            if loadedURL == nil, let at = DebugHooks.takeSeek() { start = at }
            #endif
            load(url, key: snapshot.key, attempt: snapshot.attempt, at: start)
        case "failed":
            player.pause()
            notice = nil
        default:
            break
        }
    }

    /// `key`: the version this URL plays, for its `duration`; `attempt`: the
    /// core's number for this source, for `failOver`. Neither falls back to
    /// the picked version: an unknown one is not sent.
    private func load(_ url: String, key: String?, attempt: Int64?, at start: Double) {
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
                    guard let self, let key, time.isNumeric, time.seconds > 0, generation == self.generation,
                          let sub = self.sub, self.durationSent.insert(key).inserted else { return }
                    let ms = Int(time.seconds * 1000)
                    Task { try? await self.core.command(sub, ["type": "duration", "key": key, "durationMs": ms]) }
                }
            }
            .store(in: &itemBag)
        player.replaceCurrentItem(with: item)
        loadTracks(item, generation: generation)
        pendingStart = start > 1 ? start : nil
        if start > 1 { position = start }
        seekToken += 1
        let token = seekToken
        if start > 1 {
            let at = CMTime(seconds: start, preferredTimescale: 600)
            // Only a seek that landed hands over, and the cached position takes the start
            // at once: a failure before the next tick then still stops at the start.
            player.seek(to: at, toleranceBefore: .zero, toleranceAfter: .zero) { [weak self] finished in
                DispatchQueue.main.async {
                    MainActor.assumeIsolated {
                        guard let self, let landed = startLanded(
                            finished: finished, sameItem: generation == self.generation && token == self.seekToken,
                            pending: self.pendingStart
                        ) else { return }
                        self.position = landed
                        self.pendingStart = nil
                    }
                }
            }
        }
        NowPlaying.activate()
        player.play()
    }

    /// The player cannot play this source: stop it, then ask the core for the next.
    private func sourceFailed(_ error: Error?) {
        let failure = playerFailure(error)
        let reason = failure.error
        coreLog.error("source failed: \(reason, privacy: .public)")
        notice = "This source failed: \(reason). Trying the next one."
        generation += 1
        player.pause()
        player.replaceCurrentItem(with: nil)
        let command = failOverCommand(failure, attempt: loadedAttempt)
        stoppedAt = stoppedPosition(pendingStart: pendingStart, position: position)
        pendingStart = nil
        (loadedURL, loadedAttempt) = (nil, nil)
        guard let sub else { return }
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
        if let now = ticked(time: time.seconds, pending: pendingStart) { position = now }
        if let total = player.currentItem?.duration, total.isNumeric { duration = total.seconds }
        if duration != publishedDuration { publishNowPlaying() }
        if playing, ContinuousClock.now - lastReport > .seconds(5) { report() }
    }

    /// Saves progress in the core. Nothing before the first frame: a position
    /// of zero then would wipe the resume point.
    private func report() {
        guard rendered, let sub, duration > 0, position > 0 else { return }
        lastReport = .now
        let at = stoppedPosition(pendingStart: pendingStart, position: position)
        core.commandNow(sub, ["type": "progress", "positionMs": Int(at * 1000), "durationMs": Int(duration * 1000)])
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
