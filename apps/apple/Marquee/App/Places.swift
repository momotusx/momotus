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
