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
        "ratings": ["imdb": "8.4"], "imdbUrl": "https://www.imdb.com/title/tt5875444/",
        "source": "Cinemeta", "trailers": [],
    ]
    for (key, value) in overrides { json[key] = value }
    return try JSONDecoder().decode(TitleDetails.self, from: JSONSerialization.data(withJSONObject: json))
}

/// A version list where nothing plays and the way on would hold only Back: Resume stays, inert.
private func noRoom() throws -> VersionsSnapshot {
    let empty = try JSONDecoder().decode(EmptyVersions.self, from: Data(#"{"kind": "noRoom", "count": 0, "rows": [], "key": null}"#.utf8))
    return VersionsSnapshot(
        recommended: nil, rows: [], pendingAddons: [], slowAddons: [], failedAddons: [], done: true, frozen: false,
        conditions: nil, note: nil, empty: empty)
}

/// The Mac's real action row (TitleScreen's `wideActions`): the primary, Trailer, the marks capsule, Versions.
private struct Row: View {
    let kind: String
    let target: PlayTarget?
    let versions: VersionsSnapshot?
    @FocusState private var focused: Bool

    var body: some View {
        WideActions(
            trailer: {},
            marks: titleMarks(kind: kind, inLibrary: false, watched: false), toggle: { _ in },
            versionsReady: true, showVersions: {}
        ) {
            PlayBlock(
                target: target, nextAirs: nil, hasEpisodes: kind == "series", runtime: "148 min", versions: versions,
                failure: nil, wide: true, play: { _ in }, focus: $focused, v3: true)
        }
    }
}

/// The rows at their widest: a movie whose Resume is inert, a movie and an episode to resume.
private func rows() throws -> [(String, Row)] {
    let resume = { (label: String) in PlayTarget(videoId: "v", label: label, resumeMs: 1_800_000, leftMs: 3_720_000) }
    return [
        ("movie, inert", Row(kind: "movie", target: resume("Resume"), versions: try noRoom())),
        ("movie, resume", Row(kind: "movie", target: resume("Resume"), versions: nil)),
        ("series, resume", Row(kind: "series", target: resume("Resume S6:E2"), versions: nil)),
    ]
}

/// Lays the Mac hero out in a `width` window at the hero's `height` and returns its parts, once every
/// part has reported and a layout pass has passed with no change.
@MainActor private func layOut<Actions: View>(
    width: CGFloat, height: CGFloat, details overrides: [String: Any] = [:], @ViewBuilder row: () -> Actions
) throws -> [HeroPart: CGRect] {
    let parts = Parts()
    let hero = TitleHeroWide(facts: TitleHeroFacts(try details(overrides)), open: { _ in }, more: {}, actions: row)
        .heroFrame(height: height, alignment: .bottomLeading) { EmptyView() }
        .heroProbe(.box)
        .environment(\.heroProbe, HeroProbeSink { part, frame in parts.frames[part] = frame })
        .environment(\.metrics, .regular)
    let host = NSHostingView(rootView: hero.frame(width: width).fixedSize(horizontal: false, vertical: true))
    host.frame = CGRect(x: 0, y: 0, width: width, height: 1200)
    let all: Set<HeroPart> = [.box, .meta, .chips, .synopsis, .actions]
    var last: [HeroPart: CGRect] = [:]
    for _ in 0..<200 {
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.01))
        if all.isSubset(of: parts.frames.keys), parts.frames == last { break }
        last = parts.frames
    }
    #expect(all.isSubset(of: parts.frames.keys), "parts reported: \(parts.frames.keys)")
    return parts.frames
}

/// A part's top above the hero's foot, and how far in its right edge reaches.
private func spot(_ part: CGRect, in box: CGRect) -> (top: CGFloat, end: CGFloat) {
    (box.maxY - part.minY, part.maxX - box.minX)
}

