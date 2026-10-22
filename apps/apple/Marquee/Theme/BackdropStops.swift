import SwiftUI

/// The backdrop's scrims as numbers, the one source for the hero and the title page and for the
/// glass check (ThemeColorTests): a stop is a location along its gradient (0 at the start edge) and
/// how much of its color (the page's, or black for Home's wash and vignette) covers the art there.
/// Over white art, a secondary's white label on its 16 percent fill must keep 4.5:1 wherever a
/// secondary row sits.
enum BackdropStops {
    struct Stop: Sendable, Equatable {
        let at: Double
        let alpha: Double
    }

    /// Home's full-bleed hero, wide (the frames' desktop ramps): the black wash along `washAngle`,
    /// and the page color from the foot up, where it reaches the page.
    static let heroSide = [Stop(at: 0, alpha: 0.74), Stop(at: 0.22, alpha: 0.56), Stop(at: 0.4, alpha: 0.24), Stop(at: 0.56, alpha: 0)]
    static let heroFoot = [Stop(at: 0, alpha: 1), Stop(at: 0.07, alpha: 0.9), Stop(at: 0.2, alpha: 0.72), Stop(at: 0.36, alpha: 0.4), Stop(at: 0.5, alpha: 0.12), Stop(at: 0.6, alpha: 0)]
    /// The wash's direction as CSS gives it (`linear-gradient(80deg, ...)`): 0 points up, 90 to the right.
    static let washAngle = 80.0
    /// Under the wash, the frames' light vignette (`radial-gradient(100% 100% at 66.7% 0%, ...)`): black,
    /// by the distance from `vignetteCenter` as a share of the box's own width and height.
    static let vignette = [Stop(at: 0.4, alpha: 0), Stop(at: 1, alpha: 0.28)]
    static let vignetteCenter = UnitPoint(x: 0.667, y: 0)
    /// The Mac window's narrowest width, half of a 13 inch MacBook's screen so two windows tile; the
    /// glass check reads every Mac spot here.
    static let macMinWidth: CGFloat = 720
    /// The desktop frames' hero, 1440 by 810. A smaller wide hero (a narrow Mac window, an iPad) lays
    /// the wash and the foot out in this box from its own foot and leading edge, so every word and
    /// button at the foot keeps the frame's darkness in points; tvOS keeps its own box (D9).
    static let heroFrame = CGSize(width: 1440, height: 810)
    static func rampBox(_ size: CGSize, held: Bool) -> CGSize {
        held ? CGSize(width: max(size.width, heroFrame.width), height: max(size.height, heroFrame.height)) : size
    }
    /// The wide hero's synopsis column off TV, capped short of the frames' 560 so its first line ends
    /// where the wash still holds it; the glass check reads its end.
    static let heroSynopsisWidth: CGFloat = 480
    /// The foot's stops in a `size` hero laid out in `box`.
    static func heroFoot(_ size: CGSize, box: CGSize) -> [Stop] {
        size.height > 0 ? held(heroFoot, scale: box.height / size.height) : heroFoot
    }
    /// Home's hero on the phone (the frames' iPhone shade): from the foot up into the black page, and
    /// a light wash from the top so the status bar and the large title read on any art.
    static let heroPhoneFoot = [Stop(at: 0, alpha: 1), Stop(at: 0.09, alpha: 0.92), Stop(at: 0.2, alpha: 0.7), Stop(at: 0.33, alpha: 0.4), Stop(at: 0.46, alpha: 0.12), Stop(at: 0.56, alpha: 0)]
    static let heroPhoneTop = [Stop(at: 0, alpha: 0.5), Stop(at: 0.11, alpha: 0.2), Stop(at: 0.2, alpha: 0)]
    /// The foot ramp's stops are shares of the 680 pt portrait hero and hold in points from the foot:
    /// a shorter hero (a landscape phone, down to 280) keeps the same darkness at the same height
    /// above the foot, so Details reads there as it does upright.
    static let heroPhoneHeight = 680.0
    static func heroPhoneFoot(height: Double) -> [Stop] {
        guard height > 0 else { return heroPhoneFoot }
        return held(heroPhoneFoot, scale: heroPhoneHeight / height)
    }

