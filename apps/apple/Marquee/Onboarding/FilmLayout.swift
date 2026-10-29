import SwiftUI

/// First run B4 (the owner's pick of 2026-10-28, design/DECISION.md): the question steps are a film frame on
/// black, a 2.39:1 frame edge to edge while the height the header and the bar leave allows it, else that height
/// at 2.39:1; never under 40 percent of the screen's height (the page scrolls then). The frame and the bar under
/// it sit in the middle of the space under the header, the gaps above and below alike.
enum FilmRules {
    static let ratio: CGFloat = 2.39
    static let floor: CGFloat = 0.4
    /// The player's subtitle scrim behind each line of the question: its white words keep 4.5:1 over a white still.
    static let scrim = 0.6
    static let steps = 4

    /// The frame in a `width` wide space `room` tall whose bar is `bar` tall, with `edge` above and below.
    static func frame(width: CGFloat, room: CGFloat, bar: CGFloat, edge: CGFloat, floor: CGFloat) -> CGSize {
        let height = max(room - bar - 2 * edge, floor)
        let w = min(width, height * ratio)
        return CGSize(width: w, height: w / ratio)
    }

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
}

/// The frame (the first subview) and its bar (the second), laid out by `FilmRules` in a space `room` tall.
struct FilmLayout: Layout {
    let room: CGFloat
    let floor: CGFloat
    let edge: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? 0
        return CGSize(width: width, height: max(content(width, subviews).total, room))
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        guard subviews.count == 2 else { return }
        let (frame, bar, total) = content(bounds.width, subviews)
        let top = bounds.minY + edge + max(0, bounds.height - total) / 2
        subviews[0].place(at: CGPoint(x: bounds.midX - frame.width / 2, y: top), proposal: ProposedViewSize(frame))
        subviews[1].place(at: CGPoint(x: bounds.minX, y: top + frame.height), proposal: ProposedViewSize(width: bounds.width, height: bar))
    }

    private func content(_ width: CGFloat, _ subviews: Subviews) -> (frame: CGSize, bar: CGFloat, total: CGFloat) {
        guard subviews.count == 2 else { return (.zero, 0, 0) }
        let bar = subviews[1].sizeThatFits(ProposedViewSize(width: width, height: nil)).height
        let frame = FilmRules.frame(width: width, room: room, bar: bar, edge: edge, floor: floor)
        return (frame, bar, edge + frame.height + bar + edge)
    }
}

/// The still every framed step shows (`firstRunStill`), asked once per app run: the core's "none" (offline, past
/// its 1.5 s budget) is kept; an ask that failed (the core not ready yet) is asked again by the next step.
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
