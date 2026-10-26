import Accessibility
import AVFoundation
import AetherEngine
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
    /// Which engine plays the current source (plans/apple-second-engine.md 2.2): AVPlayer, or the second engine
    /// (`aether`) where `EngineChoice` turns it on.
    private(set) var backend: Backend = .avPlayer
    /// The second engine, made the first time a source needs it.
    @ObservationIgnored private(set) var aether: AetherSession?
    /// One more each time a source on the second engine shows its first frame, failovers included: the window
    /// attaches to that session's layer (`firstFrameMs` is set once a play).
    private(set) var enginePictures = 0
    /// The current source reached the second engine because AVPlayer refused it: its first frame tells the core
    /// (`playedOn`), so the next play of the version goes there first.
    @ObservationIgnored private var fellBack = false
    @ObservationIgnored private var loadedKey: String?
    /// The engine's ids for the listed audio and file subtitle tracks, in `audioTracks` and `textTracks` order.
    @ObservationIgnored private var engineAudioIDs: [Int] = []
    @ObservationIgnored private var engineTextIDs: [Int] = []
    /// On the second engine, the file subtitle's cues decoded so far (the engine hands the whole list each time).
    private(set) var engineCues: [WebVTT.Cue] = []
    /// On the second engine, the file subtitle's bitmap cues (PGS, DVD, DVB) decoded so far.
    private(set) var engineImages: [AetherSession.ImageCue] = []
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
    /// A few seconds of words over the video: why the picked subtitle file did not load, or why the play moved or paused.
    private(set) var statusLine: String?
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
    /// The mid-play stall the core is told of (`stalled`).
    @ObservationIgnored private var stall = StallWatch()
    /// Versions is open over the player: nothing plays or sounds under it, the attempt keeps resolving.
    @ObservationIgnored private var covered = false
    @ObservationIgnored private var playsAfterCover = false
    /// Play opened the player while versions were found, and none could start.
    private(set) var noPick = false
    private(set) var cues: [WebVTT.Cue] = [] { didSet { watchCueEdges() } }
    var rate: Float = 1 {
        // The Lock Screen extrapolates the playhead from the rate: it hears every change at once.
        didSet {
            if backend == .aether { aether?.setRate(rate) } else { player.defaultRate = rate; if playing { player.rate = rate } }
            publishNowPlaying()
        }
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
    /// Each audio track's language tag as the file gives it, for the picker's flags; nil for none.
    private(set) var audioTrackLanguages: [String?] = []
    /// The file's own default audio track, marked in the picker.
    private(set) var audioDefault: Int?
    private(set) var audioTrack: Int?
    @ObservationIgnored private var audioGroup: AVMediaSelectionGroup?
    /// The file's own subtitles (a stream's WebVTT renditions, a file's text
    /// tracks) by name, and the index of the one on. AVKit draws them in the
    /// viewer's caption style; an addon's subtitle is drawn by the app instead.
    private(set) var textTracks: [String] = []
    /// Each of the file's subtitle tracks' language tag, for the picker; nil for none.
    private(set) var textTrackLanguages: [String?] = []
    /// A track picker is open over the player: the Mac menu's bare keys (arrows, Space, Esc) are the picker's.
    var pickerOpen = false
    /// The file's track the core's start pick turned on in place of its own (nil for none).
    @ObservationIgnored private var startFile: Int?
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
    /// The start window's bytes as the core counted them, for the ready and failed metrics (the bar's swarm figure).
    @ObservationIgnored private var startWindow = StartWindow()
    /// The heat step was taken this play (once: a move's new version keeps playing however warm the phone stays).
    @ObservationIgnored private var heatHandled = false
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
        #if MARQUEE_WALK
        soak()
        #endif
        observePlayer()
        #if os(iOS)
        watchHeat()
        #endif
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
        if backend == .aether {
            guard engineAudioIDs.indices.contains(index) else { return }
            aether?.selectAudio(id: engineAudioIDs[index])
            audioTrack = index
            return
        }
        guard let group = audioGroup, group.options.indices.contains(index) else { return }
        player.currentItem?.select(group.options[index], in: group)
        audioTrack = index
    }

    /// Turns one of the file's own subtitles on, in place of an addon's: the
    /// viewer's pick, sent to the core with its language tag when it has one.
    /// `start`: the core's start pick turned it on (its language's track inside the file wins), not the viewer.
    func selectText(_ index: Int, start: Bool = false) {
        if backend == .aether {
            guard engineTextIDs.indices.contains(index) else { return }
            if start { startFile = index } else { choices.choseInFile(language: textTrackLanguages[safe: index] ?? nil) }
            selectSubtitle(nil)
            aether?.selectText(id: engineTextIDs[index])
            (textTrack, engineCues, engineImages) = (index, [], [])
            subtitleDelay = PlayerLayout.startFileDelay(textTrack: index, kept: outbox.kept)
            return
        }
        guard let group = textGroup, textOptions.indices.contains(index) else { return }
        if start { startFile = index } else { choices.choseInFile(language: textTrackLanguages[safe: index] ?? nil) }
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
        (audioTracks, audioTrackLanguages, audioDefault, audioTrack, audioGroup) = ([], [], nil, nil, nil)
        (textTracks, textTrackLanguages, textTrack, textGroup, textOptions, textTimeable) = ([], [], nil, nil, [], [])

        let asset = item.asset
        Task { [weak self] in
            guard let group = try? await asset.loadMediaSelectionGroup(for: .audible),
                  let self, generation == self.generation else { return }
            audioGroup = group
            audioTracks = Self.names(group.options, audio: true)
            audioTrackLanguages = group.options.map(Self.tag)
            audioDefault = group.defaultOption.flatMap { group.options.firstIndex(of: $0) }
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
            textTracks = Self.names(options, audio: false)
            textTrackLanguages = options.map(Self.tag)
            startFile = nil
            textTrack = item.currentMediaSelection.selectedMediaOption(in: group).flatMap { options.firstIndex(of: $0) }
            // The live timing follows the new item's track: where that track was left this play, else none (another
            // version or source never plays at the last file's timing).
            if subtitle == nil { subtitleDelay = PlayerLayout.startFileDelay(textTrack: textTrack, kept: outbox.kept) }

            if let index = carriedTracks?.text { selectText(index) }
            carriedTracks?.text = nil
            // The file's tracks came after the core's start pick: its language's track inside the file takes over.
            if let auto = subtitle, !choices.viewerChose, auto.url == playback?.autoSubtitle { startPick(auto) }
            #if DEBUG
            if let index = DebugHooks.textTrack { selectText(index) }
            #endif
        }
    }

    /// An option's language tag, nil when it names none.
    private static func tag(_ option: AVMediaSelectionOption) -> String? {
        TrackPicker.tag(option.extendedLanguageTag ?? option.locale?.identifier)
    }

    /// The file's subtitle tracks as the picker groups them under their languages.
    var fileTextTracks: [TrackPicker.FileTrack] {
        textTracks.indices.map { TrackPicker.FileTrack(index: $0, name: textTracks[$0], language: textTrackLanguages[safe: $0] ?? nil) }
    }

    /// The track on is the core's start pick, not the viewer's (its file track included): the picker marks
    /// its language Auto.
    var subtitleIsAuto: Bool {
        TrackPicker.isAuto(
            subtitleURL: subtitle?.url, autoURL: playback?.autoSubtitle, textTrack: textTrack, startFile: startFile,
            viewerChose: choices.viewerChose)
    }

    /// The core's start subtitle: a track inside the file of its language wins once the file has its tracks
    /// (design/DECISION.md, Subtitle picker); else the core's own track.
    private func startPick(_ track: SubtitleTrack) {
        if let index = TrackPicker.startFile(track, in: fileTextTracks) {
            selectText(index, start: true)
        } else {
            selectSubtitle(track)
        }
    }

    /// The options' language names. One tagged "und" (undetermined) or not at
    /// all would read "Unknown language", so it is named by its place instead.
    private static func names(_ options: [AVMediaSelectionOption], audio: Bool) -> [String] {
        options.enumerated().map { index, option in
            let tag = option.extendedLanguageTag ?? option.locale?.identifier ?? ""
            if !tag.isEmpty, !tag.hasPrefix("und") { return option.displayName }
            return TrackPicker.placeName(index: index, count: options.count, audio: audio)
        }
    }

    /// Plays this one again from the start.
    func playAgain() {
        (nextFailure, nextVersions, nextCancelled, skippedAt) = (nil, nil, false, nil)
        seek(to: 0)
        if backend == .aether { aether?.play() } else { player.play() }
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
            let words = model.snapshot?.empty.flatMap { VersionWords.emptyLine($0.kind) }
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
        heatHandled = false
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
        #if DEBUG
        if let fixture = DebugHooks.playbackFixture {
            apply(fixture)
            return
        }
        #endif
        // Play pressed while versions were found: the pick, once it settles, opens the view (the page's
        // task drives the run again for the new key); none, and the card says why.
        if route.waitsForPick {
            guard let versions else {
                noPick = true
                return
            }
            // No app clock: the wait ends when the core's view does (done, or a settled pick).
            guard var pick = await versions.settledPick(tapped: route.tapped, limit: nil) else {
                if !Task.isCancelled { noPick = true }
                return
            }
            pick.fromStart = route.fromStart
            route = pick
            return
        }
        runs += 1
        let run = runs
        durationSent = []
        prefs.start()
        nowPlaying.stop()
        nowPlaying.start(self)
        Task { await nowPlaying.loadArtwork(route.target.poster) }
        defer { if run == runs { stop() } }
        openError = nil
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
            choices.opened(sub: view.sub) { sub, command in core.commandNow(sub, command) }
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

    #if MARQUEE_WALK
    /// `-MQSoak N` (walk builds only, plans/iphone-mkv.md phase 3): N version moves in this process, one every 45 s,
    /// to the next listed row after the one playing, so the sampler's footprint shows whether sessions leak. A step
    /// that finds fewer than two rows is logged as skipped; it is not tried again.
    private func soak() {
        let moves = UserDefaults.standard.integer(forKey: "MQSoak")
        guard moves > 0 else { return }
        Task { [weak self] in
            for step in 1...moves {
                try? await Task.sleep(for: .seconds(45))
                guard let self else { return }
                // The Versions menu's list: the recommendation, then the rows.
                let rows = self.versions?.snapshot.map { [$0.recommended].compactMap { $0 } + $0.rows } ?? []
                guard rows.count > 1 else {
                    coreLog.notice("metric soak step=\(step, privacy: .public) of=\(moves, privacy: .public) skipped rows=\(rows.count, privacy: .public)")
                    continue
                }
                let playing = self.playback?.key ?? self.route.key
                let at = rows.firstIndex { $0.key == playing } ?? -1
                let next = rows[(at + 1) % rows.count]
                coreLog.notice("metric soak step=\(step, privacy: .public) of=\(moves, privacy: .public) to=\(next.key, privacy: .public)")
                self.switchVersion(next.key)
            }
        }
    }
    #endif

    /// Plays another version of the same video from where this one is.
    func switchVersion(_ key: String) {
        // The old track's URL goes with the old view: the new version starts
        // with its own start subtitle.
        if route.waitsForPick, let pick = versions?.play(key) {
            // Chosen from Versions over a player that found no pick: that version starts now.
            (noPick, route) = (false, pick)
            return
        }
        guard let next = choices.switchVersion(from: route, to: key) else { return }
        // A move before the last start was ready: the new start measures only itself.
        startWindow = StartWindow()
        sendStall(stall.interrupted())
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

    /// The film plays now, on whichever engine: a film brought back from Picture in Picture reads it.
    var playingNow: Bool { backend == .aether ? playing || waiting : player.rate > 0 }

    /// Plays on, on whichever engine.
    func resume() {
        if backend == .aether { aether?.play() } else { player.play() }
    }

    /// Picture in Picture started or stopped: the engine keeps its picture alive in the window and hands an item over
    /// in place while it is up (it asks its host to say so).
    func pictureInPicture(active: Bool) {
        aether?.engine.pictureInPictureActive = active
    }

    func togglePlay() {
        // Playing again answers a line that waited for it (a heat pause).
        if !playing, statusLine == Self.heatPaused { statusLine = nil }
        if backend == .aether, let aether {
            if playing { aether.pause(); report(paused: true) } else { if ended { seek(to: 0) }; aether.play() }
            return
        }
        if playing { player.pause(); report(paused: true) } else { if ended { seek(to: 0) }; player.play() }
    }

    func seek(to seconds: Double) {
        sendStall(stall.interrupted())
        if backend == .aether, let aether {
            let target = max(0, min(seconds, duration > 0 ? duration : seconds))
            (position, ended, loadStart) = (target, false, target)
            // As on AVPlayer: the clock's old point does not come back before the seek lands, and only the last
            // seek's landing hands over.
            pendingStart = target
            linesChanged()
            seekToken += 1
            let (token, generation) = (seekToken, generation)
            Task {
                await aether.seek(to: target)
                guard token == self.seekToken, generation == self.generation else { return }
                self.pendingStart = nil
                self.report()
                self.publishNowPlaying()
            }
            return
        }
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
        coreLog.notice("metric first_frame_ms=\(ms, privacy: .public) key=\(kind, privacy: .public) backend=\(self.backend.rawValue, privacy: .public)")
        #if MARQUEE_WALK
        readBack()
        #endif
    }

    #if MARQUEE_WALK
    /// Where `-MQSeekTo` asked the first source to start, for the walk's read-back.
    @ObservationIgnored private var walkSeekTarget: Double?

    /// Walk builds read back what a row cannot see in its shots (plans/iphone-mkv.md phase 4): 1 s after the first
    /// frame, where a `-MQSeekTo` start landed against its target; 5 s after it, the tracks the player lists and the
    /// ones it plays; with `-MQSeekWhilePlaying YES`, at 60 s two seeks while playing, to 50 and 90 percent, each read
    /// back once the position moves on from the target (code r8).
    private func readBack() {
        let target = walkSeekTarget
        walkSeekTarget = nil
        Task { [weak self] in
            try? await Task.sleep(for: .seconds(1))
            if let self, let target {
                coreLog.notice("metric \(WalkReadBack.seek(target: target, position: self.position), privacy: .public)")
            }
            try? await Task.sleep(for: .seconds(4))
            guard let self else { return }
            coreLog.notice("metric \(WalkReadBack.tracks(text: self.textTracks, activeText: self.textTrack, audio: self.audioTracks, activeAudio: self.audioTrack), privacy: .public)")
            guard UserDefaults.standard.bool(forKey: "MQSeekWhilePlaying") else { return }
            try? await Task.sleep(for: .seconds(55))
            for fraction in [0.5, 0.9] {
                guard self.duration > 0 else { return }
                let target = self.duration * fraction
                let asked = ContinuousClock.now
                self.seek(to: target)
                // Played on from the target: two reads 0.5 s apart inside the window, the second further on.
                var resumed = false
                while !resumed, ContinuousClock.now - asked < .seconds(30) {
                    let before = self.position
                    try? await Task.sleep(for: .milliseconds(500))
                    resumed = WalkReadBack.playsOn(target: target, before: before, after: self.position)
                }
                let ms = resumed ? "\(Int((ContinuousClock.now - asked) / .milliseconds(1)))" : "n/a"
                try? await Task.sleep(for: .seconds(1))
                coreLog.notice("metric \(WalkReadBack.seek(target: target, position: self.position), privacy: .public) while_playing=1 seek_ms=\(ms, privacy: .public)")
                try? await Task.sleep(for: .seconds(20))
            }
        }
    }
    #endif

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

    /// Try again on the card of a play that found no version: the versions are asked again and the
    /// player waits for their pick.
    func retryPick() {
        versions?.retry()
        noPick = false
        reopen()
    }

    /// Versions opens over the player: it pauses (nothing plays or sounds under the page) and a source
    /// that comes in meanwhile loads paused; the attempt keeps resolving.
    func cover() {
        guard !covered else { return }
        (covered, playsAfterCover) = (true, !paused)
        if backend == .aether { aether?.pause() } else { player.pause() }
    }

    /// Back from Versions: the player as it was, playing again unless the viewer had paused it.
    func uncover() {
        guard covered else { return }
        covered = false
        if playsAfterCover, loadedURL != nil, playback?.phase != "failed" {
            if backend == .aether { aether?.play() } else { player.play() }
        }
    }

    /// Tells the core a mid-play stall started or ended (plans/play-every-version.md 2e).
    private func sendStall(_ change: Bool?) {
        guard let change, let sub else { return }
        let command = StallWatch.command(change, position: position)
        core.commandNow(sub, command)
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
        if backend == .aether, textTrack != nil {
            aether?.clearText()
            (textTrack, engineCues, engineImages) = (nil, [], [])
        }
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
    /// The file subtitle's bitmaps on screen now, on the second engine, at the file's timing.
    var subtitleImages: [AetherSession.ImageCue] {
        guard backend == .aether, textTrack != nil, subtitle == nil else { return [] }
        let at = PlayerLayout.cueTime(position: position, delay: fileDelay)
        return engineImages.filter { $0.start <= at && at < $0.end }
    }

    var subtitleText: String? {
        _ = legibleLines
        let fileAt = PlayerLayout.cueTime(position: position, delay: fileDelay)
        return PlayerLayout.overlayText(
            file: backend == .aether ? WebVTT.text(at: fileAt, in: engineCues) : legibleLog.text(at: fileAt),
            addon: WebVTT.text(at: PlayerLayout.cueTime(position: position, delay: subtitleDelay), in: cues))
    }

    /// `untilPlay`: the line stays until the viewer plays again (a heat pause), not for a few seconds.
    private func showStatus(_ words: String, untilPlay: Bool = false) {
        statusLine = words
        guard !untilPlay else { return }
        Task {
            try? await Task.sleep(for: .seconds(5))
            if statusLine == words { statusLine = nil }
        }
    }

    // MARK: core and player plumbing

    /// Milliseconds since the play was tapped, for the metric lines.
    private var sinceTap: Int { Int((ContinuousClock.now - route.tapped) / .milliseconds(1)) }

    private func apply(_ snapshot: PlaybackSnapshot) {
        let wasPending = playback?.subtitlesPending == true
        let wasFailed = playback?.phase == "failed"
        if let trouble = PlayerLayout.subtitleTrouble(
            picked: subtitle, old: playback?.notes ?? [], new: snapshot.notes, tracks: snapshot.subtitles
        ) {
            showStatus(trouble.say)
            // Its file played as an empty one: the position stays, and no other version is tried.
            if trouble.off { selectSubtitle(nil) }
        }
        // Every start subtitle the core tried could not be read: said once, naming no source.
        if let words = PlayerLayout.startSubtitleFailedWords(snapshot), PlayerLayout.startSubtitleFailedWords(playback) == nil {
            showStatus(words)
        }
        playback = snapshot
        if PlayerLayout.retryRose(old: lastRetries, new: snapshot.retries) { showTryingAgain() }
        lastRetries = snapshot.retries ?? lastRetries
        if let start = snapshot.start { startWindow.saw(start.haveBytes, at: .now) }
        tryingAgain = PlayerLayout.keepsTryingAgain(showing: tryingAgain, phase: snapshot.phase)
        // The core's start subtitle, once, as if picked, sending nothing.
        // A move on to another source: the old start subtitle and its cues go.
        let step = choices.subtitleStep(attempt: snapshot.attempt, auto: snapshot.autoSubtitle, tracks: snapshot.subtitles)
        if step.drop { selectSubtitle(nil) }
        if let track = step.turnOn {
            startPick(track)
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
            // Every ready ends its start window, logged or not, so the next start measures only itself.
            let startLine = startWindow.line(from: route.tapped)
            if loadedURL == nil, !rendered {
                let ms = Int((ContinuousClock.now - route.tapped) / .milliseconds(1))
                let host = URL(string: url)?.host() ?? "?"
                coreLog.notice("metric playback_ready_ms=\(ms, privacy: .public) host=\(host, privacy: .public) \(startLine, privacy: .public)")
            }
            // The first source starts at the saved position; a failover resumes where the last one stopped.
            var start = loadedURL == nil
                ? sourceStart(stoppedAt: stoppedAt, resumeMs: snapshot.resumeMs, fromStart: route.fromStart)
                : stoppedPosition(pendingStart: pendingStart, position: position)
            stoppedAt = nil
            // Start over holds for the first source only: a later reopen resumes.
            if route.fromStart { route.fromStart = false }
            #if DEBUG
            if loadedURL == nil, let at = DebugHooks.takeSeek() {
                start = at
                #if MARQUEE_WALK
                walkSeekTarget = at
                #endif
            }
            #endif
            #if os(iOS)
            // A play that starts on a hot phone takes its heat step now, before the source starts: a move instead of
            // the load, or the load held.
            let pick = EngineChoice.backend(
                container: playback?.container, audio: playback?.audio ?? [], engine: playback?.engine,
                enabled: EngineChoice.enabledHere)
            if heatCheck(atStart: true, engine: pick == .aether) { return }
            #endif
            // Another source: a stall on the last one ends with it.
            sendStall(stall.interrupted())
            load(url, key: snapshot.key, attempt: snapshot.attempt, at: start)
        case "failed":
            player.pause()
            notice = nil
            // Once per failure, the core's own words for the last source (a walk reads why without the screen).
            if !wasFailed {
                let startLine = startWindow.line(from: route.tapped)
                coreLog.notice("metric playback_failed_ms=\(self.sinceTap, privacy: .public) \(startLine, privacy: .public) why=\(snapshot.attempts.last?.error ?? "none", privacy: .public)")
            }
        default:
            break
        }
    }

    /// `key`: the version this URL plays, for its `duration`; `attempt`: the
    /// core's number for this source, for `failOver`. Neither falls back to
    /// the picked version: an unknown one is not sent.
    private func load(_ url: String, key: String?, attempt: Int64?, at start: Double) {
        guard let address = URL(string: url) else { return }
        // The hero's trailer sound ends with the film's start: back on Home, the next trailer is muted.
        TrailerState.shared.filmStarts()
        let pick = EngineChoice.backend(
            container: playback?.container, audio: playback?.audio ?? [], engine: playback?.engine,
            enabled: EngineChoice.enabledHere)
        if pick == .aether {
            openOnEngine(address, key: key, attempt: attempt, at: start, fellBack: false)
            return
        }
        aether?.stop()
        backend = .avPlayer
        generation += 1
        let generation = generation
        loadedURL = url
        loadedKey = key
        loadedAttempt = attempt
        notice = nil
        itemBag = []
        (pictureShown, loadStart, playingSince) = (false, start, nil)
        // An AirPlay receiver would be handed this URL and ask itself for it:
        // a loopback stream stays on this device (mirroring still works).
        airPlayable = PlayerLayout.airPlayable(on: .avPlayer, address)
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
                    self.core.commandNow(sub, ["type": "duration", "key": key, "durationMs": ms])
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
        if holdPause || covered { holdPause = false } else { player.play() }
    }

    /// The player cannot play this source: the second engine takes it where it is on (plan 2.3), else stop it and
    /// ask the core for the next.
    private func sourceFailed(_ error: Error?) {
        let failure = playerFailure(error)
        if fallBack(why: failure.kind) { return }
        fail(failure, notice: "This source failed: \(failure.error). Trying the next one.")
    }

    /// The sound runs but no frame ever came: never a spinner over a running
    /// audio track. The second engine takes it where it is on, else the core
    /// tries the next version, or says Try again.
    private func pictureFailed() {
        if fallBack(why: "decode") { return }
        fail(PlayerFailure(kind: PlayerLayout.pictureFailureKind(floating: floating), error: "this version's picture cannot be shown on this device"),
             notice: "This version's picture cannot be shown here, trying the next one")
    }

    /// AVPlayer refused the source before its first frame: once, at the same position, the second engine opens it,
    /// before the core is told (plan 2.3). False where the switch is off, after the first frame, or on the engine.
    private func fallBack(why kind: String) -> Bool {
        guard EngineChoice.fallsBack(enabled: EngineChoice.enabledHere, from: backend, firstFrameShown: pictureShown, kind: kind),
              let url = loadedURL, let address = URL(string: url) else { return false }
        let ms = Int((ContinuousClock.now - route.tapped) / .milliseconds(1))
        coreLog.notice("metric engine_fallback_ms=\(ms, privacy: .public) why=\(kind, privacy: .public)")
        let at = stoppedPosition(pendingStart: pendingStart, position: position)
        openOnEngine(address, key: loadedKey, attempt: loadedAttempt, at: at, fellBack: true)
        return true
    }

    /// Plays `address` on the second engine; AVPlayer lets go first (one engine alive at a time).
    private func openOnEngine(_ address: URL, key: String?, attempt: Int64?, at start: Double, fellBack: Bool) {
        player.pause()
        player.replaceCurrentItem(with: nil)
        itemBag = []
        generation += 1
        let generation = generation
        (loadedURL, loadedKey, loadedAttempt) = (address.absoluteString, key, attempt)
        (pictureShown, loadStart, playingSince, notice) = (false, start, nil, nil)
        // The start stays pending until the first frame: the engine's clock begins at 0, and a failure before the
        // picture stops at the start, not there (review apple-aether P2-2).
        pendingStart = start > 1 ? start : nil
        if start > 1 { position = start }
        backend = .aether
        self.fellBack = fellBack
        // The engine's players refuse external playback (patch 2): no AirPlay button left from AVPlayer's source.
        airPlayable = PlayerLayout.airPlayable(on: .aether, address)
        guard let session = aether ?? AetherSession() else {
            fail(PlayerFailure(kind: "other", error: "the second player could not start on this device"), notice: "This source failed. Trying the next one.")
            return
        }
        aether = session
        (audioTracks, audioTrack, textTracks, textTrack, textTimeable, engineCues, engineImages) = ([], nil, [], nil, [], [], [])
        let mine = { [weak self] in self.map { $0.generation == generation } ?? false }
        #if DEBUG
        let muted = DebugHooks.mute
        #else
        let muted = false
        #endif
        // As AVPlayer's load: a source opened under Versions, or again after the server moved, waits paused.
        let autoplay = !(holdPause || covered)
        holdPause = false
        session.load(.url(address), container: playback?.container, at: start, rate: rate, muted: muted, autoplay: autoplay, openWithin: EngineChoice.openWithin(budgetEndsAtMs: playback?.startBudgetEndsAt), events: .init(
            time: { [weak self] time in if mine() { self?.engineTick(time) } },
            duration: { [weak self] seconds in if mine() { self?.engineDuration(seconds, key: key) } },
            state: { [weak self] playing, waiting in if mine() { self?.engineState(playing: playing, waiting: waiting) } },
            firstFrame: { [weak self] in if mine() { self?.engineFirstFrame() } },
            ended: { [weak self] in if mine() { self?.engineEnded() } },
            failed: { [weak self] words in
                guard mine() else { return }
                self?.fail(PlayerFailure(kind: EngineChoice.failureKind(refused: false), error: words), notice: "This source failed: \(words). Trying the next one.")
            },
            refused: { [weak self] words, waited in
                guard mine() else { return }
                self?.fail(PlayerFailure(kind: EngineChoice.failureKind(refused: true, waited: waited), error: words), notice: waited ? "This source did not start in time. Trying the next one." : "This version does not play on this device. Trying the next one.")
            },
            tracks: { [weak self] tracks in if mine() { self?.engineTracks(tracks) } },
            cues: { [weak self] text, images in if mine(), let self { (self.engineCues, self.engineImages) = (text, images) } },
            videoSize: { [weak self] size in if mine() { self?.videoSize = size } }))
        NowPlaying.activate()
    }

    private func engineTick(_ time: Double) {
        if watchPicture() { return }
        guard let now = ticked(time: time, pending: pendingStart) else { return }
        // The overlay reads the engine's cues at each tick (every 0.1 s); no boundary observer.
        position = now
        if playing { sendStall(stall.see(playing: true, waiting: false, position: position)) }
        if duration != publishedDuration { publishNowPlaying() }
        if playing, ContinuousClock.now - lastReport > .seconds(5) { report() }
    }

    private func engineDuration(_ seconds: Double, key: String?) {
        duration = seconds
        // The core looks up skip segments once it knows this version's length.
        guard let key, let sub, durationSent.insert(key).inserted else { return }
        let ms = Int(seconds * 1000)
        core.commandNow(sub, ["type": "duration", "key": key, "durationMs": ms])
    }

    private func engineState(playing: Bool, waiting: Bool) {
        (self.playing, self.waiting) = (playing, waiting)
        // Playing without a pause or a stall: buffering is not a missing picture.
        playingSince = playing ? playingSince ?? .now : nil
        publishNowPlaying()
        if let paused = PlayerLayout.traktPaused(paused: !playing && !waiting, buffering: waiting, was: self.paused) {
            self.paused = paused
            report()
        }
        // The engine's stall reaches the core as AVPlayer's does (plans/play-every-version.md 2e).
        if !covered { sendStall(stall.see(playing: playing, waiting: waiting, position: position)) }
    }

    private func engineFirstFrame() {
        pendingStart = nil
        enginePictures += 1
        firstFrame()
        guard fellBack, let sub, let key = loadedKey else { return }
        fellBack = false
        // The core keeps it for this version on this device: its next play opens here first.
        core.commandNow(sub, ["type": "playedOn", "key": key, "engine": Backend.aether.rawValue])
    }

    /// The file's tracks on the second engine; a forced-only subtitle shows by itself and is not a choice.
    private func engineTracks(_ tracks: AetherSession.Tracks) {
        let (audioArrived, textArrived) = (engineAudioIDs.isEmpty && !tracks.audio.isEmpty, engineTextIDs.isEmpty)
        engineAudioIDs = tracks.audio.map(\.id)
        audioTracks = tracks.audio.enumerated().map { index, track in
            Self.engineName(track, index: index, of: tracks.audio.count, audio: true)
        }
        audioTrack = tracks.activeAudio.flatMap { engineAudioIDs.firstIndex(of: $0) }
        let text = tracks.text.filter { !$0.isForced }
        engineTextIDs = text.map(\.id)
        textTracks = text.enumerated().map { index, track in
            Self.engineName(track, index: index, of: text.count, audio: false)
        }
        (audioTrackLanguages, audioDefault, textTrackLanguages) = AetherSession.pickerTags(audio: tracks.audio, text: text)
        // Bitmap tracks draw as images (`subtitleImages`) at the file's timing, as text tracks do.
        textTimeable = text.map { _ in true }
        textTrack = tracks.activeText.flatMap { engineTextIDs.firstIndex(of: $0) }
        // As `loadTracks` does once AVPlayer has them: the picks carried over a reopen, then the file's timing.
        if audioArrived, let index = carriedTracks?.audio {
            carriedTracks?.audio = nil
            selectAudio(index)
        }
        guard textArrived, !text.isEmpty else { return }
        startFile = nil
        if subtitle == nil { subtitleDelay = PlayerLayout.startFileDelay(textTrack: textTrack, kept: outbox.kept) }
        if let index = carriedTracks?.text {
            carriedTracks?.text = nil
            selectText(index)
        }
        // As on AVPlayer: the file's tracks came after the core's start pick, so its language's track inside the
        // file takes over.
        if let auto = subtitle, !choices.viewerChose, auto.url == playback?.autoSubtitle { startPick(auto) }
        #if DEBUG
        if let index = DebugHooks.textTrack { selectText(index) }
        #endif
    }

    /// A track's language name, else its place, as `names` does for AVPlayer's options.
    private static func engineName(_ track: TrackInfo, index: Int, of count: Int, audio: Bool) -> String {
        if let language = track.language, !language.isEmpty, language != "und",
           let name = Locale(identifier: "en").localizedString(forLanguageCode: language) {
            return name
        }
        return TrackPicker.placeName(index: index, count: count, audio: audio)
    }

    private func engineEnded() {
        (ended, playing, waiting) = (true, false, false)
        report()
        if next != nil, autoplayNext, !nextCancelled {
            Task { await playNext() }
        }
    }

    private func fail(_ failure: PlayerFailure, notice words: String) {
        coreLog.error("source failed: \(failure.error, privacy: .public)")
        notice = words
        generation += 1
        aether?.stop()
        backend = .avPlayer
        fellBack = false
        player.pause()
        player.replaceCurrentItem(with: nil)
        // A delay not sent yet goes first, while the failed source's version is the current one.
        flushDelay()
        let command = failOverCommand(failure, attempt: loadedAttempt)
        stoppedAt = stoppedPosition(pendingStart: pendingStart, position: position)
        pendingStart = nil
        (loadedURL, loadedAttempt) = (nil, nil)
        guard let sub else { return }
        core.commandNow(sub, command)
    }

    /// The status line a heat pause leaves until the viewer plays again.
    static let heatPaused = "Paused: the phone is running hot."

    #if os(iOS)
    /// The heat step on every thermal change; `heatCheck` also runs at each ready, so a play that starts on a hot phone
    /// is not left until the next change.
    private func watchHeat() {
        Task { [weak self] in
            for await _ in NotificationCenter.default.notifications(named: ProcessInfo.thermalStateDidChangeNotification) {
                guard let self else { return }
                self.heatCheck(atStart: false, engine: self.backend == .aether)
            }
        }
    }

    /// Once per play, at thermal serious or worse: an engine play above 1080p moves to a cooler listed version, or
    /// pauses when none is listed (`EngineChoice.heatStep`, `heatAction`), and says why on the status line. The first
    /// pick a failover left behind is not a candidate (a move back to it would be refused); a step that does nothing is
    /// not spent. True when it took the start's place (a move), so the caller loads nothing.
    @discardableResult
    private func heatCheck(atStart: Bool, engine: Bool) -> Bool {
        guard !heatHandled else { return false }
        let playingKey = playback?.key ?? route.key
        let rows = (versions?.snapshot.map { [$0.recommended].compactMap { $0 } + $0.rows } ?? [])
            .filter { $0.key == playingKey || $0.key != route.key }
        let step = EngineChoice.heatStep(
            hot: ProcessInfo.processInfo.thermalState.rawValue >= ProcessInfo.ThermalState.serious.rawValue,
            on: EngineChoice.platform, engine: engine, playing: playingKey,
            rows: rows.map { ($0.key, $0.quality) })
        switch EngineChoice.heatAction(step, atStart: atStart, playing: playing, waiting: waiting) {
        case .none:
            return false
        case .move(let key):
            heatHandled = true
            let quality = rows.first { $0.key == key }?.quality ?? "a lighter version"
            coreLog.notice("metric heat_step move to=\(key, privacy: .public)")
            switchVersion(key)
            showStatus("Moved to \(quality) to keep the phone cool.")
            return atStart
        case .holdStart:
            heatHandled = true
            coreLog.notice("metric heat_step hold")
            holdPause = true
            showStatus(Self.heatPaused, untilPlay: true)
            return false
        case .pause:
            heatHandled = true
            coreLog.notice("metric heat_step pause")
            if backend == .aether, let aether { aether.pause() } else { player.pause() }
            report(paused: true)
            showStatus(Self.heatPaused, untilPlay: true)
            return false
        }
    }


    #endif

    /// For the model's lifetime: the player goes with it, so nothing removes these.
    private func observePlayer() {
        timeObserver = player.addPeriodicTimeObserver(forInterval: CMTime(value: 1, timescale: 4), queue: .main) { [weak self] time in
            MainActor.assumeIsolated { self?.tick(time) }
        }
        player.publisher(for: \.timeControlStatus)
            .receive(on: DispatchQueue.main)
            .sink { [weak self] status in
                MainActor.assumeIsolated {
                    // AVPlayer's own state only: on the second engine it sits empty and paused.
                    guard let self, self.backend == .avPlayer else { return }
                    self.playing = status == .playing
                    self.waiting = status == .waitingToPlayAtSpecifiedRate
                    // Versions over the player paused it: the stall stays as it was until Back.
                    if !self.covered { self.sendStall(self.stall.see(playing: self.playing, waiting: self.waiting, position: self.position)) }
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
        // AVPlayer's clock only: the second engine reports through `engineTick`.
        guard time.isNumeric, backend == .avPlayer else { return }
        if let now = ticked(time: time.seconds, pending: pendingStart) { position = now }
        if playing { sendStall(stall.see(playing: true, waiting: false, position: position)) }
        // A track delivered whole (HLS) sends nothing more: the observer moves on with the playhead.
        if position > edgesUntil { watchCueEdges() }
        if watchPicture() { return }
        if let total = player.currentItem?.duration, total.isNumeric { duration = total.seconds }
        if duration != publishedDuration { publishNowPlaying() }
        if playing, ContinuousClock.now - lastReport > .seconds(5) { report() }
    }

    /// Sound runs but no frame came, on either engine: the source fails over (`pictureFailed`). True when it did.
    private func watchPicture() -> Bool {
        let watched = PlayerLayout.pictureWatched(floating: floating, windowWatches: windowWatchesFrames(), onScreen: appOnScreen)
        (loadStart, playingSince) = PlayerLayout.pictureClocks(
            watched: watched, position: position, loadStart: loadStart, playingSince: playingSince, now: .now)
        guard loadedURL != nil, PlayerLayout.pictureMissing(
            shown: pictureShown, advanced: position - loadStart,
            playingFor: playingSince.map { (ContinuousClock.now - $0) / .seconds(1) } ?? 0,
            watched: watched
        ) else { return false }
        pictureFailed()
        return true
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
        aether?.pause()
    }

    private func stop() {
        flushDelay()
        report()
        nowPlaying.stop()
        NowPlaying.deactivate()
        itemBag = []
        player.pause()
        player.replaceCurrentItem(with: nil)
        aether?.stop()
        backend = .avPlayer
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
