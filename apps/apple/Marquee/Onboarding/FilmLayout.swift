import SwiftUI

/// First run V1, Full Bleed (the owner's pick of 2026-10-30, design/exploration/onboarding-v2, design/DECISION.md):
/// one film still fills the screen behind each question step. The phone stacks the question, its line and the
/// choices at the foot on a ramp anchored to them; wide screens keep them in a column at the lower leading corner
/// over Apple's wash (from the leading edge, from the foot, under the header).
enum FilmRules {
    typealias Stop = BackdropStops.Stop

    static let steps = 4

    /// Wide: the column's leading inset (the Mac's and iPad's padding, the TV's safe area side).
    static let inset: CGFloat = 80

    /// Wide: the wash from the leading edge, laid along `BackdropStops.washAngle` as the web's `80deg` lays it in
    /// a `width` by `height` screen. Its 86 percent stop sits at half the line or 60 points past the column's
    /// trailing edge, whichever is further, so a narrow window keeps its lead at 4.5:1; none 28 percent after.
    static func side(width: CGFloat, height: CGFloat, tv: Bool) -> [Stop] {
        let angle = BackdropStops.washAngle * .pi / 180
        let line = Double(width) * abs(sin(angle)) + Double(height) * abs(cos(angle))
        let at = line > 0 ? max(0.5, Double(inset + column(tv: tv) + 60) / line) : 0.5
        let stops = [Stop(at: 0, alpha: 0.92), Stop(at: at, alpha: 0.86), Stop(at: at + 0.28, alpha: 0)]
        // A gradient's stops end at 1: past it, the ramp is cut where it is.
        guard at + 0.28 > 1 else { return stops }
        return [stops[0], Stop(at: min(at, 1), alpha: 0.86), Stop(at: 1, alpha: BackdropStops.alpha(stops, at: 1))]
    }

    /// Wide: the wash from the foot up.
    static let foot = [Stop(at: 0, alpha: 0.7), Stop(at: 0.4, alpha: 0)]
    /// Wide: the wash under the header, in shares of the screen's height from its top.
    static let top = [Stop(at: 0, alpha: 0.74), Stop(at: 0.2, alpha: 0.66), Stop(at: 0.36, alpha: 0)]
    /// The phone's wash under the header, `phoneTopReach` points from the screen's top (the status bar's
    /// included): 66 percent or more down to 120, where the header's words end.
    static let phoneTopReach: CGFloat = 220
    static let phoneTop = [Stop(at: 0, alpha: 0.72), Stop(at: 120 / 220, alpha: 0.66), Stop(at: 1, alpha: 0)]

    /// The phone's ramp behind the step's block: clear `above` points over the question's top, black 82 percent
    /// at its top line, solid black `solid` points under it and down to the foot.
    static let above: CGFloat = 170
    static let solid: CGFloat = 135
    static let atQuestion = 0.82

    /// The ramp's stops in a layer `height` tall that starts `above` over the question's top.
    static func ramp(height: CGFloat) -> [Stop] {
        guard height > 0 else { return [Stop(at: 0, alpha: 1)] }
        let at = min(Double(above / height), 1)
        let black = min(Double((above + solid) / height), 1)
        return [Stop(at: 0, alpha: 0), Stop(at: at, alpha: atQuestion), Stop(at: black, alpha: 1), Stop(at: 1, alpha: 1)]
    }

    /// The wide column's width: the gallery's 580 on the Mac and iPad, about 840 on the TV's canvas.
    static func column(tv: Bool) -> CGFloat { tv ? 840 : 580 }

    /// The first-run steps the header counts ("Step 2 of 4"); the others (sign-in, the setup check) count none.
    enum Place { case doors, defaults, question, subtitles, other }

    static func count(_ place: Place) -> Int? {
        switch place {
        case .doors: 1
        case .defaults: 2
        case .question: 3
        case .subtitles: 4
        case .other: nil
        }
    }

    /// Back shows from step 2.
    static func hasBack(_ count: Int?) -> Bool { (count ?? 0) >= 2 }

    /// The Mac's first run fills the window (the still runs edge to edge): its sheet is the window's size, never
    /// under the sheet's floor.
    static let sheetFloor = CGSize(width: 760, height: 640)

    static func macSheet(window: CGSize) -> CGSize {
        CGSize(width: max(window.width, sheetFloor.width), height: max(window.height, sheetFloor.height))
    }
}

/// The still every framed step shows (`firstRunStill`), asked once per app run: the core's "none" (offline, past
/// its budget) is kept; an ask that failed (the core not ready yet) is asked again by the next step.
@MainActor final class StillOnce {
    static let shared = StillOnce()
    private var asked: Task<FirstRunStill?, any Error>?

    func get(_ fetch: @escaping @Sendable () async throws -> FirstRunStill?) async -> FirstRunStill? {
        let task = asked ?? Task { try await fetch() }
        asked = task
        do {
            return try await task.value
        } catch {
            if asked == task { asked = nil }
            return nil
        }
    }
}
