import Foundation
import Testing

// Search on the Mac, iPad and iPhone (plans/design-v3-phase7-apple.md 3): the words, Top results, the
// columns, and the wiring the views take from them.

private func source(_ path: String) throws -> String {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    return try String(contentsOf: app.appending(path: path), encoding: .utf8)
}

private func tile(_ n: Int, kind: String = "movie") -> [String: Any] {
    ["id": "tt\(n)", "kind": kind, "name": "Title \(n)", "poster": "https://api.ratingposterdb.com/tt\(n).jpg",
     "posterFallback": "https://images.metahub.space/poster/tt\(n)", "background": NSNull(), "logo": NSNull(),
     "posterShape": "poster", "releaseInfo": "2010", "progress": NSNull(), "videoId": NSNull(), "rating": NSNull(),
     "ratings": ["imdb": "8.8"]]
}

private func row(_ id: String, title: String, source: String, state: String, tiles: Int) -> [String: Any] {
    ["rowId": id, "title": title, "source": source, "state": state == "failed" ? ["state": state, "reason": "https://v3-cinemeta.strem.io 500"] : ["state": state],
     "tiles": (0..<tiles).map { tile($0) }]
}

private func snapshot(query: String = "dune", rows: [[String: Any]] = [], searchable: Int = 2, recent: [String] = ["heat"], start: [String: Any]? = nil) throws -> SearchSnapshot {
    var json: [String: Any] = ["query": query, "rows": rows, "recent": recent, "searchable": searchable]
    if let start { json["start"] = start } else { json["start"] = NSNull() }
    return try JSONDecoder().decode(SearchSnapshot.self, from: JSONSerialization.data(withJSONObject: json))
}

/// A kids profile's Search looks over its own list (plans/m11-kids.md 6.4, QA round 1 Q7 row 31): its no-match line asks a
/// grown-up, never that no addon can search, and it offers no addons.
@Test func aKidsSearchSaysItsOwnListAndOffersNoAddons() throws {
    let none = try snapshot(rows: [], searchable: 0)
    #expect(SearchWords.answered(none, query: "dune", kids: true) == "That isn't on your list. Ask a grown-up.")
    let found = try snapshot(rows: [row("kids-titles", title: "Your shows and movies", source: "Your list", state: "ready", tiles: 1)], searchable: 0)
    #expect(SearchWords.answered(found, query: "dune", kids: true) == nil)
    #expect(SearchWords.idleLine(try snapshot(searchable: 0, recent: []), kids: true) == SearchWords.hint)
    #expect(!SearchWords.offersAddons(none, kids: true))
    #expect(SearchWords.kidsPrompt == "Search your shows")
}

@Test func theFieldSaysWhatSearchFinds() throws {
    // The hint shows only where nothing else does: no Recent and no shelf of titles (owner, 2026-11-02).
    #expect(SearchWords.idleLine(nil) == nil, "nothing before the core's first snapshot")
    #expect(SearchWords.idleLine(try snapshot(recent: [])) == "Search movies and series by title.")
    #expect(SearchWords.idleLine(try snapshot(recent: ["heat"])) == nil)
    let shelf = row("cinemeta:movie:top", title: "Popular Movies", source: "Cinemeta", state: "ready", tiles: 3)
    #expect(SearchWords.idleLine(try snapshot(recent: [], start: shelf)) == nil)
    // No addon can search: said before any query, and after one.
    #expect(SearchWords.idleLine(try snapshot(searchable: 0)) == "None of your addons can search")
    #expect(SearchWords.answered(try snapshot(searchable: 0), query: "dune") == "None of your addons can search")
    // Nothing matched: the line says the query, as the web and Android do (trimmed, as the core keeps it).
    #expect(SearchWords.answered(try snapshot(rows: []), query: "dune ") == "No matches for \"dune\" in your catalogs.")
    #expect(SearchWords.answered(try snapshot(rows: [row("a", title: "Movies", source: "Cinemeta", state: "ready", tiles: 2)]), query: "dune") == nil)
    // An answer to an older query says nothing yet.
    #expect(SearchWords.answered(try snapshot(query: "du"), query: "dune") == nil)
}

