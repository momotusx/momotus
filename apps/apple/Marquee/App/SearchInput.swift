import Observation

/// The one search query (plans/design-v3-phase7-apple.md 2): the Mac header's field, the page's
/// field, a Recent row and a title page's director chip all write `text`.
@MainActor @Observable final class SearchInput {
    var text = ""
    /// Bumped to put the cursor in whichever field shows: Cmd-F, the narrow header's glyph, a window
    /// crossing the header field's width while Search shows.
    var focus = 0
    /// Bumped by Return in a field: focus moves to the first result.
    var submit = 0
    /// Bumped when a link asks for the query already in the field: Search asks the core again.
    var requery = 0

    /// A title page's director chip: the name in the field, or the same query asked again.
    func link(_ name: String) {
        if text == name { requery += 1 } else { text = name }
    }
}

/// The field's rules, apart from the views for the tests.
enum SearchRules {
    /// Typing shows Search's first page: from another place, or over a title opened from the results,
    /// whose results would otherwise change unseen.
    static func bringsSearch(text: String, onSearch: Bool, titleOver: Bool) -> Bool {
        !text.isEmpty && (!onSearch || titleOver)
    }

    /// Top results' columns on the Mac and iPad: cards at least 360 wide, 20 apart, inside 40 margins,
    /// three at most (three at 1440, two at 1024, one at 720).
    static func topResultColumns(width: Double) -> Int {
        min(3, max(1, Int(((width - 80 + 20) / (360 + 20)).rounded(.down))))
    }

    /// Search opening empty takes the cursor on the Mac (the field is the way in); on a touch screen the
    /// keyboard waits for a tap, as the TV and Music apps, so the browse grid is not covered.
    static func focusesOnOpen(text: String) -> Bool {
        #if os(macOS)
        text.isEmpty
        #else
        false
        #endif
    }

    enum Escape: Equatable { case clear, leave }

    /// Escape in a field with text clears it and keeps the cursor; in an empty one it lets go.
    static func escape(_ text: String) -> Escape { text.isEmpty ? .leave : .clear }

    /// Leaving Search for another place on the Mac, where the header's field shows everywhere: the
    /// query is remembered, then cleared. A title pushed from the results is not leaving.
    static func clearsOnLeave(text: String, shown: Bool) -> Bool {
        #if os(macOS)
        !shown && !text.isEmpty
        #else
        false
        #endif
    }

    /// `remember` keeps the core's query, which lands after its debounce: leaving Search before the typed
    /// query landed waits for it, so the old query is never the one remembered.
    static func remembersNow(text: String, answered: String?) -> Bool {
        answered == text.trimmingCharacters(in: .whitespaces)
    }
}
