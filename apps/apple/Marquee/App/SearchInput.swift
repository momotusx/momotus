import Observation

/// The one search query (plans/design-v3-phase7-apple.md 2): the Mac header's field, the page's
/// field, a Recent row and a title page's director chip all write `text`.
@MainActor @Observable final class SearchInput {
    var text = ""
    /// Bumped to put the cursor in whichever field shows: Cmd-F, the narrow header's glyph, a window
    /// crossing the header field's width while Search shows.
    var focus = 0
}

/// The field's rules, apart from the views for the tests.
enum SearchRules {
    /// Typing shows Search's first page: from another place, or over a title opened from the results,
    /// whose results would otherwise change unseen.
    static func bringsSearch(text: String, onSearch: Bool, titleOver: Bool) -> Bool {
        !text.isEmpty && (!onSearch || titleOver)
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
}