@Test func noAddonThatSearchesOffersTheRecommendedAddons() throws {
    // The line that says no addon can search has a way on; one that says nothing matched does not.
    #expect(SearchWords.offersAddons(try snapshot(searchable: 0)))
    #expect(!SearchWords.offersAddons(try snapshot(rows: [])))
    #expect(!SearchWords.offersAddons(nil))
    #expect(SearchWords.addons == "Recommended addons")
    // Each line that can say it carries the action, and the action pushes the page Settings opens.
    let screen = try source("Search/SearchScreen.swift")
    // The answered line on every form, and TV's idle line.
    #expect(screen.components(separatedBy: "StateLine(symbol: \"search\", text: line, action: addonsAction, spoken: true)").count - 1 == 2, "an answered line lost its action")
    #expect(screen.contains("StateLine(symbol: \"search\", text: line, action: addonsAction).padding(.horizontal, metrics.gutter)"), "TV's idle line lost its action")
    #expect(screen.contains("if let addonsAction {\n                    Button(addonsAction.title, action: addonsAction.run)"), "the idle line lost its action")
    #expect(screen.contains(".navigationDestination(isPresented: $addons) { RecommendedAddonsPage(core: core, scope: .all) }"))
}

@Test func topResultsLeadWithSixAndGoWhenTheyHaveNone() throws {
    let movies = row("cinemeta:movie:top", title: "Movies", source: "Cinemeta", state: "ready", tiles: 3)
    func top(_ state: String, _ tiles: Int) throws -> SearchWords.Top? {
        SearchWords.top(try snapshot(rows: [row("top", title: "Top results", source: "Trakt", state: state, tiles: tiles), movies]).rows)
    }
    guard case .tiles(let six) = try top("ready", 9) else { Issue.record("no tiles"); return }
    #expect(six.map(\.id) == (0..<6).map { "tt\($0)" })
    #expect(try top("loading", 0) == .loading)
    #expect(try top("ready", 0) == nil)
    #expect(try top("empty", 0) == nil)
    #expect(try top("failed", 0) == nil)
    // Absent (a kids profile, a short query, no Trakt app): the groups start at the top.
    let groupsOnly = try snapshot(rows: [movies]).rows
    #expect(SearchWords.top(groupsOnly) == nil)
    #expect(SearchWords.groups(try snapshot(rows: [row("top", title: "Top results", source: "Trakt", state: "ready", tiles: 2), movies]).rows).map(\.rowId) == ["cinemeta:movie:top"])
}

@Test func topResultsTakeUpToThreeColumns() {
    #expect(SearchRules.topResultColumns(width: 720) == 1)
    #expect(SearchRules.topResultColumns(width: 1024) == 2)
    #expect(SearchRules.topResultColumns(width: 1440) == 3)
    #expect(SearchRules.topResultColumns(width: 2560) == 3)
    #expect(SearchRules.topResultColumns(width: 300) == 1)
}

@Test func aResultSaysItsKindOnlyWhenMarqueeKnowsIt() throws {
    let movie = try JSONDecoder().decode(Tile.self, from: JSONSerialization.data(withJSONObject: tile(1)))
    let trakt = try JSONDecoder().decode(Tile.self, from: JSONSerialization.data(withJSONObject: tile(2, kind: "trakt")))
    #expect(SearchWords.meta(movie) == "Movie \u{B7} 2010")
    #expect(SearchWords.meta(trakt) == "2010")
}

