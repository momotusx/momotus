import Foundation
import Testing

// Discover on the Mac, iPad and iPhone (plans/design-v3-phase7-apple.md 4): the layout from the width,
// select then open, the words, and the wiring the views take from them.

private func source(_ path: String) throws -> String {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    return try String(contentsOf: app.appending(path: path), encoding: .utf8)
}

private func snapshot(
    kinds: [(String, String)] = [("movie", "Movies"), ("series", "Series")], genres: [String] = ["Drama"],
    genre: String? = nil, genreRequired: Bool = false, state: [String: String] = ["state": "ready"], items: Int = 0
) throws -> DiscoverSnapshot {
    let tile: (Int) -> [String: Any] = { n in
        ["id": "tt\(n)", "kind": "movie", "name": "Title \(n)", "poster": NSNull(), "posterFallback": NSNull(), "background": NSNull(),
         "logo": NSNull(), "posterShape": "poster", "releaseInfo": "2020", "progress": NSNull(), "videoId": NSNull(),
         "rating": NSNull(), "ratings": NSNull()]
    }
    let json: [String: Any] = [
        "kinds": kinds.map { [$0.0, $0.1] }, "kind": kinds.first?.0 ?? NSNull(),
        // An addon's name where it may sit: the catalog's source, never shown.
        "catalogs": [["id": "cinemeta|movie|top", "name": "Popular", "source": "Cinemeta"]], "catalog": "cinemeta|movie|top",
        "genres": genres, "genre": genre ?? NSNull(), "genreRequired": genreRequired,
        "items": (0..<items).map(tile), "state": state, "missingGenre": NSNull(),
    ]
    return try JSONDecoder().decode(DiscoverSnapshot.self, from: JSONSerialization.data(withJSONObject: json))
}

@Test func thePreviewShowsBesideFourColumns() {
    #expect(!DiscoverLayout.previewFits(width: 1119))
    #expect(DiscoverLayout.previewFits(width: 1120))
    #expect(DiscoverLayout.columns(width: 1120) == 4)
    #expect(DiscoverLayout.columns(width: 1440) == 6)
    // Without the preview the grid takes the width.
    #expect(DiscoverLayout.columns(width: 1119) == 6)
    #expect(DiscoverLayout.columns(width: 1024) == 6)
    #expect(DiscoverLayout.columns(width: 720) == 4)
    #expect(DiscoverLayout.columns(width: 200) == 1)
}

@Test func theIPhonesTwoColumnsShareTheWidth() {
    // The frame's 169 by 254 at 390 with 20 margins, then the other phones.
    #expect(DiscoverLayout.phonePoster(width: 350) == CGSize(width: 169, height: 254))
    #expect(DiscoverLayout.phonePoster(width: 343) == CGSize(width: 165, height: 248))
    #expect(DiscoverLayout.phonePoster(width: 398) == CGSize(width: 193, height: 290))
}

@Test func kindsAreSegmentedUpToFour() {
    #expect(KindControl.for(count: 1) == .none)
    #expect(KindControl.for(count: 2) == .segmented)
    #expect(KindControl.for(count: 4) == .segmented)
    #expect(KindControl.for(count: 5) == .pullDown)
}

@Test func aFirstActivationSelectsAndTheSelectedOneOpens() {
    var selection = DiscoverSelection()
    selection.update(scope: "movie|top|", keys: ["a", "b", "c"])
    #expect(selection.key == "a")
    let first = selection.activate("b")
    #expect(!first)
    #expect(selection.key == "b")
    let second = selection.activate("b")
    #expect(second)
    // More of the same catalog keeps the selection; a new choice starts at its first poster.
    selection.update(scope: "movie|top|", keys: ["a", "b", "c", "d"])
    #expect(selection.key == "b")
    // A new list starts at its first poster even when it also lists the selected one.
    selection.update(scope: "movie|top|Drama", keys: ["x", "b"])
    #expect(selection.key == "x")
    // A page that loses the selected poster selects its first.
    selection.update(scope: "movie|top|Drama", keys: ["y"])
    #expect(selection.key == "y")
}

@Test func arrowsMoveTheSelectionAndStopAtTheEdges() {
    // Seven posters, three a row.
    #expect(DiscoverSelection.moved(from: 0, .right, columns: 3, count: 7) == 1)
    #expect(DiscoverSelection.moved(from: 2, .right, columns: 3, count: 7) == 2)
    #expect(DiscoverSelection.moved(from: 3, .left, columns: 3, count: 7) == 3)
    #expect(DiscoverSelection.moved(from: 1, .down, columns: 3, count: 7) == 4)
    #expect(DiscoverSelection.moved(from: 5, .down, columns: 3, count: 7) == 5)
    #expect(DiscoverSelection.moved(from: 1, .up, columns: 3, count: 7) == 1)
    #expect(DiscoverSelection.moved(from: 6, .right, columns: 3, count: 7) == 6)
}

