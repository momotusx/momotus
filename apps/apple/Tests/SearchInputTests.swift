import Foundation
import Testing

// One query for Search's page, opened from the Mac's Search button or Cmd-F (plans/design-v3-phase7-apple.md 2).

private func source(_ path: String) throws -> String {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    return try String(contentsOf: app.appending(path: path), encoding: .utf8)
}

@Test func typingShowsSearchsFirstPage() {
    // From another place.
    #expect(SearchRules.bringsSearch(text: "d", onSearch: false, titleOver: false))
    // Over a title opened from the results: back to the results, which the query changes (A r2 N2-3).
    #expect(SearchRules.bringsSearch(text: "du", onSearch: true, titleOver: true))
    // Already on the results: nothing moves.
    #expect(!SearchRules.bringsSearch(text: "dune", onSearch: true, titleOver: false))
    // A cleared field (Escape, leaving Search) shows nothing.
    #expect(!SearchRules.bringsSearch(text: "", onSearch: false, titleOver: false))
}

@Test func escapeClearsThenLetsGo() {
    #expect(SearchRules.escape("dune") == .clear)
    #expect(SearchRules.escape("") == .leave)
}

@Test func leavingSearchOnTheMacClearsTheQuery() {
    #expect(SearchRules.clearsOnLeave(text: "dune", shown: false))
    // Still shown (a title pushed from the results keeps the place on screen), or nothing typed.
    #expect(!SearchRules.clearsOnLeave(text: "dune", shown: true))
    #expect(!SearchRules.clearsOnLeave(text: "", shown: false))
}

@Test func theSearchButtonOpensThePagesFieldOnTheOneQuery() throws {
    // Read from the sources: the toolbar and the page are SwiftUI views over a live core.
    let root = try source("App/RootView.swift")
    let search = try source("Search/SearchScreen.swift")
    // Variant 4 (plans/owner-picks.md 2026-10-25): the window's top holds a round glass Search button, no field.
    let tools = try body(of: "private var tools: some View", in: root)
    #expect(tools.contains("Button(action: find)") && tools.contains(".buttonStyle(GlassCircleStyle())"))
    #expect(!root.contains("HeaderSearchField") && !root.contains("headerSearchField"))
    // So the page always draws its field, on the one query.
    #expect(search.contains("SearchField(input: input, prompt: Self.prompt, focused: $fieldFocused)"))
    #expect(try source("Search/SearchParts.swift").contains("TextField(prompt, text: $input.text)"))
    // Cmd-F and the button pop Search to its root and put the cursor in that field.
    let find = try body(of: "private func find()", in: root)
    #expect(find.contains("popToRoot(.search)") && find.contains("searchInput.focus += 1"))
    // Typing anywhere goes through the one rule.
    #expect(root.contains("SearchRules.bringsSearch(text: text, onSearch: tab == .search, titleOver: !(titlePaths[.search] ?? []).isEmpty)"))
    // A director chip writes the query too; the old parallel paths are gone.
    #expect(try source("Title/TitleScreen.swift").contains("searchInput?.link(name)"))
    for (path, text) in [("App/RootView.swift", root), ("Search/SearchScreen.swift", search), ("Title/TitleScreen.swift", try source("Title/TitleScreen.swift"))] {
        #expect(!text.contains("links?.search") && !text.contains("searchRequest"), "\(path) keeps an old search path")
    }
}

@Test func leavingSearchRemembersTheQueryBeforeClearingIt() throws {
    let search = try source("Search/SearchScreen.swift")
    let shown = try body(of: ".onChange(of: shown)", in: search)
    let remember = try #require(shown.range(of: "remember()"))
    let clear = try #require(shown.range(of: "input.text = \"\""))
    // The two reach the core in the order sent (CommandOrderTests).
    #expect(remember.upperBound <= clear.lowerBound)
}

@Test func onlyTheMacPutsTheCursorInSearchOnOpen() throws {
    // The Mac: the field is the way in, and an empty Search takes the cursor; a query kept (the way back
    // from a title) does not.
    #expect(SearchRules.focusesOnOpen(text: ""))
    #expect(!SearchRules.focusesOnOpen(text: "dune"))
    // The iPhone and iPad (read from the source; this suite runs on the Mac): no keyboard until the field
    // is tapped, so Browse by genre is not covered (the lead, from the 390 walk).
    let rule = try body(of: "static func focusesOnOpen(text: String) -> Bool", in: source("App/SearchInput.swift"))
    #expect(rule.contains("#if os(macOS)\n        text.isEmpty\n        #else\n        false\n        #endif"))
    #expect(try source("Search/SearchScreen.swift").contains(".onAppear { if SearchRules.focusesOnOpen(text: query) { fieldFocused = true } }"))
}

@Test func rememberSendsTheFieldsText() throws {
    // The core takes a typed query after its debounce; `remember {query}` keeps the text given instead, so a
    // quick Cmd-2 or a result opened right after the last keystroke keeps them (G9; core a1697990).
    #expect(fields(SearchRules.rememberCommand(text: " dune ")) == ["type": "remember", "query": "dune"])
    let search = try source("Search/SearchScreen.swift")
    #expect(try body(of: "private func remember()", in: search).contains("search.command(SearchRules.rememberCommand(text: query))"))
    // Both ways of remembering go through it: a result opened (the page disappears) and leaving on the Mac.
    #expect(search.contains(".onDisappear { if typed { remember() } }"))
    #expect(!search.contains("[\"type\": \"remember\"]"))
}

private func fields(_ command: [String: any Sendable]) -> [String: String] { command.mapValues { "\($0)" } }

@MainActor @Test func theSameDirectorChipAsksAgain() {
    // A chip naming the query already in the field changes no text, so it bumps a requery instead
    // (review-apple-player P3 2); Search's stack is popped by the link itself (opensAtRoot).
    let input = SearchInput()
    input.link("Christopher Nolan")
    #expect(input.text == "Christopher Nolan" && input.requery == 0)
    input.link("Christopher Nolan")
    #expect(input.requery == 1)
}
