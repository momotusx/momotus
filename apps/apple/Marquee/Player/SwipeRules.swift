import CoreGraphics
import Foundation

/// The player's level swipes on iPhone and iPad (plans/player-gestures.md 3): a vertical swipe on the left half sets
/// the brightness, on the right half the volume. Pure, with nothing from UIKit, so the Mac's tests run it.
enum SwipeRules {
    enum Side: Equatable { case brightness, volume }

    /// The bands along the edges where a touch is the system's.
    struct Bands: Equatable {
        var left: CGFloat
        var top: CGFloat
        var right: CGFloat
        var bottom: CGFloat
    }

    /// 24 from either side; at the top Notification Center and Control Center, at the foot the home indicator.
    static func bands(safeTop: CGFloat, safeBottom: CGFloat) -> Bands {
        Bands(left: 24, top: max(32, safeTop), right: 24, bottom: max(32, safeBottom + 8))
    }

    /// The side a swipe starting at `start` sets, by the start point alone; nil in a band.
    static func side(start: CGPoint, size: CGSize, bands: Bands) -> Side? {
        if start.x < bands.left || start.x > size.width - bands.right { return nil }
        if start.y < bands.top || start.y > size.height - bands.bottom { return nil }
        return start.x < size.width / 2 ? .brightness : .volume
    }

    /// True once the touch has gone `threshold` up or down at least twice as far as across (VLC's 2:1), false once
    /// it has gone `threshold` across first, nil while undecided. iOS asks at the pan recogniser's own slop, so it
    /// passes 0 and only the 2:1 decides.
    static func commits(dx: CGFloat, dy: CGFloat, threshold: CGFloat) -> Bool? {
        if abs(dy) >= threshold, abs(dy) >= 2 * abs(dx) { return true }
        if abs(dx) >= threshold { return false }
        return nil
    }

    /// The level after a drag of `dy` from where the swipe began: 80 percent of `height` sweeps the whole range.
    static func level(start: Double, dy: CGFloat, height: CGFloat) -> Double {
        min(max(start - Double(dy / (sweep * height)), 0), 1)
    }

    /// A swipe just reached either end: the one moment for a haptic tick.
    static func reachedEnd(before: Double, after: Double) -> Bool {
        before != after && (after == 0 || after == 1)
    }

    /// Whether the picture takes swipes now: across on an iPhone (upright the picture is a small box over the
    /// controls), any way on an iPad, with only the picture showing (a stall mid-play keeps them), and nothing over
    /// it, never with VoiceOver or Switch Control.
    static func active(
        phone: Bool, across: Bool, status: PlayerStatus, endCard: Bool, pickerOpen: Bool, versions: Bool,
        floating: Bool, assistive: Bool
    ) -> Bool {
        guard !phone || across else { return false }
        guard status == .none || status == .waiting(words: false) else { return false }
        return !endCard && !pickerOpen && !versions && !floating && !assistive
    }

    /// The volume read back after the first set moved at least half way: no slider, or a route that keeps its own
    /// volume (an AirPlay speaker), leaves it where it was.
    static func volumeTook(before: Float, set: Float, after: Float) -> Bool {
        abs(after - before) >= abs(set - before) / 2
    }

    /// A volume change the swipe made itself, not the hardware buttons.
    static func isEcho(value: Float, lastSet: Float?) -> Bool {
        guard let lastSet else { return false }
        return abs(value - lastSet) <= 0.01
    }

    /// The screen still shows the brightness the player set: nobody changed it since.
    static func shouldRestore(current: CGFloat, set: CGFloat) -> Bool {
        abs(current - set) <= 0.02
    }

    /// The brightness to put back on close, taken at a swipe's start: the one found before the first swipe, unless
    /// the viewer changed the screen since the player last set it, which makes theirs the one to keep.
    static func found(current: CGFloat, set: CGFloat?, found: CGFloat?) -> CGFloat {
        guard let found, let set, shouldRestore(current: current, set: set) else { return current }
        return found
    }

    /// What a brightness return does (3.5): put `found` back while the screen still shows what the player set; forget it
    /// once the viewer changed the screen or nothing was swiped; keep it, record included, while no screen can be read.
    enum BrightnessReturn: Equatable { case forget, put(CGFloat), keep }

    static func brightnessReturn(found: CGFloat?, set: CGFloat?, current: CGFloat?) -> BrightnessReturn {
        guard let found, let set else { return .forget }
        guard let current else { return .keep }
        return shouldRestore(current: current, set: set) ? .put(found) : .forget
    }

    /// The screen read back after a put shows it while a scene is active: only then is the record cleared. A write
    /// from a scene in the background can be ignored, and the getter may still echo the written value, so a read-back
    /// there proves nothing; the record then repairs it later.
    static func took(brightness readBack: CGFloat, put: CGFloat, active: Bool) -> Bool {
        active && shouldRestore(current: readBack, set: put)
    }

    /// The volume read-back runs on a set at least one hardware step (1/16) from where the play's sets began: a first
    /// set a few thousandths away can round to the same value and would look as if it did not take.
    static func checksVolume(before: Float, set: Float) -> Bool {
        abs(set - before) >= 1.0 / 16
    }

    /// The share of the height a swipe travels for the whole range (VLC's factor 1.25).
    private static let sweep: CGFloat = 0.8
}
