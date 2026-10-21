import SwiftUI

/// The backdrop's scrims as numbers, the one source for the hero and the title page and for the
/// glass check (ThemeColorTests): a stop is a location along its gradient (0 at the start edge) and
/// how much of the page color covers the art there. Over white art, a secondary's white label on
/// its 16 percent fill must keep 4.5:1 wherever a secondary row sits.
enum BackdropStops {
    struct Stop: Sendable, Equatable {
        let at: Double
        let alpha: Double
    }

    /// Home hero, wide: from the leading edge, and from the bottom up.
    static let heroSide = [Stop(at: 0, alpha: 0.94), Stop(at: 0.28, alpha: 0.72), Stop(at: 0.58, alpha: 0.18), Stop(at: 0.72, alpha: 0)]
    // Deepened in 1b (the glass check): the Mac hero at its 400 pt floor held In Library at 4.36.
    static let heroFoot = [Stop(at: 0, alpha: 0.92), Stop(at: 0.34, alpha: 0.4), Stop(at: 0.6, alpha: 0)]
    /// Home hero, phone: from the bottom up, and a band under the wordmark from the top down
    /// (72 pt deep, gone by 140 pt), so the white mark reads on bright art too.
    static let heroPhoneFoot = [Stop(at: 0, alpha: 0.96), Stop(at: 0.38, alpha: 0.55), Stop(at: 0.62, alpha: 0)]
    static func heroPhoneTop(height: Double) -> [Stop] {
        [Stop(at: 0, alpha: 0.78), Stop(at: 72 / height, alpha: 0.6), Stop(at: 140 / height, alpha: 0)]
    }

    /// Title page, wide: from the leading edge, and from the top down; on a phone only the second,
    /// darker at the top under the status bar.
    static let titleSide = [Stop(at: 0, alpha: 0.95), Stop(at: 0.42, alpha: 0.6), Stop(at: 0.75, alpha: 0)]
    /// Deepened in 1b (the glass check): Start over beside Resume read 3.0:1 on a 913 pt Mac window.
    static func titleFoot(wide: Bool) -> [Stop] {
        [Stop(at: 0, alpha: wide ? 0 : 0.6), Stop(at: 0.25, alpha: 0), Stop(at: 0.5, alpha: 0.62), Stop(at: 0.82, alpha: 0.88), Stop(at: 1, alpha: 1)]
    }

    /// How much page color covers the art at `t`, linear between stops.
    static func alpha(_ stops: [Stop], at t: Double) -> Double {
        guard let first = stops.first, let last = stops.last else { return 0 }
        if t <= first.at { return first.alpha }
        if t >= last.at { return last.alpha }
        let i = stops.firstIndex { $0.at >= t }!
        let (a, b) = (stops[i - 1], stops[i])
        return a.alpha + (b.alpha - a.alpha) * (t - a.at) / (b.at - a.at)
    }

    /// Two scrims over each other: what is left of the art is what both leave.
    static func covered(_ a: Double, _ b: Double) -> Double { 1 - (1 - a) * (1 - b) }

    static func gradient(_ stops: [Stop], _ page: Color) -> [Gradient.Stop] {
        stops.map { Gradient.Stop(color: page.opacity($0.alpha), location: $0.at) }
    }
}

/// A secondary's fill over art: white at `rest`, one step lighter while it holds a state (In
/// Library, Watched). Its label is pure white, never the 92 percent `text` (design/DECISION.md).
enum SecondaryFillLevel {
    static let rest = 0.16
    static let held = 0.2
}
