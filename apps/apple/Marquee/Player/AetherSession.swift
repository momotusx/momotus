import AetherEngine
import Combine
import CoreGraphics
import Foundation

/// The second player engine behind `PlayerModel` (plans/apple-second-engine.md 2.4): it owns the vendored
/// AetherEngine, its subscriptions and its view, and tells the model what happens through `Events`. One engine is
/// alive at a time: the model stops this one before AVPlayer plays again, and the other way round.
@MainActor
final class AetherSession {
    /// What the model hears, each on the main actor, only for the load it asked for.
    struct Events {
        var time: (Double) -> Void
        var duration: (Double) -> Void
        /// Playing, and waiting for data.
        var state: (_ playing: Bool, _ waiting: Bool) -> Void
        var firstFrame: () -> Void
        var ended: () -> Void
        /// The engine could not play the source, in its own words; the model sends it to the core.
        var failed: (String) -> Void
        /// The file's audio and subtitle tracks, and the one each plays.
        var tracks: (Tracks) -> Void
        /// The selected file subtitle's cues so far: text, and bitmaps (PGS, DVD, DVB) drawn as images.
        var cues: ([WebVTT.Cue], [ImageCue]) -> Void
        /// The video's coded size, once the source is open: bitmap cues are placed on it.
        var videoSize: (CGSize) -> Void
    }

    /// A bitmap subtitle: its image and where it sits, [0, 1] against the video frame.
    struct ImageCue: @unchecked Sendable {
        let start: Double
        let end: Double
        let image: CGImage
        let position: CGRect
    }

    /// The file's tracks as the engine lists them.
    struct Tracks {
        var audio: [TrackInfo]
        var activeAudio: Int?
        var text: [TrackInfo]
        var activeText: Int?
    }

    let engine: AetherEngine
    private var bag: Set<AnyCancellable> = []
    private var opening: Task<Void, Never>?
    /// One more per load: a late answer from an earlier load says nothing.
    private var load = 0

    /// Nil when the engine cannot start on this device.
    init?() {
        guard let engine = try? AetherEngine() else { return nil }
        self.engine = engine
    }

    /// Opens `source` at `start` (seconds) at the viewer's `rate`, which a load of another source does not keep;
    /// `openWithin` bounds the open to its first frame (what the start budget has left, plan 2.3), past which the
    /// source counts as failed.
    func load(
        _ source: MediaSource, container: String?, at start: Double, rate: Float, muted: Bool, autoplay: Bool = true,
        openWithin: Duration, events: Events
    ) {
        stop()
        load += 1
        let load = load
        engine.volume = muted ? 0 : 1
        let mine = { [weak self] in self?.load == load }
        engine.clock.$currentTime
            .receive(on: DispatchQueue.main)
            .sink { time in MainActor.assumeIsolated { if mine() { events.time(time) } } }
            .store(in: &bag)
        engine.$duration
            .receive(on: DispatchQueue.main)
            .sink { duration in MainActor.assumeIsolated { if mine(), duration > 0 { events.duration(duration) } } }
            .store(in: &bag)
        engine.$state.combineLatest(engine.$isBuffering)
            .receive(on: DispatchQueue.main)
            .sink { state, buffering in
                MainActor.assumeIsolated {
                    guard mine() else { return }
                    switch state {
                    case .ended: events.ended()
                    case .error(let message): events.failed(message)
                    default:
                        let (playing, waiting) = Self.playState(state, buffering: buffering)
                        events.state(playing, waiting)
                    }
                }
            }
            .store(in: &bag)
        engine.$audioTracks.combineLatest(engine.$activeAudioTrackIndex, engine.$subtitleTracks, engine.$activeSubtitleTrackIndex)
            .receive(on: DispatchQueue.main)
            .sink { audio, activeAudio, text, activeText in
                MainActor.assumeIsolated {
                    if mine() { events.tracks(Tracks(audio: audio, activeAudio: activeAudio, text: text, activeText: activeText)) }
                }
            }
            .store(in: &bag)
        engine.$subtitleCues
            .receive(on: DispatchQueue.main)
            .sink { cues in
                MainActor.assumeIsolated { if mine() { events.cues(cues.compactMap(Self.cue), cues.compactMap(Self.image)) } }
            }
            .store(in: &bag)
        // Where the engine's start goes (plans/apple-second-engine.md 2.3), logged at the first frame: the probe
        // (open and find_stream_info), the session (route and remux set up), then the first frame (for a remuxed
        // source, its first segment's bytes).
        let loaded = ContinuousClock.now
        var passed: [StartupCheckpoint: ContinuousClock.Instant] = [:]
        var logged = false
        // Matroska carries its codec parameters in its header: 1 MiB first, 8 MiB once more only when what plays
        // came back without its parameters.
        var probeMiB: Int64 = Self.firstProbeMiB(container: container)
        engine.$startupProgress
            .receive(on: DispatchQueue.main)
            .sink { progress in
                MainActor.assumeIsolated {
                    guard mine(), let progress, passed[progress.checkpoint] == nil else { return }
                    passed[progress.checkpoint] = .now
                }
            }
            .store(in: &bag)
        engine.$hasFirstFrameReadyForDisplay
            .receive(on: DispatchQueue.main)
            .sink { shown in
                MainActor.assumeIsolated {
                    guard mine(), shown else { return }
                    defer { events.firstFrame() }
                    guard !logged else { return }
                    logged = true
                    let ms = { (from: ContinuousClock.Instant?, to: ContinuousClock.Instant?) in
                        from.flatMap { from in to.map { "\(Int(($0 - from) / .milliseconds(1)))" } } ?? "n/a"
                    }
                    let (probed, session) = (passed[.streamsProbed], passed[.sessionConstructed])
                    coreLog.notice("metric engine_start probe_ms=\(ms(loaded, probed), privacy: .public) session_ms=\(ms(probed, session), privacy: .public) frame_ms=\(ms(session, .now), privacy: .public) probe_mib=\(probeMiB, privacy: .public)")
                }
            }
            .store(in: &bag)
        let engine = engine
        opening = Task { [weak self] in
            do {
                var probe = try await engine.load(source: source, startPosition: start > 1 ? start : nil, options: Self.options(probeMiB: probeMiB))
                guard self?.load == load else { return }
                if probeMiB < 8, !Self.resolved(video: probe.map { CGSize(width: Int($0.videoWidth), height: Int($0.videoHeight)) }, audio: engine.audioTracks, active: engine.activeAudioTrackIndex) {
                    (probeMiB, passed) = (8, [:])
                    probe = try await engine.load(source: source, startPosition: start > 1 ? start : nil, options: Self.options(probeMiB: probeMiB))
                }
                // A stop or another load while this one opened: it does not play.
                guard self?.load == load else { return }
                if let probe, probe.videoWidth > 0, probe.videoHeight > 0 {
                    events.videoSize(CGSize(width: Int(probe.videoWidth), height: Int(probe.videoHeight)))
                }
                if rate != 1 { engine.setRate(rate) }
                if autoplay { engine.play() }
            } catch is CancellationError {
            } catch {
                if self?.load == load { events.failed(error.localizedDescription) }
            }
        }
        Task { [weak self] in
            try? await Task.sleep(for: openWithin)
            guard let self, self.load == load, !engine.hasFirstFrameReadyForDisplay else { return }
            self.stop()
            events.failed("the second player did not start this source in time")
        }
    }