@Test func discoversWordsNeverNameAnAddon() throws {
    let s = try snapshot(items: 3)
    let words = DiscoverWords.kinds(s).map(\.title) + DiscoverWords.catalogs(s).map(\.title) + DiscoverWords.genres(s).map(\.title)
        + [DiscoverWords.kind(s), DiscoverWords.catalog(s), DiscoverWords.genre(s)]
    #expect(words == ["Movies", "Series", "Popular", "All genres", "Drama", "Movies", "Popular", "All genres"])
    #expect(!words.joined().contains("Cinemeta"))
    // A failure's reason can carry a host or an addon: the shared line instead.
    let failed = try snapshot(state: ["state": "failed", "reason": "https://v3-cinemeta.strem.io answered 500 (Cinemeta)"])
    #expect(DiscoverWords.state(failed) == "This catalog did not load.")
    #expect(DiscoverWords.state(try snapshot(state: ["state": "end"])) == "Nothing here for this choice.")
    #expect(DiscoverWords.state(try snapshot(genreRequired: true)) == "This catalog lists titles by genre. Pick one above.")
    #expect(DiscoverWords.genres(try snapshot(genreRequired: true)).map(\.title) == ["Drama"])
    #expect(DiscoverWords.state(try snapshot(items: 3)) == nil)
}

@Test func aKindIsWordedOnlyWhenMarqueeKnowsIt() {
    #expect(["movie", "series", "channel", "tv", "anime"].compactMap(kindWord) == ["Movie", "Series", "Channel", "Live TV", "Anime"])
    // An addon's own type is often its name ("trakt"): never shown as is.
    #expect(kindWord("trakt") == nil && kindWord("other") == nil)
}

@Test func onlyDiscoverAndSearchTakeTheDesktopTitle() {
    #expect(Metrics.regular.screenTitle() == 40)
    #expect(Metrics.regular.screenTitle(desktop: true) == 28)
    #expect(Metrics.compact.screenTitle() == Theme.type.title1 + 6)
    #expect(Metrics.compact.screenTitle(desktop: true) == Theme.type.title1 + 6)
}

@Test func discoversViewsTakeTheRulesAndTheWords() throws {
    // Read from the sources: the grid and the preview are SwiftUI views over a live core.
    let parts = try source("Discover/DiscoverParts.swift")
    let screen = try source("Discover/DiscoverScreen.swift")
    // Every activation goes through the selection; the grid pushes no link of its own.
    let grid = try body(of: "struct DiscoverGrid: View", in: parts)
    #expect(grid.contains("Button { activate(key, tile) }"))
    #expect(grid.contains("if !selects || selection.activate(key) { openTitle(TitleRoute(tile)) }"))
    #expect(!grid.contains("NavigationLink"))
    // The selection follows the keyboard's focus and VoiceOver's cursor; "Select" is the custom action.
    #expect(grid.contains("@AccessibilityFocusState private var voiceOver: String?"))
    #expect(grid.contains(".onChange(of: voiceOver)") && grid.contains(".onChange(of: focused)"))
    #expect(grid.contains(".accessibilityAction(named: \"Select\") { selection.select(key) }"))
    #expect(grid.contains(".buttonStyle(CardButtonStyle(lifted: on))"))
    // The preview reads the catalog item alone: no title model, no title view.
    let preview = try body(of: "struct DiscoverPreview: View", in: parts)
    #expect(!preview.contains("TitleModel") && !preview.contains("LiveRecord") && !preview.contains("core"))
    // The words come from DiscoverWords; no catalog's source and no core reason reach the page.
    for text in [parts, screen] {
        #expect(!text.contains(".source") && !text.contains("reason"))
    }
    #expect(screen.contains("StateLine(symbol: \"triangle-alert\", text: DiscoverWords.failed"))
    #expect(screen.contains("ScreenTitle(text: \"Discover\", desktop: true)"))
    #expect(screen.contains("DiscoverLayout.previewFits(width: width)") && screen.contains("DiscoverLayout.columns(width: width)"))
    // Dynamic Type grows the phone's parts (the Mac has none, so the iPhone walk shows it): heights are
    // floors and names wrap to two lines.
    #expect(parts.contains(".frame(minHeight: Theme.Pill.phone)"))
    #expect(parts.contains(".frame(maxWidth: wide ? nil : .infinity, minHeight: (wide ? Theme.Pill.desktop : Theme.Pill.phone) - 6)"))
    let phone = try body(of: "struct PhoneDiscoverGrid: View", in: parts)
    #expect(phone.contains(".lineLimit(2)"))
}

