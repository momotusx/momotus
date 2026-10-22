import AppKit
import Foundation
import SwiftUI
import Testing

// The title hero laid out for real from plain values (plans/design-v3-phase3-apple.md 5.1): where its
// parts sit above the foot, which the glass check's title spots must cover.

@MainActor private final class Parts {
    var frames: [HeroPart: CGRect] = [:]
}

private func details(_ overrides: [String: Any] = [:]) throws -> TitleDetails {
    var json: [String: Any] = [
        "id": "tt5875444", "kind": "series", "name": "Slow Horses",
        "description": "A dysfunctional team of MI5 agents, exiled to Slough House for their career-ending mistakes, find themselves drawn into the heart of a conspiracy that reaches the very top of the service, while their boss, the brilliant and slovenly Jackson Lamb, keeps them in line.",
        "poster": NSNull(), "background": NSNull(), "logo": NSNull(), "releaseInfo": "Since 2022", "runtime": "46 min",
        "genres": ["Drama", "Thriller", "Crime"], "cast": ["Gary Oldman", "Jack Lowden", "Kristin Scott Thomas"],
        "directors": ["James Hawes", "Jeremy Lovering"], "rating": "8.4",
        "ratings": ["imdb": "8.4", "rottenTomatoes": "Fresh"], "imdbUrl": "https://www.imdb.com/title/tt5875444/",
        "ratingsFrom": NSNull(), "source": "Cinemeta", "trailers": [],
    ]
    for (key, value) in overrides { json[key] = value }
    return try JSONDecoder().decode(TitleDetails.self, from: JSONSerialization.data(withJSONObject: json))
}

/// The Mac's action row at its widest: Resume, Trailer, a two-cell capsule, Versions (about 760).
private var fullRow: some View {
    FlowRow(spacing: 12) {
        ForEach([210.0, 120, 300, 130], id: \.self) { Color.clear.frame(width: $0, height: 40) }
    }
}

/// Lays the Mac hero out in a \`width\` window at the hero's \`height\` and returns its parts.
@MainActor private func layOut(width: CGFloat, height: CGFloat) throws -> [HeroPart: CGRect] {
    let parts = Parts()
    let hero = TitleHeroWide(facts: TitleHeroFacts(try details()), open: { _ in }, more: {}) { fullRow }
        .heroFrame(height: height, alignment: .bottomLeading) { EmptyView() }
        .heroProbe(.box)
        .environment(\.heroProbe, HeroProbeSink { part, frame in parts.frames[part] = frame })
        .environment(\.metrics, .regular)
    let host = NSHostingView(rootView: hero.frame(width: width).fixedSize(horizontal: false, vertical: true))
    host.frame = CGRect(x: 0, y: 0, width: width, height: 1200)
    host.layoutSubtreeIfNeeded()
    RunLoop.main.run(until: Date().addingTimeInterval(0.3))
    return parts.frames
}

/// A part's top above the hero's foot, and how far in its right edge reaches.
private func spot(_ part: CGRect, in box: CGRect) -> (top: CGFloat, end: CGFloat) {
    (box.maxY - part.minY, part.maxX - box.minX)
}

@MainActor @Test func theMacTitleHeroSitsWhereTheGlassCheckReadsIt() throws {
    for (width, floor) in [(CGFloat(BackdropStops.macMinWidth), CGFloat(400)), (1440, 810)] {
        let frames = try layOut(width: width, height: floor)
        let box = try #require(frames[.box])
        let meta = spot(try #require(frames[.meta]), in: box)
        let chips = spot(try #require(frames[.chips]), in: box)
        let synopsis = spot(try #require(frames[.synopsis]), in: box)
        let actions = spot(try #require(frames[.actions]), in: box)
        print(#"title-hero {"width":\#(width),"height":\#(box.height),"meta":[\#(meta.top),\#(meta.end)],"chips":[\#(chips.top),\#(chips.end)],"synopsis":[\#(synopsis.top),\#(synopsis.end)],"actions":[\#(actions.top),\#(actions.end)]}"#)
        // The column starts at the 40 pt margin and ends 75 above the foot.
        #expect(abs(try #require(frames[.actions]).minX - box.minX - 40) < 1)
        #expect(box.maxY - (try #require(frames[.actions])).maxY >= 75 - 1)
        // The meta row, the chips and the synopsis stay within the 480 cap.
        for (name, part) in [("meta", meta), ("chips", chips), ("synopsis", synopsis)] {
            #expect(part.end <= 40 + BackdropStops.heroSynopsisWidth + 1, "\(name) ends \(part.end) in")
        }
        // Each part sits no higher than the glass check's spot for it (TitleSpots).
        #expect(meta.top <= TitleSpots.meta + 1, "meta top \(meta.top) at \(width)")
        #expect(chips.top <= TitleSpots.chips + 1, "chips top \(chips.top) at \(width)")
        #expect(synopsis.top <= TitleSpots.synopsis + 1, "synopsis top \(synopsis.top) at \(width)")
        #expect(actions.top <= TitleSpots.upperRow + 40 + 1, "actions top \(actions.top) at \(width)")
        // The hero grows to its words at the floor.
        #expect(box.height >= floor)
    }
}