    /// The probe reads 8 MiB, not the engine's 50: a sparse track with no early packet (a PGS subtitle) makes
    /// find_stream_info read to the budget, which on a torrent goes at the swarm's pace (20 s on a walk, cut by
    /// the open bound). A Matroska file (mkv, webm) carries its codec parameters in its Tracks header, so it starts
    /// at 1 MiB and 2 s of analysis; a track left without its parameters is placed when its first packet arrives.
    static func options(probeMiB: Int64) -> LoadOptions {
        var options = LoadOptions()
        options.probesize = probeMiB << 20
        if probeMiB < 8 { options.maxAnalyzeDuration = 2_000_000 }
        return options
    }

    static func firstProbeMiB(container: String?) -> Int64 {
        ["mkv", "webm"].contains(container?.lowercased() ?? "") ? 1 : 8
    }

    /// Whether the probe came back with what plays: the picture's size, and the playing audio's rate and channels.
    /// No probe (the engine reports none on some paths) counts as resolved: there is nothing to check. The 2 s of
    /// analysis can leave the frame rate or interlacing unknown for a file with no DefaultDuration; this does not
    /// look at them (plan 2.3).
    static func resolved(video: CGSize?, audio: [TrackInfo], active: Int?) -> Bool {
        guard let video else { return true }
        guard video.width > 0, video.height > 0 else { return false }
        guard let track = audio.first(where: { $0.id == active }) ?? audio.first else { return true }
        return track.sampleRate > 0 && track.channels > 0
    }

    /// Playing, and waiting for data, from the engine's state: `isBuffering` is its mid-play stall (its inner
    /// player waiting to keep up), the one wait after the first frame that shows the spinner; `.loading` is the
    /// open, under the loading screen.
    static func playState(_ state: PlaybackState, buffering: Bool) -> (playing: Bool, waiting: Bool) {
        (state == .playing && !buffering, state == .loading || buffering)
    }

    /// A text cue as the overlay draws it; a bitmap cue is not text.
    static func cue(_ cue: SubtitleCue) -> WebVTT.Cue? {
        switch cue.body {
        case .text(let text): WebVTT.Cue(start: cue.startTime, end: cue.endTime, text: text)
        case .richText(let runs): WebVTT.Cue(start: cue.startTime, end: cue.endTime, text: runs.map(\.text).joined())
        case .image: nil
        }
    }

    /// A bitmap cue as the overlay draws it; a text cue is not one.
    static func image(_ cue: SubtitleCue) -> ImageCue? {
        guard case .image(let image) = cue.body else { return nil }
        return ImageCue(start: cue.startTime, end: cue.endTime, image: image.cgImage, position: image.position)
    }

    func selectAudio(id: Int) { engine.selectAudioTrack(index: id) }
    func selectText(id: Int) { engine.selectSubtitleTrack(index: id) }
    func clearText() { engine.clearSubtitle() }

    func play() { engine.play() }
    func pause() { engine.pause() }
    func seek(to seconds: Double) async { await engine.seek(to: seconds) }
    func setRate(_ rate: Float) { engine.setRate(rate) }

    /// Ends the session: its events stop, the engine tears down.
    func stop() {
        load += 1
        bag = []
        opening?.cancel()
        opening = nil
        engine.stop()
    }
}