@Test func searchNamesNoAddon() throws {
    // The rows' sources and the Top row's ("Trakt") never reach a word the page shows.
    let s = try snapshot(rows: [row("top", title: "Top results", source: "Trakt", state: "ready", tiles: 2),
                                row("cinemeta:movie:top", title: "Movies", source: "Cinemeta", state: "failed", tiles: 0)])
    let groups: [String?] = SearchWords.groups(s.rows).flatMap { [$0.title, $0.stateWords] }
    let metas: [String?] = s.rows.flatMap(\.tiles).map(SearchWords.meta)
    let words: [String?] = [SearchWords.idleLine(s), SearchWords.answered(s, query: "dune"), SearchWords.topResults] + groups + metas
    let shown = words.compactMap { $0 }.joined(separator: " ")
    #expect(!shown.contains("Cinemeta") && !shown.contains("Trakt") && !shown.contains("strem.io"))
    for path in ["Search/SearchScreen.swift", "Search/SearchParts.swift"] {
        let text = try source(path)
        #expect(!text.contains(".source") && !text.contains("reason") && !text.contains("failure ??"), "\(path) shows a source or a core reason")
    }
}

@Test func searchsViewsTakeTheRulesAndTheWords() throws {
    let screen = try source("Search/SearchScreen.swift")
    let parts = try source("Search/SearchParts.swift")
    // A result's art is the plain poster, never the cover with its band.
    let thumb = try body(of: "private struct ResultThumb: View", in: parts)
    #expect(thumb.contains("ArtImage(url: tile.plainPoster"))
    #expect(!parts.contains("url: tile.poster"))
    // The Mac's and iPad's Top results; the groups as shelves with no addon name.
    #expect(screen.contains("SearchWords.top(rows)") && screen.contains("SearchRules.topResultColumns(width: width)"))
    #expect(screen.contains("ResultSkeleton(count: 6, wide: true, columns: columns)"))
    #expect(screen.contains("ForEach(SearchWords.groups(rows), id: \\.rowId)"))
    // The page's field: a fill and no edge (the gate's old stroke goes).
    let field = try body(of: "struct SearchField: View", in: parts)
    #expect(!field.contains("strokeBorder") && field.contains(".frame(minHeight: 42)"))
    // Dynamic Type grows the phone's parts (the iPhone walk shows it): floors, and names on two lines.
    let row = try body(of: "struct SearchResultRow: View", in: parts)
    #expect(row.contains(".frame(minHeight: 92)") && row.contains(".lineLimit(2)"))
    // Return in a field moves to the first result.
    #expect(screen.contains(".onChange(of: input.submit) { firstResult = true }"))
    #expect(field.contains(".onSubmit { input.submit += 1 }"))
}

/// An empty Search gives the user something to do (owner, 2026-11-02): Recent with a way to clear it, then the core's
/// shelf of titles with its Home title, on the Mac, iPad, iPhone and TV; the field clears to that page and keeps the cursor.
@Test func anEmptySearchShowsRecentAndAShelfOfTitles() throws {
    let shelf = row("cinemeta:movie:top", title: "Popular Movies", source: "Cinemeta", state: "ready", tiles: 4)
    let s = try snapshot(recent: ["heat"], start: shelf)
    #expect(s.start?.title == "Popular Movies" && s.start?.tiles.count == 4)
    #expect(try snapshot().start == nil, "offline or none: no shelf, no error block")
    #expect(SearchRules.clearRecentCommand["type"] as? String == "clearRecent")
    let screen = try source("Search/SearchScreen.swift")
    let parts = try source("Search/SearchParts.swift")
    // The wide and phone pages draw the shelf as a Home row; the TV page too.
    #expect(screen.components(separatedBy: "RowView(row: start, retry: {})").count - 1 == 2, "a form lost the shelf")
    #expect(screen.components(separatedBy: "SearchRules.clearRecentCommand").count - 1 == 2, "a form lost Clear")
    // The clear button keeps the cursor in the field.
    let field = try body(of: "struct SearchField: View", in: parts)
    #expect(field.contains("input.text = \"\"; focused.wrappedValue = true"))
    // Clear is a 44 pt target with its own words.
    let recent = try body(of: "struct RecentList: View", in: parts)
    #expect(recent.contains(".frame(minWidth: 44, minHeight: 44)") && recent.contains("SearchWords.clearRecentLabel"))
    // The shelf shows no addon's name.
    #expect(!screen.contains("start.source") && !screen.contains("start?.source"))
}
