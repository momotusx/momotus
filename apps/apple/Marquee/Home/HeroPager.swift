import Foundation
import Observation

/// Home's hero carousel as plain state (plans/appletv-polish.md 4.1): one progress value drives both the active
/// dot's fill and the advance, so a pause or reduced motion can never make them drift. Testable with no view.
@MainActor @Observable
final class HeroPagerModel {
    var count: Int
    /// 0 to 1 through the page's dwell, or through its trailer while one plays.
    private(set) var progress: Double = 0
    /// Paged by hand: no more timed pages this session.
    private(set) var stopped = false
    let dwell: Double

    init(count: Int, dwell: Double = Theme.Motion.heroDwell) {
        self.count = count
        self.dwell = dwell
    }

    /// Advances the clock by `dt` seconds while auto-advance may run; true when the page should change now.
    func tick(_ dt: Double, running: Bool) -> Bool {
        guard running, !stopped, count > 1 else { return false }
        progress = min(1, progress + dt / dwell)
        guard progress >= 1 else { return false }
        progress = 0
        return true
    }

    /// A trailer's place in its cap, written to the same value.
    func trailer(_ fraction: Double) { progress = min(1, max(0, fraction)) }

    /// Any touch, key or pointer move: the dwell starts over.
    func input() { progress = 0 }

    /// The viewer paged: no more timed pages this session.
    func manual() {
        stopped = true
        progress = 0
    }

    /// Whether the dwell may run (decision 7): never while a trailer plays, so the 8 s dwell cannot cut it; that
    /// page moves on by the trailer's end rule instead (the lead, 2026-10-24). `held`: the pointer rests on the
    /// hero, which pauses the clock where it is.
    nonisolated static func runs(visible: Bool, active: Bool, reduceMotion: Bool, trailerPlaying: Bool, held: Bool = false, pages: Int) -> Bool {
        visible && active && !reduceMotion && !trailerPlaying && !held && pages > 1
    }

    /// The next page, wrapping, for the timer.
    func next(after page: Int) -> Int { count > 0 ? (page + 1) % count : 0 }

    /// The page a key asks for, with no wrap: Right on the last and Left on the first do nothing.
    func step(from page: Int, by delta: Int) -> Int? {
        let to = page + delta
        return (0..<count).contains(to) ? to : nil
    }
}