    /// Stops drawn `scale` times as far along their gradient, so they keep their place in points in
    /// a box shorter by that much; the ramp's end is cut at the box's edge.
    static func held(_ stops: [Stop], scale: Double) -> [Stop] {
        let inside = stops.filter { $0.at * scale < 1 }.map { Stop(at: $0.at * scale, alpha: $0.alpha) }
        return inside.count == stops.count ? inside : inside + [Stop(at: 1, alpha: alpha(stops, at: 1 / scale))]
    }

    /// Title page, wide: from the leading edge, and from the top down; on a phone only the second,
    /// darker at the top under the status bar.
    static let titleSide = [Stop(at: 0, alpha: 0.95), Stop(at: 0.42, alpha: 0.6), Stop(at: 0.75, alpha: 0)]
    /// Below a 900 pt window the side ramp keeps that window's points, so Start over beside Resume
    /// reads on a half-screen window as on a 900 pt one.
    static let titleSideWidth = 900.0
    static func titleSide(width: Double) -> [Stop] {
        width > 0 ? held(titleSide, scale: max(1, titleSideWidth / width)) : titleSide
    }
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

    /// The wash's line in a `width` by `height` box, as CSS lays an angled gradient: through the
    /// center, its ends where the perpendiculars through the far corners cross it.
    static func washLine(width: Double, height: Double) -> (start: UnitPoint, end: UnitPoint) {
        guard width > 0, height > 0 else { return (.leading, .trailing) }
        let angle = washAngle * .pi / 180
        let (dx, dy) = (sin(angle), -cos(angle))
        let half = (width * abs(dx) + height * abs(dy)) / 2
        return (UnitPoint(x: 0.5 - dx * half / width, y: 0.5 - dy * half / height),
                UnitPoint(x: 0.5 + dx * half / width, y: 0.5 + dy * half / height))
    }

    /// Where a point (`x`, `y` from the box's top leading corner) falls along the wash's line.
    static func washT(x: Double, y: Double, width: Double, height: Double) -> Double {
        let (start, end) = washLine(width: width, height: height)
        let (sx, sy) = (start.x * width, start.y * height)
        let (ex, ey) = (end.x * width - sx, end.y * height - sy)
        return ((x - sx) * ex + (y - sy) * ey) / (ex * ex + ey * ey)
    }

    /// The vignette's cover at a point given as shares of the box (0 to 1 from the top leading corner).
    static func vignetteAlpha(x: Double, y: Double) -> Double {
        let (dx, dy) = (x - vignetteCenter.x, y - vignetteCenter.y)
        return alpha(vignette, at: (dx * dx + dy * dy).squareRoot())
    }

    /// Where a point of a `size` hero (from its top leading corner) falls along the wash laid out in `box`.
    static func washT(x: Double, y: Double, size: CGSize, box: CGSize) -> Double {
        washT(x: x, y: y + box.height - size.height, width: box.width, height: box.height)
    }

    /// The wash as HeroScrims draws it in a `size` hero, laid out in `box` from the foot and the
    /// leading edge, so the glass check reads what is drawn.
    static func wash(_ size: CGSize, box: CGSize) -> LinearGradient {
        let line = washLine(width: box.width, height: box.height)
        guard size.width > 0, size.height > 0 else { return LinearGradient(stops: gradient(heroSide, .black), startPoint: line.start, endPoint: line.end) }
        let lift = box.height - size.height
        func unit(_ p: UnitPoint) -> UnitPoint { UnitPoint(x: p.x * box.width / size.width, y: (p.y * box.height - lift) / size.height) }
        return LinearGradient(stops: gradient(heroSide, .black), startPoint: unit(line.start), endPoint: unit(line.end))
    }

    /// The phone's foot ramp as HeroView draws it in a `size` box, in the page color.
    static func phoneFoot(_ size: CGSize, _ page: Color) -> LinearGradient {
        LinearGradient(stops: gradient(heroPhoneFoot(height: size.height), page), startPoint: .bottom, endPoint: .top)
    }

    static var vignetteGradient: EllipticalGradient {
        EllipticalGradient(stops: gradient(vignette, .black), center: vignetteCenter, startRadiusFraction: 0, endRadiusFraction: 1)
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
