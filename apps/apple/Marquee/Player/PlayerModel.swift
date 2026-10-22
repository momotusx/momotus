import Accessibility
import AVFoundation
import Combine
import Observation
#if canImport(UIKit)
import UIKit
#endif

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
    /// Paused by the user or the system, not buffering: what `progress` says to Trakt.
    private var paused = false
    private(set) var waiting = true
    private(set) var ended = false
    private(set) var firstFrameMs: Int?
    /// A local player error the core has not answered yet.
    private(set) var notice: String?
    /// Why the picked subtitle file did not load, shown for a few seconds over the video.
    private(set) var subtitleStatus: String?
    /// The picture's size, for placing subtitles on it rather than on the screen.
    private(set) var videoSize: CGSize = .zero
    private(set) var subtitle: SubtitleTrack?
    /// The loaded stream can go to an AirPlay receiver (not the core's loopback
    /// server): only then does the player offer AirPlay, which sends the picture.
    private(set) var airPlayable = false
    /// Seconds the addon subtitles show later (negative: earlier). A track starts where the core
    /// remembers the viewer left it (`delayMs`); `setSubtitleDelay` changes it.
    private(set) var subtitleDelay: Double = 0 { didSet { watchCueEdges() } }
    /// The stepper's value not yet sent to the core: it goes once it rests, on another track and on close.
    @ObservationIgnored private var outbox: DelayOutbox!
    @ObservationIgnored private var accountWait = AccountWait()
    private(set) var cues: [WebVTT.Cue] = [] { didSet { watchCueEdges() } }
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
    /// Per option: the app can draw it with a timing (subtitles and text, not captions in the video).
    @ObservationIgnored private var textTimeable: [Bool] = []
    /// The item's legible text as its muting output delivers it: the app draws the file's own subtitles itself, so a
    /// timing can move them (plans/apple-file-subtitle-timing.md, decision A).
    @ObservationIgnored private var legibleLog = LegibleLog()
    /// The output's delegate, held here: the output holds it weakly.
    @ObservationIgnored private var legibleSink: LegibleSink?
    /// Bumped when the log changes, so the overlay reads it again.
    private(set) var legibleLines = 0
    @ObservationIgnored private var edgesQueued = false
    /// The boundary observer covers file edges up to here; the tick rebuilds it before the playhead gets there.
    @ObservationIgnored private var edgesUntil = 0.0
    /// Deliveries queued on the main queue before a pick or a reset belong to the old track: they are dropped until a
    /// block enqueued at the pick runs (the main queue keeps order).
    @ObservationIgnored private var legibleOpen = true

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
    /// The viewer's choices in this play and the commands they send.
    @ObservationIgnored private let choices = PlayChoices()
    /// The playback view could not open: its error, shown on a card.
    private(set) var openError: String?
    /// Bumped by Try again on that card, so the view opens again.
    private var opens = 0
    /// The core took a Try again: "Trying again" shows for a moment, even
    /// when the play fails again at once and the snapshot reads as before.
    private(set) var tryingAgain = false
    @ObservationIgnored private var lastRetries: Int?
    @ObservationIgnored private var tryingAgainShown = 0
    /// The loaded source has shown a frame (`rendered` counts the first of the playback).
    @ObservationIgnored private var pictureShown = false
    /// Where the loaded source started, and since when it has played without a frame.
    @ObservationIgnored private var loadStart: Double = 0
    @ObservationIgnored private var playingSince: ContinuousClock.Instant?
    @ObservationIgnored private var lastReport = ContinuousClock.now
    /// The player calls the time observer only while this token is held.
    @ObservationIgnored private var timeObserver: Any?
    /// Fires at each cue's start and end, so a line changes on time.
    @ObservationIgnored private var cueObserver: Any?
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
    /// Picture in Picture holds the film with its page gone: leaving the page keeps it playing.
    @ObservationIgnored var floating = false
    /// The Picture in Picture window keeps a first-frame watch of its own.
    @ObservationIgnored var windowWatchesFrames: @MainActor () -> Bool = { false }
    /// iPhone: whether the player was across when its page last left (Versions
    /// over it, Picture in Picture); nil before its first appear.
    @ObservationIgnored var lastAcross: Bool?
    /// The run of the view and the task id it runs for, outliving a page while the film floats.
    @ObservationIgnored private var running: (id: [String], task: Task<Void, Never>)?
    /// The next load stays paused: the source opened again after a server move.
    @ObservationIgnored private var holdPause = false
    /// The file's audio and subtitle picks, chosen again on the item that
    /// replaces this one after a server move.
    @ObservationIgnored private var carriedTracks: (audio: Int?, text: Int?)?
    #if DEBUG
    /// `-MQWalkLog YES`: the walk's once-a-second playback line.
    @ObservationIgnored private var walkLog: WalkLog?
    #endif

    init(route: PlayRoute, versions: VersionsModel?, core: CoreClient) {
        self.route = route
        self.versions = versions
        self.core = core
        prefs = LiveRecord("settings", core: core)
        // A video pauses in the background by default; the sound goes on with the screen locked.
        player.audiovisualBackgroundPlaybackPolicy = .continuesIfPossible
        #if DEBUG
        // Walks never make a sound (`-MQMute YES`), and the log says so before anything plays.
        player.isMuted = DebugHooks.mute
        coreLog.notice("debug: player muted \(self.player.isMuted, privacy: .public)")
        if WalkLog.on { walkLog = WalkLog(player: player) }
        #endif
        observePlayer()
        outbox = DelayOutbox { [weak self] command in
            // At once, not in a Task: it reaches the core before the view's unsubscribe.
            guard let self, let sub = self.sub else { return false }
            self.core.commandNow(sub, command)
            return true
        }
        outbox.settled = { AccessibilityNotification.Announcement(PlayerLayout.delayWords($0)).post() }
    }

    var taskId: [String] { [route.key, "\(route.versionsSub)", "\(core.generation)", "\(opens)"] }

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

    /// Turns one of the file's own subtitles on, in place of an addon's: the
    /// viewer's pick for this play, which the core does not know of.
    func selectText(_ index: Int) {
        guard let group = textGroup, textOptions.indices.contains(index) else { return }
        choices.choseInFile()
        selectSubtitle(nil)
        player.currentItem?.select(textOptions[index], in: group)
        textTrack = index
        // Another track: nothing delivered for the last one holds; this one starts where it was left this play.
        resetLegible()
        subtitleDelay = PlayerLayout.startFileDelay(textTrack: index, kept: outbox.kept)
    }

    /// The file track on takes timing (`PlayerLayout.fileTrackTimeable`).
    var textTrackTimeable: Bool {
        guard let textTrack, textTimeable.indices.contains(textTrack) else { return false }
        return textTimeable[textTrack]
    }

    /// What the stepper times: the addon track, else a file track the app can draw; nil for neither.
    var timingKey: String? {
        if let url = subtitle?.url { return url }
        return textTrackTimeable ? textTrack.map(PlayerLayout.fileTrackKey) : nil
    }

    /// The file's timing applies to a timeable file track with no addon track on (an addon track has its own, and a
    /// forced line the player picks beside it keeps its time).
    var fileDelay: Double { PlayerLayout.fileDelay(addonOn: subtitle != nil, timeable: textTrackTimeable, delay: subtitleDelay) }

    private func legibleDelivered(_ cues: [String], at time: Double) {
        guard legibleOpen else { return }
        legibleLog.deliver(cues, at: time)
        linesChanged()
    }

    /// Nothing delivered for the last track holds, and what the old track still has queued is dropped.
    private func resetLegible() {
        legibleLog.reset()
        legibleOpen = false
        DispatchQueue.main.async { [weak self] in self?.legibleOpen = true }
        linesChanged()
    }

    /// The overlay reads the log again; the boundary observer is rebuilt once per run loop turn, not per delivery
    /// (a lookahead burst is dozens at once).
    private func linesChanged() {
        legibleLines += 1
        guard !edgesQueued else { return }
        edgesQueued = true
        Task { @MainActor [weak self] in
            self?.edgesQueued = false
            self?.watchCueEdges()
        }
    }

    /// Reads the audio and subtitle tracks once the file is open.
    private func loadTracks(_ item: AVPlayerItem, generation: Int) {
        (audioTracks, audioTrack, audioGroup) = ([], nil, nil)
        (textTracks, textTrack, textGroup, textOptions, textTimeable) = ([], nil, nil, [], [])

        let asset = item.asset
        Task { [weak self] in
            guard let group = try? await asset.loadMediaSelectionGroup(for: .audible),
                  let self, generation == self.generation else { return }
            audioGroup = group
            audioTracks = Self.names(group.options, one: "Default", many: "Track")
            audioTrack = item.currentMediaSelection.selectedMediaOption(in: group).flatMap { group.options.firstIndex(of: $0) }
            if let index = carriedTracks?.audio { selectAudio(index) }
            carriedTracks?.audio = nil
        }
        Task { [weak self] in
            guard let group = try? await asset.loadMediaSelectionGroup(for: .legible),
                  let self, generation == self.generation else { return }
            // A forced-only track shows by itself where the film needs it; it is not a choice.
            let options = AVMediaSelectionGroup.mediaSelectionOptions(
                from: group.options, withoutMediaCharacteristics: [.containsOnlyForcedSubtitles])
            (textGroup, textOptions) = (group, options)
            textTimeable = options.map { PlayerLayout.fileTrackTimeable(mediaType: $0.mediaType.rawValue) }
            textTracks = Self.names(options, one: "Subtitles", many: "Subtitles")
            textTrack = item.currentMediaSelection.selectedMediaOption(in: group).flatMap { options.firstIndex(of: $0) }
            // The live timing follows the new item's track: where that track was left this play, else none (another
            // version or source never plays at the last file's timing).
            if subtitle == nil { subtitleDelay = PlayerLayout.startFileDelay(textTrack: textTrack, kept: outbox.kept) }

            if let index = carriedTracks?.text { selectText(index) }
            carriedTracks?.text = nil
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
    /// The profile's subtitle size, synced (`Prefs.subtitleSize`).
    var subtitleSize: SubtitleSize { prefs.snapshot?.subtitleSize ?? .medium }

    /// The profile's size on the file's own tracks too, drawn by AVFoundation:
    /// on each new item and when the size changes.
    func textSize(_ item: AVPlayerItem? = nil) {
        guard let item = item ?? player.currentItem else { return }
        let rule = CaptionStyle.relativeFontSize(subtitleSize).flatMap {
            AVTextStyleRule(textMarkupAttributes: [kCMTextMarkupAttribute_RelativeFontSize as String: $0])
        }
        item.textStyleRules = rule.map { [$0] }
    }
    /// The profile's subtitle languages, then its audio ones: the Subtitles menu's top groups.
    var subtitleLanguages: [String] {
        SubtitleMenu.preferred(
            subtitle: prefs.snapshot?.subtitleLanguages ?? [],
            audio: prefs.snapshot?.streams.languages ?? [],
            device: DeviceCaps.language
        )
    }

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
        // The next episode gets its own start subtitle.
        route = choices.nextEpisode(pick)
        (loadedURL, rendered, firstFrameMs, playback, ended, nextCancelled, nextFailure, nextVersions) = (nil, false, nil, nil, false, false, nil, nil)
        (loadedAttempt, stoppedAt, pendingStart) = (nil, nil, nil)
        (position, duration, skippedAt) = (0, 0, nil)
        selectSubtitle(nil)
        // Floating in Picture in Picture there is no page to start the new
        // run: the model starts it, and a page brought back joins it.
        if floating { Task { await drive() } }
    }

    /// The page's run of the view. A page shown again over a floating film
    /// joins the run already going; a page leaving ends the run unless the
    /// film floats (review apple-journey P2-4).
    func drive() async {
        let id = taskId
        if running?.id != id {
            running?.task.cancel()
            running = (id, Task { await run() })
        }
        guard let task = running?.task else { return }
        await withTaskCancellationHandler {
            await task.value
        } onCancel: {
            Task { @MainActor in if !self.floating { task.cancel() } }
        }
    }

    /// The floating window closed with its X: the film ends as Back ends it.
    func end() {
        floating = false
        close()
        running?.task.cancel()
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
        openError = nil
        #if DEBUG
        let release = false
        #else
        let release = true
        #endif
        let store = Bundle.main.object(forInfoDictionaryKey: "MarqueeStore") as? String == "1"
        if let refusal = PlayerLayout.engineRefusal(hasEngine: hasEngine(), store: store, release: release) {
            openError = refusal
            return
        }
        // A new view counts its own Try again presses.
        lastRetries = nil
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
            let core = core
            choices.opened(sub: view.sub) { sub, command in Task { try? await core.command(sub, command) } }
            for await event in view.events {
                switch event {
                case .snapshot(let value): apply(value)
                case .updates(let values): if let last = values.last { apply(last) }
                }
            }
        } catch is CancellationError {
        } catch {
            openError = error.localizedDescription
        }
    }

    /// Plays another version of the same video from where this one is.
    func switchVersion(_ key: String) {
        // The old track's URL goes with the old view: the new version starts
        // with its own start subtitle.
        guard let next = choices.switchVersion(from: route, to: key) else { return }
        // Another file: its own tracks' timings start again (a server move keeps them: the same file).
        outbox.forget(prefix: PlayerLayout.fileTrackPrefix)
        report()
        route = next
        (loadedURL, rendered, firstFrameMs, playback) = (nil, false, nil, nil)
        (loadedAttempt, stoppedAt, pendingStart) = (nil, nil, nil)
        selectSubtitle(nil)
    }

    private func showTryingAgain() {
        tryingAgain = true
        tryingAgainShown += 1
        let shown = tryingAgainShown
        Task {
            try? await Task.sleep(for: PlayerLayout.tryingAgainHold)
            if shown == tryingAgainShown { tryingAgain = false }
        }
    }

    /// The core's stream server came back on another port while the app was
    /// away, so the loaded URL is dead: the view opens again and its new URL
    /// loads where this one was, paused if it was (review apple-journey P1-1).
    func serverMoved() {
        guard loadedURL != nil else { return }
        holdPause = !playing
        // The picks stay (review apple-journey P3-5): the file's audio and
        // subtitle tracks are chosen again once the new item has them, and the
        // addon track, its cues and its timing are kept.
        carriedTracks = (audioTrack, textTrack)
        choices.reopened()
        // To the view about to close, while its track URLs are the ones the value names.
        flushDelay()
        reopen()
    }

    /// Try again on the card for a view that could not open: open it again.
    func reopen() {
        openError = nil
        opens += 1
    }

    func togglePlay() {
        if playing { player.pause(); report(paused: true) } else { if ended { seek(to: 0) }; player.play() }
    }

    func seek(to seconds: Double) {
        let target = CMTime(seconds: max(0, min(seconds, duration > 0 ? duration : seconds)), preferredTimescale: 600)
        position = target.seconds
        // A seek is not playback: the missing-picture count starts again where it lands.
        loadStart = target.seconds
        // The viewer's seek replaces a start that has not landed yet, and stays pending itself.
        pendingStart = target.seconds
        ended = false
        // The log drops its open frontier; the output delivers again from the landing on (MP4 and HLS, bench AFS4, AFS7).
        legibleLog.seek()
        linesChanged()
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
        pictureShown = true
        guard !rendered else { return }
        rendered = true
        let ms = Int((ContinuousClock.now - route.tapped) / .milliseconds(1))
        firstFrameMs = ms
        // The key's kind only ("url", "bt", "rel"): a "url:" key is the stream link, which can carry an addon's key.
        let kind = String(route.key.prefix { $0 != ":" })
        coreLog.notice("metric first_frame_ms=\(ms, privacy: .public) key=\(kind, privacy: .public)")
    }

    /// The viewer's pick from the menu, or Off: it wins for the rest of the
    /// play, and the core remembers it for the title (`subtitleChosen`).
    func chooseSubtitle(_ track: SubtitleTrack?) {
        choices.chose(track)
        selectSubtitle(track)
    }

    /// "Try again" on the failure card: the core starts the same versions
    /// again from the first, each with its full start time.
    func retry() {
        notice = nil
        choices.retry()
    }

    /// The versions or the card changed: a card waiting for an account (`AccountWait`) plays the
    /// versions' pick once they offer one, the same version again when it is the one that failed.
    func versionsChanged() {
        let failed = playback?.phase == "failed" || openError != nil
        guard let pick = accountWait.see(failed: failed, versions: versions?.snapshot) else { return }
        if pick != route.key {
            openError = nil
            switchVersion(pick)
        } else if openError != nil {
            reopen()
        } else {
            retry()
        }
    }

    /// The stepper's value for the addon track on: drawn at once, sent to the core (which keeps it for
    /// this track) once it rests, and said to VoiceOver then (`spoken`), never on each tick of a hold.
    func setSubtitleDelay(_ seconds: Double, spoken: Bool = true) {
        if let url = subtitle?.url {
            subtitleDelay = seconds
            outbox.set(url: url, seconds: seconds, spoken: spoken)
            return
        }
        // A file's own track: kept for the play, never sent (the core does not know it).
        guard let key = timingKey else { return }
        subtitleDelay = seconds
        outbox.set(url: key, seconds: seconds, spoken: spoken, sent: false)
    }

    /// The value not sent yet goes to the view still open, now.
    private func flushDelay() { outbox.flush() }

    func selectSubtitle(_ track: SubtitleTrack?) {
        if track?.url != subtitle?.url {
            // Another track's change still waiting goes first; the new one starts where it was left.
            flushDelay()
            subtitleDelay = outbox.start(track)
        }
        subtitle = track
        cues = []
        if textTrack != nil, let group = textGroup, group.allowsEmptySelection {
            player.currentItem?.select(nil, in: group)
            textTrack = nil
            resetLegible()
        }
        guard let track, let url = URL(string: track.url) else { return }
        Task {
            guard let (data, _) = try? await URLSession.shared.data(from: url), subtitle == track else { return }
            cues = WebVTT.parse(String(decoding: data, as: UTF8.self))
        }
    }

    /// The drawn line changes at each cue's start and end, shifted by the
    /// timing, not on the next quarter second tick (review apple-journey P3-13).
    private func watchCueEdges() {
        if let cueObserver { player.removeTimeObserver(cueObserver) }
        cueObserver = nil
        let edges = PlayerLayout.cueEdges(cues, delay: subtitleDelay)
            + legibleLog.edges(delay: fileDelay, from: position, ahead: PlayerLayout.maxSubtitleDelay + 10)
        edgesUntil = position + PlayerLayout.maxSubtitleDelay
        guard !edges.isEmpty else { return }
        let times = edges.map { NSValue(time: CMTime(seconds: $0, preferredTimescale: 600)) }
        cueObserver = player.addBoundaryTimeObserver(forTimes: times, queue: .main) { [weak self] in
            MainActor.assumeIsolated {
                guard let self else { return }
                self.tick(self.player.currentTime())
            }
        }
    }

    /// The file's line (its timing, or none) and the addon track's (its own): both when the player shows a forced line
    /// beside an addon track.
    var subtitleText: String? {
        _ = legibleLines
        return PlayerLayout.overlayText(
            file: legibleLog.text(at: PlayerLayout.cueTime(position: position, delay: fileDelay)),
            addon: WebVTT.text(at: PlayerLayout.cueTime(position: position, delay: subtitleDelay), in: cues))
    }

    private func showSubtitleStatus(_ words: String) {
        subtitleStatus = words
        Task {
            try? await Task.sleep(for: .seconds(5))
            if subtitleStatus == words { subtitleStatus = nil }
        }
    }

    // MARK: core and player plumbing

    /// Milliseconds since the play was tapped, for the metric lines.
    private var sinceTap: Int { Int((ContinuousClock.now - route.tapped) / .milliseconds(1)) }

    private func apply(_ snapshot: PlaybackSnapshot) {
        let wasPending = playback?.subtitlesPending == true
        if let trouble = PlayerLayout.subtitleTrouble(
            picked: subtitle, old: playback?.notes ?? [], new: snapshot.notes, tracks: snapshot.subtitles
        ) {
            showSubtitleStatus(trouble.say)
            // Its file played as an empty one: the position stays, and no other version is tried.
            if trouble.off { selectSubtitle(nil) }
        }
        playback = snapshot
        if PlayerLayout.retryRose(old: lastRetries, new: snapshot.retries) { showTryingAgain() }
        lastRetries = snapshot.retries ?? lastRetries
        tryingAgain = PlayerLayout.keepsTryingAgain(showing: tryingAgain, phase: snapshot.phase)
        // The core's start subtitle, once, as if picked, sending nothing.
        // A move on to another source: the old start subtitle and its cues go.
        let step = choices.subtitleStep(attempt: snapshot.attempt, auto: snapshot.autoSubtitle, tracks: snapshot.subtitles)
        if step.drop { selectSubtitle(nil) }
        if let track = step.turnOn {
            selectSubtitle(track)
            coreLog.notice("metric start_subtitle_ms=\(self.sinceTap, privacy: .public)")
        }
        if wasPending, !snapshot.subtitlesPending {
            coreLog.notice("metric subtitles_settled_ms=\(self.sinceTap, privacy: .public) tracks=\(snapshot.subtitles.count, privacy: .public)")
        }
        // After a server move the same addon track has a new URL: it stays on,
        // its cues and timing kept, and the menu marks it.
        if let shown = subtitle, !snapshot.subtitles.contains(shown), let same = SubtitleMenu.same(shown, in: snapshot.subtitles) {
            outbox.moved(from: shown.url, to: same.url)
            subtitle = same
        }
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
                ? sourceStart(stoppedAt: stoppedAt, resumeMs: snapshot.resumeMs, fromStart: route.fromStart)
                : stoppedPosition(pendingStart: pendingStart, position: position)
            stoppedAt = nil
            // Start over holds for the first source only: a later reopen resumes.
            if route.fromStart { route.fromStart = false }
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
        (pictureShown, loadStart, playingSince) = (false, start, nil)
        // An AirPlay receiver would be handed this URL and ask itself for it:
        // a loopback stream stays on this device (mirroring still works).
        airPlayable = PlayerLayout.allowsExternalPlayback(address)
        player.allowsExternalPlayback = airPlayable
        let item = AVPlayerItem(url: address)
        textSize(item)
        // Made with the item and never removed: an output added later gets no delivery, and any legible output stops
        // the player drawing the track (bench AFS2, AFS3), so the app draws the item's legible text itself.
        let sink = LegibleSink { [weak self] cues, time in
            guard let self, generation == self.generation else { return }
            self.legibleDelivered(cues, at: time)
        }
        let legible = AVPlayerItemLegibleOutput()
        legible.advanceIntervalForDelegateInvocation = PlayerLayout.maxSubtitleDelay
        legible.suppressesPlayerRendering = true
        legible.setDelegate(sink, queue: .main)
        item.add(legible)
        legibleSink = sink
        legibleLog.reset()
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
        // A source opened again after the server moved keeps the viewer's pause.
        if holdPause { holdPause = false } else { player.play() }
    }

    /// The player cannot play this source: stop it, then ask the core for the next.
    private func sourceFailed(_ error: Error?) {
        let failure = playerFailure(error)
        fail(failure, notice: "This source failed: \(failure.error). Trying the next one.")
    }

    /// The sound runs but no frame ever came: never a spinner over a running
    /// audio track. The core tries the next version, or says Try again.
    private func pictureFailed() {
        fail(PlayerFailure(kind: PlayerLayout.pictureFailureKind(floating: floating), error: "this version's picture cannot be shown on this device"),
             notice: "This version's picture cannot be shown here, trying the next one")
    }

    private func fail(_ failure: PlayerFailure, notice words: String) {
        coreLog.error("source failed: \(failure.error, privacy: .public)")
        notice = words
        generation += 1
        player.pause()
        player.replaceCurrentItem(with: nil)
        // A delay not sent yet goes first, while the failed source's version is the current one.
        flushDelay()
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
                    guard let self else { return }
                    self.playing = status == .playing
                    self.waiting = status == .waitingToPlayAtSpecifiedRate
                    // Playing without a pause or a stall: buffering is not a missing picture.
                    self.playingSince = status == .playing ? self.playingSince ?? .now : nil
                    self.publishNowPlaying()
                    // A pause or a play from anywhere (the Lock Screen, a call) reaches Trakt at once;
                    // buffering is neither.
                    if let paused = PlayerLayout.traktPaused(paused: status == .paused, buffering: status == .waitingToPlayAtSpecifiedRate, was: self.paused) {
                        self.paused = paused
                        self.report()
                    }
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

    /// The app is in front (iOS, tvOS); a Mac window counts as on screen.
    private var appOnScreen: Bool {
        #if os(macOS)
        true
        #else
        UIApplication.shared.applicationState == .active
        #endif
    }

    private func tick(_ time: CMTime) {
        guard time.isNumeric else { return }
        if let now = ticked(time: time.seconds, pending: pendingStart) { position = now }
        // A track delivered whole (HLS) sends nothing more: the observer moves on with the playhead.
        if position > edgesUntil { watchCueEdges() }
        let watched = PlayerLayout.pictureWatched(floating: floating, windowWatches: windowWatchesFrames(), onScreen: appOnScreen)
        (loadStart, playingSince) = PlayerLayout.pictureClocks(
            watched: watched, position: position, loadStart: loadStart, playingSince: playingSince, now: .now)
        if loadedURL != nil, PlayerLayout.pictureMissing(
            shown: pictureShown, advanced: position - loadStart,
            playingFor: playingSince.map { (ContinuousClock.now - $0) / .seconds(1) } ?? 0,
            watched: watched
        ) {
            pictureFailed()
            return
        }
        if let total = player.currentItem?.duration, total.isNumeric { duration = total.seconds }
        if duration != publishedDuration { publishNowPlaying() }
        if playing, ContinuousClock.now - lastReport > .seconds(5) { report() }
    }

    /// Saves progress in the core. Nothing before the first frame: a position
    /// of zero then would wipe the resume point.
    /// `paused` tells Trakt's scrobbles a pause from a play (the player's own pause unless given).
    private func report(paused: Bool? = nil) {
        guard rendered, let sub, duration > 0, position > 0 else { return }
        lastReport = .now
        let at = stoppedPosition(pendingStart: pendingStart, position: position)
        core.commandNow(sub, ["type": "progress", "positionMs": Int(at * 1000), "durationMs": Int(duration * 1000), "paused": paused ?? self.paused])
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
        flushDelay()
        report()
        player.pause()
    }

    private func stop() {
        flushDelay()
        report()
        nowPlaying.stop()
        NowPlaying.deactivate()
        itemBag = []
        player.pause()
        player.replaceCurrentItem(with: nil)
        sub = nil
    }
}

/// The item's legible output delegate: each delivery's strings (one per cue), on the main queue.
private final class LegibleSink: NSObject, AVPlayerItemLegibleOutputPushDelegate, @unchecked Sendable {
    private let deliver: @MainActor ([String], Double) -> Void

    init(deliver: @escaping @MainActor ([String], Double) -> Void) { self.deliver = deliver }

    func legibleOutput(_ output: AVPlayerItemLegibleOutput, didOutputAttributedStrings strings: [NSAttributedString],
                       nativeSampleBuffers nativeSamples: [Any], forItemTime itemTime: CMTime) {
        let cues = strings.map(\.string)
        let time = itemTime.seconds
        MainActor.assumeIsolated { deliver(cues, time) }
    }
}
