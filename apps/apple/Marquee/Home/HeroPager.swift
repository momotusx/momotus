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
    nonisolated static func runs(
        visible: Bool, active: Bool, reduceMotion: Bool, trailerPlaying: Bool, held: Bool = false, assistive: Bool = false,
        pages: Int
    ) -> Bool {
        // `assistive`: VoiceOver or Switch Control is on; a page that slides away under its cursor is lost to it.
        visible && active && !reduceMotion && !trailerPlaying && !held && !assistive && pages > 1
    }

    /// The hero is the thing on screen: half of it in view, its place the tab shown, and nothing over it (a pushed
    /// page, the player). Home stays alive behind other tabs, so without this it would page and play there.
    nonisolated static func live(visible: Bool, tabShown: Bool, covered: Bool) -> Bool {
        visible && tabShown && !covered
    }

    /// The next page, wrapping, for the timer.
    func next(after page: Int) -> Int { count > 0 ? (page + 1) % count : 0 }

    /// The page a key asks for, with no wrap: Right on the last and Left on the first do nothing.
    func step(from page: Int, by delta: Int) -> Int? {
        let to = page + delta
        return (0..<count).contains(to) ? to : nil
    }
}

/// Where a seek by `by` seconds from `t` lands in a `d` second trailer: never before its start, and never into its
/// last 2 s, where the player ends it (plans/trailer-inline.md 7).
func trailerSeekTarget(_ t: Double, _ d: Double, by: Double) -> Double {
    max(0, min(t + by, max(0, d - 2)))
}

/// The full-screen trailer's line for the first `n` trailers YouTube refused here and the layer moved past, so the
/// one playing is never a silent swap (the web's `skippedLine`); none when it moved past none.
func trailerSkippedLine(_ n: Int) -> String? {
    guard n > 0 else { return nil }
    let names = (0..<n).map { $0 == 0 ? "Main trailer" : "Trailer \($0 + 1)" }
    let list = names.count == 1 ? names[0] : names.dropLast().joined(separator: ", ") + " and " + names[names.count - 1]
    return "\(list) can't play here."
}

/// The full-screen bar's Slider (plans/trailer-inline.md 7): a drag holds its place until it is let go; any other
/// move (VoiceOver's adjust, the arrow keys) seeks at once.
func trailerSliderMove(_ value: Double, dragging: Bool) -> (held: Double?, seek: Double?) {
    dragging ? (value, nil) : (nil, value)
}

/// Which of the player page's messages belong to the trailer asked for last (review r1, W6): each `play` has a new
/// generation, the page echoes it, and an `end` of the one before, landing after the new `play`, is dropped.
enum TrailerMessages {
    static func accepts(_ message: [String: Any], generation: Int) -> Bool {
        (message["gen"] as? Int) == generation
    }
}

/// Where the hero's foot controls sit: the page dots, and the trailer's sound switch level with them.
enum HeroFoot {
    /// The dots' place in the foot: 7 pt tall with 7 pt above and under them (the control reaches past it).
    static let dotsHeight: CGFloat = 21
    static let switchSize: CGFloat = 36
    static func dots(wide: Bool) -> CGFloat { wide ? 28 : 20 }
    /// The sound switch's foot, so its middle sits level with the dots'.
    static func soundSwitch(wide: Bool) -> CGFloat { dots(wide: wide) + dotsHeight / 2 - switchSize / 2 }
}

/// The page dots' layout (HeroDots): 7 pt dots, the active one a 24 pt pill, 9 pt apart, in one control of at least
/// 44 by 44 pt.
enum PageDots {
    static let dot: CGFloat = 7
    static let pill: CGFloat = 24
    static let gap: CGFloat = 9
    /// The platform's least touch target.
    static let side: CGFloat = 44

    /// The dot a tap `x` pt from the control's leading edge is nearest to; the dots sit in its middle.
    static func nearest(_ x: CGFloat, count: Int, active: Int) -> Int {
        var left = max(0, (side - (pill + CGFloat(count - 1) * (dot + gap))) / 2)
        var best = 0
        var distance = CGFloat.infinity
        for i in 0..<count {
            let width = i == active ? pill : dot
            if abs(left + width / 2 - x) < distance { (best, distance) = (i, abs(left + width / 2 - x)) }
            left += width + gap
        }
        return best
    }
}

/// What the hero draws from the trailer: the clock follows `fraction` while `playing`; the art fades when `shown`.
@MainActor @Observable
final class TrailerState {
    /// The one trailer on screen: the carousel writes it, the header's sound switch reads it.
    static let shared = TrailerState()

    /// A trailer is due on the page shown: its player is mounted under the art until it ends.
    var loading = false
    var playing = false
    var shown = false
    var fraction: Double = 0
    /// A full-screen trailer's place and length in seconds (0 until known), and whether it is paused or waiting.
    var seconds: Double = 0
    var length: Double = 0
    var paused = false
    var buffering = true
    /// The sound switch's choice for the session: off at launch, on only by a press, kept by every later trailer
    /// and page; a film starting in the player turns it off, so coming back from it is never loud.
    var sound = false

    /// A film starts in the player (`PlayerModel.load`, every source of every title).
    func filmStarts() { sound = false }
}
