import Foundation

enum AppTab: String, CaseIterable, Hashable {
    case home, discover, library, calendar, search, settings

    var title: String { rawValue.capitalized }

    var symbol: String {
        switch self {
        case .home: "house"
        case .discover: "compass"
        case .library: "library-big"
        case .calendar: "calendar"
        case .search: "search"
        case .settings: "settings"
        }
    }

    /// Search and Settings sit after the divider in the top navigation.
    var isTool: Bool { self == .search || self == .settings }

    /// The places the navigation shows. The iPhone's bar has five: Calendar is a row at the top
    /// of Library there (D6).
    static var bar: [AppTab] {
        #if os(iOS)
        allCases.filter { $0 != .calendar }
        #else
        allCases
        #endif
    }

    /// The place the navigation lights for this one: on the iPhone, Library while Calendar is open.
    var lit: AppTab {
        #if os(iOS)
        self == .calendar ? .library : self
        #else
        self
        #endif
    }
}

extension AppTab {
    /// Whether opening this place first goes back to its first page: the current tab again on the
    /// iPhone and iPad, as a tab bar does, and on every platform a title page's chip, whose request
    /// only the place's first page reads. A title opened from Search kept the Mac's Director chip
    /// dead (review design-v3-phase3-apple-code P1-1).
    func opensAtRoot(from current: AppTab, link: Bool) -> Bool {
        #if os(iOS)
        link || self == current
        #else
        link
        #endif
    }
}

/// Where a tile leads: the title page for one id.
struct TitleRoute: Hashable {
    let kind: String
    let id: String
    let name: String
    /// The season to show first (from the calendar).
    var season: Int?
    /// The episode a Calendar link names: the panel (or the Episodes page) opens on it.
    var videoId: String?

    init(kind: String, id: String, name: String, season: Int? = nil, videoId: String? = nil) {
        (self.kind, self.id, self.name, self.season, self.videoId) = (kind, id, name, season, videoId)
    }

    init(_ tile: Tile) {
        self.init(kind: tile.kind, id: tile.id, name: tile.name)
    }
}

/// What a sheet's flow does with the Go menu's Back, written by `FullScreen` for the pages inside it.
enum SheetBack {
    /// Not in a sheet flow: a pushed page leaves, a root gives none.
    case none
    /// The flow's root closes the sheet (a stack's root in a sheet may not read as presented).
    case close(() -> Void)
    /// The flow keeps Back to itself (onboarding): its pages give none.
    case off

    /// The page is inside a sheet's flow (where the toolbar draws only some placements).
    var isInSheet: Bool { if case .none = self { false } else { true } }
}

/// What the Go menu's Back does on a page.
enum BackRoute {
    case none
    /// Leaves the page: a pushed page pops, a sheet's root closes its sheet.
    case dismiss
    case close(() -> Void)

    var isNone: Bool { if case .none = self { true } else { false } }
    var closes: Bool { if case .close = self { true } else { false } }
}

func backRoute(presented: Bool, sheet: SheetBack) -> BackRoute {
    switch sheet {
    case .off: .none
    case .close(let action): presented ? .dismiss : .close(action)
    case .none: presented ? .dismiss : .none
    }
}

/// Whether a sheet is over the pages beneath it: this one, or one it is nested in (a writer that only
/// set its own value would clear the outer one).
func sheetCovers(outer: Bool, own: Bool) -> Bool {
    outer || own
}

/// Whether the page in front gives the Go menu its Back (Cmd-[): it is pushed (or presented), its
/// place is the one shown, nothing was pushed over it, and no sheet is over the places (the sheet's own
/// page answers Back then, so the title under it is never popped). `presented` is whether `backRoute` found
/// something to do.
func canGoBack(shown: Bool, inFront: Bool, presented: Bool, sheetOver: Bool = false) -> Bool {
    shown && inFront && presented && !sheetOver
}

/// The system back is hidden for a place out of sight (it would pop a page nobody sees) and while the
/// page holds edits (its own Back asks first).
func backButtonHidden(shown: Bool, holdsEdits: Bool) -> Bool {
    !shown || holdsEdits
}
