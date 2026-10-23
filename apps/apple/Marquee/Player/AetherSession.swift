import AetherEngine
import Combine
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
        /// The selected file subtitle's text cues around the playhead (images are not drawn here).
        var cues: ([WebVTT.Cue]) -> Void
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

    /// Opens `url` at `start` (seconds); `openWithin` bounds the open to its first frame (what the start budget has
    /// left, plan 2.3), past which the source counts as failed.
    func load(_ url: URL, at start: Double, muted: Bool, openWithin: Duration, events: Events) {
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
                    default: events.state(state == .playing && !buffering, state == .loading || buffering)
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
            .sink { cues in MainActor.assumeIsolated { if mine() { events.cues(cues.compactMap(Self.cue)) } } }
            .store(in: &bag)
        // Where the engine's start goes (plans/apple-second-engine.md 2.3), logged at the first frame: the probe
        // (open and find_stream_info), the session (route and remux set up), then the first frame (for a remuxed
        // source, its first segment's bytes).
        let loaded = ContinuousClock.now
        var passed: [StartupCheckpoint: ContinuousClock.Instant] = [:]
        var logged = false
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
                    coreLog.notice("metric engine_start probe_ms=\(ms(loaded, probed), privacy: .public) session_ms=\(ms(probed, session), privacy: .public) frame_ms=\(ms(session, .now), privacy: .public)")
                }
            }
            .store(in: &bag)
        let engine = engine
        opening = Task { [weak self] in
            do {
                try await engine.load(url: url, startPosition: start > 1 ? start : nil, options: Self.options)
                engine.play()
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
    /// the open bound). Video and audio resolve in the first packets; a PGS track that does not is left out (the
    /// overlay draws text only).
    static var options: LoadOptions {
        var options = LoadOptions()
        options.probesize = 8 << 20
        return options
    }

    /// A text cue as the overlay draws it; a bitmap cue is not text.
    static func cue(_ cue: SubtitleCue) -> WebVTT.Cue? {
        switch cue.body {
        case .text(let text): WebVTT.Cue(start: cue.startTime, end: cue.endTime, text: text)
        case .richText(let runs): WebVTT.Cue(start: cue.startTime, end: cue.endTime, text: runs.map(\.text).joined())
        case .image: nil
        }
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