@Test func thePreviewReadsTheCoresDetailsForItsOwnItem() throws {
    let s = try snapshot(items: 2)
    let tile = try #require(s.items.first)
    let details = ItemDetails(id: tile.id, description: "A thief who steals secrets.", genres: ["Action", "Sci-Fi"], runtime: "148 min", inLibrary: false, librarySettable: true)
    let words = DiscoverWords.preview(tile, details)
    #expect(words.parts == ["Movie", "2020", "Action, Sci-Fi", "148 min"])
    #expect(words.synopsis == "A thief who steals secrets.")
    // Details for another item (the last selection's, still in the snapshot) are not this one's.
    let other = ItemDetails(id: "tt999", description: "Another.", genres: ["Drama"], runtime: "90 min", inLibrary: true, librarySettable: true)
    #expect(DiscoverWords.preview(tile, other).parts == ["Movie", "2020"])
    #expect(DiscoverWords.preview(tile, other).synopsis == nil)
    #expect(DiscoverWords.preview(tile, nil).parts == ["Movie", "2020"])
    // The screen asks the core for the previewed item, again for a new list, and draws its answer.
    let screen = try source("Discover/DiscoverScreen.swift")
    #expect(screen.contains("discover.command([\"type\": \"preview\", \"id\": ask.id])"))
    #expect(screen.contains("DiscoverPreviewAsk(id: tile.id, list: s.list)"))
    #expect(screen.contains("DiscoverPreview(tile: preview, details: discover.snapshot?.preview,"))
}

@Test func addToLibraryShowsOnlyWhereTheLibraryCanBeSet() throws {
    let s = try snapshot(items: 1)
    let tile = try #require(s.items.first)
    func details(id: String? = nil, in inLibrary: Bool?, settable: Bool?) -> ItemDetails {
        ItemDetails(id: id ?? tile.id, description: nil, genres: [], runtime: nil, inLibrary: inLibrary, librarySettable: settable)
    }
    #expect(DiscoverWords.library(tile, details(in: false, settable: true)) == false)
    #expect(DiscoverWords.library(tile, details(in: true, settable: true)) == true)
    // A kids profile, a core from before C44b, another item's details, or none yet: no button.
    #expect(DiscoverWords.library(tile, details(in: false, settable: false)) == nil)
    #expect(DiscoverWords.library(tile, details(in: nil, settable: nil)) == nil)
    #expect(DiscoverWords.library(tile, details(id: "tt999", in: true, settable: true)) == nil)
    #expect(DiscoverWords.library(tile, nil) == nil)
    // The button sends setInLibrary for the previewed item, then the change syncs, as on the title page.
    let screen = try source("Discover/DiscoverScreen.swift")
    #expect(screen.contains("discover.command([\"type\": \"setInLibrary\", \"id\": preview.id, \"value\": value])\n                        sync.schedule()"))
    let preview = try body(of: "struct DiscoverPreview: View", in: source("Discover/DiscoverParts.swift"))
    #expect(preview.contains("if let inLibrary = DiscoverWords.library(tile, details) {"))
    #expect(preview.contains("Button { setLibrary(!inLibrary) }"))
}

@Test func discoversGridNeverHoldsTheWindowOpen() throws {
    // The fixed columns and the skeleton follow the width; with a minimum of their own they kept the last,
    // wider layout when the Mac's window narrowed (the walk at 720 showed six columns cut) and held the
    // window's content min size at that width.
    let parts = try source("Discover/DiscoverParts.swift")
    for name in ["struct DiscoverGrid: View", "struct DiscoverSkeleton: View"] {
        #expect(try body(of: name, in: parts).contains(".frame(minWidth: 0, idealWidth: 0, maxWidth: .infinity, alignment: .topLeading)"), "\(name)")
    }
}

@Test func theGridTakesItsKeysWithoutFullKeyboardAccess() throws {
    // A Button takes no keyboard focus with Full Keyboard Access off, so keys on the buttons alone were
    // dead (review-apple-player P2): the grid is focusable itself, a click gives it the keys, and the
    // arrows move the selection.
    let grid = try body(of: "struct DiscoverGrid: View", in: source("Discover/DiscoverParts.swift"))
    for part in [".focusable(interactions: .edit)", ".focusEffectDisabled()", ".focused($gridFocused)",
                 ".onKeyPress(.leftArrow) { move(.left", ".onKeyPress(.downArrow) { move(.down", ".onKeyPress(.return)"] {
        #expect(grid.contains(part), "the grid lost \(part)")
    }
    #expect(try body(of: "private func activate(_ key: String, _ tile: Tile)", in: grid).contains("gridFocused = true"))
    #expect(!grid.contains("onMoveCommand"))
}

@Test func theGridWaitsForItsFirstWidth() throws {
    // Before the page has measured, fixed columns would draw at a guess and overflow a narrow iPad window
    // (Slide Over at 320; review-apple-player P3 5).
    let screen = try source("Discover/DiscoverScreen.swift")
    #expect(screen.contains("if width > 0 {\n                        DiscoverGrid("))
}
