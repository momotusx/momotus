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

    /// Opens `url` at `start` (seconds); `openWithin` bounds the open (the 20 s of plan 2.3, less when the start
    /// budget has less left), past which the source counts as failed.
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
        engine.$hasFirstFrameReadyForDisplay
            .receive(on: DispatchQueue.main)
            .sink { shown in MainActor.assumeIsolated { if mine(), shown { events.firstFrame() } } }
            .store(in: &bag)
        let engine = engine
        opening = Task { [weak self] in
            do {
                try await engine.load(url: url, startPosition: start > 1 ? start : nil)
                engine.play()
            } catch is CancellationError {
            } catch {
                if self?.load == load { events.failed(error.localizedDescription) }
            }
        }
        Task { [weak self] in
            try? await Task.sleep(for: openWithin)
            guard let self, self.load == load, !engine.isSessionReady else { return }
            self.stop()
            events.failed("the second player did not open this source within \(Int(openWithin / .seconds(1))) s")
        }
    }

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