@MainActor @Test func theMacTitleHeroSitsWhereTheGlassCheckReadsIt() throws {
    let widest = try rows()[0].1
    for (width, floor) in [(CGFloat(BackdropStops.macMinWidth), CGFloat(400)), (1440, 810)] {
        let frames = try layOut(width: width, height: floor) { widest }
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

@MainActor @Test func starringShowsOnlyClearOfTheRealActionRow() throws {
    let widest = try rows()[0].1
    for (name, row) in try rows() {
        for width in [CGFloat(1240), 1280, 1355, 1440, 1600] {
            let frames = try layOut(width: width, height: WideLayout.titleHeroHeight(width: width, height: 900)) { row }
            let box = try #require(frames[.box])
            let actions = try #require(frames[.actions])
            print(#"title-row {"row":"\#(name)","window":\#(width),"width":\#(actions.width)}"#)
            guard WideLayout.starringFits(window: width, row: actions.width) else { continue }
            // Starring's 360 pt column, 40 in from the right, starts 40 or more past the row's end.
            #expect(box.maxX - 40 - 360 - actions.maxX >= 40 - 1, "\(name) at \(width)")
        }
    }
    // The review's case: a movie whose versions need an account, in a 1280 window.
    let frames = try layOut(width: 1280, height: 720) { widest }
    #expect(!WideLayout.starringFits(window: 1280, row: try #require(frames[.actions]).width))
}

@MainActor @Test func manyLongDirectorsKeepTheMetaRowWhereTheGlassCheckReadsIt() throws {
    let widest = try rows()[0].1
    let names = ["Florian Henckel von Donnersmarck", "Alejandro González Iñárritu", "Apichatpong Weerasethakul", "Jean-Pierre Jeunet", "Christopher McQuarrie"]
    #expect(TitleHeroFacts(try details(["directors": names])).directors.count == 3)
    for width in [CGFloat(BackdropStops.macMinWidth), 1440] {
        let frames = try layOut(width: width, height: 400, details: ["directors": names, "genres": ["Action & Adventure", "Sci-Fi & Fantasy", "Documentary"]]) { widest }
        let box = try #require(frames[.box])
        let meta = spot(try #require(frames[.meta]), in: box)
        let chips = spot(try #require(frames[.chips]), in: box)
        #expect(meta.top <= TitleSpots.meta + 1, "meta top \(meta.top) at \(width)")
        #expect(chips.top <= TitleSpots.chips + 1, "chips top \(chips.top) at \(width)")
        #expect(chips.end <= 40 + BackdropStops.heroSynopsisWidth + 1, "chips end \(chips.end) at \(width)")
    }
}

@Test func resumeCarriesStartOverAsANamedAction() throws {
    // VoiceOver's way to Start over beside Resume's context menu, only with a resume point (plans/
    // design-v3-phase3-apple.md 2.1). Read from the source: a hosting view out of a running app builds
    // no accessibility tree to ask.
    let file = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Title/TitleActions.swift")
    let text = try String(contentsOf: file, encoding: .utf8)
    let action = #"\.accessibilityActions \{\s+if \(target\?\.resumeMs \?\? 0\) > 0 \{ Button\("Start over"\) \{ play\(true\) \} \}\s+\}"#
    #expect(text.range(of: action, options: .regularExpression) != nil)
}

@MainActor @Test func aSeriesCapsuleDrawsNoWatchedCell() {
    // The core keeps no whole-series watched mark (C41): the series capsule is Library's cell alone.
    func width(_ marks: [TitleMark]) -> CGFloat {
        NSHostingView(rootView: MarkCapsule(marks: marks) { _ in }.fixedSize()).fittingSize.width
    }
    let series = width(titleMarks(kind: "series", inLibrary: false, watched: false))
    #expect(series == width([.library(on: false)]))
    #expect(width(titleMarks(kind: "movie", inLibrary: false, watched: false)) > series + 40)
}

@MainActor @Test func thePageIsInertOnlyUnderTheScrim() {
    // The rule: the panel covers the info below about 1020, and only then the page goes out of reach.
    #expect(WideLayout.pageInert(wide: true, panelOpen: true, window: 1000))
    #expect(!WideLayout.pageInert(wide: true, panelOpen: true, window: 1040))
    #expect(!WideLayout.pageInert(wide: true, panelOpen: false, window: 1000))
    #expect(!WideLayout.pageInert(wide: false, panelOpen: true, window: 390))
    // The modifier: an inert page's controls are off, so no key or click reaches them.
    final class Seen { var enabled: [Bool] = [] }
    struct Probe: View {
        let seen: Seen
        @Environment(\.isEnabled) private var enabled
        var body: some View { Color.clear.frame(width: 10, height: 10).onAppear { seen.enabled.append(enabled) } }
    }
    for on in [true, false] {
        let seen = Seen()
        let host = NSHostingView(rootView: Probe(seen: seen).inert(on))
        host.frame = CGRect(x: 0, y: 0, width: 10, height: 10)
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        #expect(seen.enabled == [!on], "inert(\(on)) left the page enabled: \(seen.enabled)")
    }
}

@MainActor @Test func aChipGroupDropsTheNamesThatDoNotFitAndCutsNone() throws {
    // Drop, never truncate: every drawn chip is as wide as its whole name (review r2).
    let names = ["Florian Henckel von Donnersmarck", "Alejandro González Iñárritu", "Apichatpong Weerasethakul"]
    let parts = Parts()
    let host = NSHostingView(rootView: ChipGroups(genres: [], directors: names, open: { _ in })
        .frame(width: BackdropStops.heroSynopsisWidth, alignment: .leading)
        .environment(\.heroProbe, HeroProbeSink { part, frame in parts.frames[part] = frame }))
    host.frame = CGRect(x: 0, y: 0, width: BackdropStops.heroSynopsisWidth, height: 200)
    for _ in 0..<50 {
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.01))
    }
    let drawn = names.filter { (parts.frames[.chip($0)]?.width ?? 0) > 0 }
    #expect((1..<names.count).contains(drawn.count), "drawn: \(drawn)")
    for name in drawn {
        let whole = NSHostingView(rootView: Text(name).textFont(13, weight: .medium, relativeTo: .footnote).lineLimit(1).padding(.horizontal, 12).fixedSize()).fittingSize.width
        #expect(abs((parts.frames[.chip(name)]?.width ?? 0) - whole) < 1, "\(name) drawn \(parts.frames[.chip(name)]?.width ?? 0), whole \(whole)")
    }
}

@MainActor @Test func theIPhoneKindLineStaysOneLine() throws {
    // At the narrowest phone width the long genres drop before the line wraps (review r2 P3-5).
    let facts = TitleHeroFacts(try details(["genres": ["Action & Adventure", "Sci-Fi & Fantasy", "Drama"]]))
    #expect(facts.kindLines.first == "Series \u{B7} Action & Adventure \u{B7} Sci-Fi & Fantasy")
    let parts = Parts()
    let host = NSHostingView(rootView: TitleHeroPhone(facts: facts) { EmptyView() } secondaries: { EmptyView() }
        .frame(width: 320)
        .environment(\.heroProbe, HeroProbeSink { part, frame in parts.frames[part] = frame }))
    host.frame = CGRect(x: 0, y: 0, width: 320, height: 400)
    for _ in 0..<50 {
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.01))
    }
    let line = try #require(parts.frames[.kindLine])
    // 12 above it, then one 15 pt line.
    #expect(line.height < 12 + 15 * 1.6, "kind line \(line)")
    #expect(line.width <= 320 - 40 + 1)
}

// The iPhone's title art (the owner, 2026-10-26: covers are vertical): the cover shows whole, at its own 2:3,
// held to 75 percent of the window; the core's large cover decodes, and an older core's page has none.
@Test func theIPhoneShowsTheCoverWholeAtTwoByThree() throws {
    #expect(coverHeroHeight(width: 390, height: 844) == 585)
    // A short window holds it to 75 percent of its height.
    #expect(coverHeroHeight(width: 375, height: 667) == 500.25)
    let large = "https://images.metahub.space/poster/large/tt22084616/img"
    #expect(try details(["cover": large]).cover == large)
    #expect(try details().cover == nil)
}

// The cover shows only where its 2:3 fits (a portrait iPhone, held to 75 percent of the window, losing at most an
// eighth at its foot); a landscape iPhone or a window over 600 pt keeps the backdrop band.
@Test func theCoverShowsOnlyWhereItFits() {
    for (w, h) in [(CGFloat(390), CGFloat(844)), (375, 667), (320, 568), (430, 932)] {
        #expect(coverFits(width: w, height: h), "\(w)x\(h)")
    }
    for (w, h) in [(CGFloat(844), CGFloat(390)), (667, 375), (700, 1100), (390, 500)] {
        #expect(!coverFits(width: w, height: h), "\(w)x\(h)")
    }
}

// The page feeds the rule the whole window (the cover runs from the screen's top): its own size plus the safe
// area it does not ignore. An iPhone SE (a 647 pt page under a 20 pt status bar) and a 320 pt phone show the cover.
@Test func theFitRuleMeasuresTheWholeWindow() {
    #expect(fullWindowHeight(viewHeight: 647, safeTop: 20, safeBottom: 0) == 667)
    #expect(coverFits(width: 375, height: fullWindowHeight(viewHeight: 647, safeTop: 20, safeBottom: 0)))
    #expect(coverFits(width: 320, height: fullWindowHeight(viewHeight: 548, safeTop: 20, safeBottom: 0)))
    // The page's own height alone would have sent the SE to the band.
    #expect(!coverFits(width: 375, height: 647))
}

// The words under a cover show once the inset is known, 0 included (a window with no status bar).
@Test func theWordsUnderACoverShowOnceTheInsetIsKnown() {
    #expect(!coverWordsShown(insetTop: nil))
    #expect(coverWordsShown(insetTop: 0))
    #expect(coverWordsShown(insetTop: 59))
}

// A cover that does not load gives way to the addon's poster (the same 2:3), then to the band with the logo: the
// sources in order, and the loader takes the first that loads.
@Test func theCoverFallsBackToThePosterThenTheBand() async throws {
    let large = "https://images.metahub.space/poster/large/tt1/img"
    let small = "https://images.metahub.space/poster/small/tt1/img"
    #expect(coverArtSources(try details(["cover": large, "poster": small])) == [large, small])
    #expect(coverArtSources(try details(["cover": small, "poster": small])) == [small])
    #expect(coverArtSources(try details(["cover": large])) == [large])
    #expect(coverArtSources(try details(["poster": small])) == [small])
    let fromPoster = await ArtLoader.firstLoaded([large, small]) { url in url == small ? "poster" : nil }
    #expect(fromPoster == "poster")
    let first = await ArtLoader.firstLoaded([large, small]) { url in "art of \(url)" }
    #expect(first == "art of \(large)")
    let none: String? = await ArtLoader.firstLoaded([large, small]) { _ in nil }
    #expect(none == nil)
}

// The logo goes with a cover the core does not know to print its title (an anime's Kitsu poster, an addon's art),
// and with an older core's, which says nothing; only a metahub cover goes without.
@Test func onlyACoverKnownToPrintItsTitleGoesWithoutTheLogo() throws {
    #expect(!coverWantsLogo(try details(["cover": "https://images.metahub.space/poster/large/tt1/img", "coverTitled": true])))
    #expect(coverWantsLogo(try details(["cover": "https://media.kitsu.app/anime/poster_images/1/large.jpg", "coverTitled": false])))
    #expect(coverWantsLogo(try details(["cover": "https://images.metahub.space/poster/large/tt1/img"])))
}

// A cover prints its own title: the iPhone draws no logo over or under it (the kind line leads the words), and the
// cover's last row of pixels is the page color, where the words start.
@MainActor @Test func underACoverTheIPhoneDrawsNoSecondTitle() throws {
    func kindTop(logo: Bool) throws -> CGFloat {
        let parts = Parts()
        let hero = TitleHeroPhone(facts: TitleHeroFacts(try details(["logo": "https://a.example/logo.png"])), logo: logo) {
            EmptyView()
        } secondaries: {
            EmptyView()
        }
        .heroProbe(.box)
        .environment(\.heroProbe, HeroProbeSink { part, frame in parts.frames[part] = frame })
        let host = NSHostingView(rootView: hero.frame(width: 390))
        host.frame = CGRect(x: 0, y: 0, width: 390, height: 600)
        for _ in 0..<100 where parts.frames[.kindLine] == nil || parts.frames[.box] == nil {
            host.layoutSubtreeIfNeeded()
            RunLoop.main.run(until: Date().addingTimeInterval(0.01))
        }
        return try #require(parts.frames[.kindLine]).minY - (try #require(parts.frames[.box])).minY
    }
    // Only the 12 pt above the kind line, against the 86 pt logo over it.
    #expect(try kindTop(logo: false) <= 13)
    #expect(try kindTop(logo: true) >= 86)
    // The cover's foot, with no picture under it yet: its bottom row is the page's color.
    let host = NSHostingView(rootView: CoverArt(sources: [], name: "Big Buck Bunny", height: 300).frame(width: 200, height: 300))
    host.frame = CGRect(x: 0, y: 0, width: 200, height: 300)
    host.layoutSubtreeIfNeeded()
    let rep = try #require(host.bitmapImageRepForCachingDisplay(in: host.bounds))
    host.cacheDisplay(in: host.bounds, to: rep)
    let page = try #require(NSColor(Theme.Palette.bg).usingColorSpace(.sRGB))
    for x in stride(from: 0, to: rep.pixelsWide, by: 20) {
        let edge = try #require(rep.colorAt(x: x, y: rep.pixelsHigh - 1)?.usingColorSpace(.sRGB))
        for (a, b) in [(edge.redComponent, page.redComponent), (edge.greenComponent, page.greenComponent), (edge.blueComponent, page.blueComponent)] {
            #expect(abs(a - b) <= 3.0 / 255, "x \(x): \(edge) against the page's \(page)")
        }
    }
}

// The owner's standing rule: Watched is a small control, never a big button. On the iPhone it is a 48 pt
// round toggle beside Library, and no pill on the page carries it.
@MainActor @Test func theIPhoneWatchedIsASmallRoundToggle() throws {
    for on in [false, true] {
        let host = NSHostingView(rootView: WatchedControl(on: on) {})
        #expect(host.fittingSize == CGSize(width: 48, height: 48), "\(host.fittingSize)")
    }
    let screen = try String(contentsOf: URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .appending(path: "../Marquee/Title/TitleScreen.swift"), encoding: .utf8)
    #expect(screen.contains("WatchedControl(on: watched) { toggle(.watched(on: watched), details) }"))
    #expect(!screen.contains("glyph: watched ? \"eye-off\" : \"eye\""))
}
