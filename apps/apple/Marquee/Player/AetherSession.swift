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
        /// The engine sent the source to its software decoders where the platform admits none (`EngineChoice.refuses`);
        /// true when the engine rerouted it there because AVPlayer waited on bytes (`EngineChoice.waited`).
        var refused: (String, _ waited: Bool) -> Void
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
    /// The open's bound, held while a paused load waits (under Versions, after a server move): it runs from the first
    /// play, never while the viewer browses or sits paused.
    private var heldBound: (within: Duration, failed: (String) -> Void)?

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
        // With the window up the engine swaps the item in place; a stop would close the window and end the film.
        if EngineChoice.stopsBeforeLoad(pictureInPictureActive: engine.pictureInPictureActive) { stop() } else { release() }
        load += 1
        let load = load
        engine.volume = muted ? 0 : 1
        let mine = { [weak self] in self?.load == load }
        Self.fresh(engine.clock.$currentTime)
            .sink { time in MainActor.assumeIsolated { if mine() { events.time(time) } } }
            .store(in: &bag)
        Self.fresh(engine.$duration)
            .sink { duration in MainActor.assumeIsolated { if mine(), duration > 0 { events.duration(duration) } } }
            .store(in: &bag)
        Self.fresh(engine.$state.combineLatest(engine.$isBuffering))
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
        Self.fresh(engine.$audioTracks.combineLatest(engine.$activeAudioTrackIndex, engine.$subtitleTracks, engine.$activeSubtitleTrackIndex))
            .sink { audio, activeAudio, text, activeText in
                MainActor.assumeIsolated {
                    if mine() { events.tracks(Tracks(audio: audio, activeAudio: activeAudio, text: text, activeText: activeText)) }
                }
            }
            .store(in: &bag)
        Self.fresh(engine.$subtitleCues)
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
        let host = engine
        // The failure the engine absorbed when it rerouted this load to its software host (AVPlayer gave up), as
        // the engine announced it: a refusal later in the load reads why, not the engine's hosts on a later turn.
        var rerouted: PlaybackErrorInfo?
        Self.recording(engine.softwarePathEscalations) { failure in
            MainActor.assumeIsolated { if mine() { rerouted = failure } }
        }
        .store(in: &bag)
        Self.fresh(engine.$startupProgress)
            .sink { progress in
                MainActor.assumeIsolated {
                    guard mine(), let progress else { return }
                    guard passed[progress.checkpoint] == nil else { return }
                    passed[progress.checkpoint] = .now
                }
            }
            .store(in: &bag)
        Self.fresh(engine.$hasFirstFrameReadyForDisplay)
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
                    coreLog.notice("metric engine_start probe_ms=\(ms(loaded, probed), privacy: .public) session_ms=\(ms(probed, session), privacy: .public) frame_ms=\(ms(session, .now), privacy: .public) probe_mib=\(probeMiB, privacy: .public) path=\(Self.path(native: host.nativePlayerLayer != nil, software: host.softwarePiPSource != nil), privacy: .public) rerouted=\(rerouted == nil ? 0 : 1, privacy: .public)")
                }
            }
            .store(in: &bag)
        // Software video only where the platform admits it (plans/iphone-mkv.md): a session the engine sent to its
        // software host fails here, and the core moves to the next version.
        Self.fresh(engine.$softwarePiPSource)
            .sink { [weak self] source in
                MainActor.assumeIsolated {
                    guard mine(), EngineChoice.refuses(softwareHost: source != nil, on: EngineChoice.platform) else { return }
                    self?.stop()
                    let waited = rerouted.map { EngineChoice.waited(domain: $0.underlyingDomain, code: $0.underlyingCode) } ?? false
                    events.refused(waited ? "AVPlayer did not get this source's first segment in time" : "this device does not decode this picture in software", waited)
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
        if autoplay { armBound(openWithin, failed: events.failed) } else { heldBound = (openWithin, events.failed) }
    }

    /// An engine value for this load only: on the main queue, without the value it holds when this load subscribes.
    /// After a stop that value is a reset one; on an in-place load (the window up) it is the previous source's
    /// duration, first-frame flag, position and state, which must not reach the new one.
    static func fresh<P: Publisher>(_ publisher: P) -> AnyPublisher<P.Output, P.Failure> {
        publisher.dropFirst().receive(on: DispatchQueue.main).eraseToAnyPublisher()
    }

    /// The engine's reroutes, each passed on as the engine sends it (on the main actor, before its rebuild). Not
    /// through `fresh`: an event has no held value to drop, and dropping the first would lose the only one.
    static func recording<P: Publisher>(_ escalations: P, _ record: @escaping (PlaybackErrorInfo) -> Void) -> AnyCancellable
    where P.Output == SoftwarePathEscalationEvent, P.Failure == Never {
        escalations.sink { record($0.absorbedFailure) }
    }

    /// The engine's path for the start log: native (remuxed for AVPlayer), software (its own decoders), or other.
    static func path(native: Bool, software: Bool) -> String {
        native ? "native" : software ? "software" : "other"
    }

    /// Past `within` with no first frame, the source counts as failed.
    private func armBound(_ within: Duration, failed: @escaping (String) -> Void) {
        let load = load
        Task { [weak self] in
            try? await Task.sleep(for: within)
            guard let self, self.load == load, !self.engine.hasFirstFrameReadyForDisplay else { return }
            self.stop()
            failed("the second player did not start this source in time")
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

    /// What the pickers read from the engine's tracks: each one's language tag as the file gives it ("eng"), nil
    /// for none, and the file's default audio.
    nonisolated static func pickerTags(audio: [TrackInfo], text: [TrackInfo]) -> (audio: [String?], audioDefault: Int?, text: [String?]) {
        (audio.map { TrackPicker.tag($0.language) }, audio.firstIndex(where: \.isDefault), text.map { TrackPicker.tag($0.language) })
    }

    func selectAudio(id: Int) { engine.selectAudioTrack(index: id) }
    func selectText(id: Int) { engine.selectSubtitleTrack(index: id) }
    func clearText() { engine.clearSubtitle() }

    func play() {
        if let bound = heldBound {
            heldBound = nil
            armBound(bound.within, failed: bound.failed)
        }
        engine.play()
    }
    func pause() { engine.pause() }
    func seek(to seconds: Double) async { await engine.seek(to: seconds) }
    func setRate(_ rate: Float) { engine.setRate(rate) }

    /// Ends the session: its events stop, the engine tears down.
    func stop() {
        release()
        engine.stop()
    }

    /// Lets go of this load's subscriptions, open and bound, and keeps the engine's player.
    private func release() {
        load += 1
        heldBound = nil
        bag = []
        opening?.cancel()
        opening = nil
    }
}
