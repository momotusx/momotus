import SwiftUI

/// Settings A1 (the owner's pick of 2026-10-28, design/DECISION.md) in sections (plans/settings-sections.md, "On
/// A1"): the profile on a film's backdrop, then a section menu beside the chosen section in one centered column
/// (TV and wide screens), or a list of the sections, each opening its own page (the phone). The menu's entries.
enum SettingsSection: String, CaseIterable, Identifiable {
    case profiles, sources, account, home, playback, subtitles, downloads, device, about

    var id: String { rawValue }

    var title: String {
        switch self {
        case .profiles: "Profiles"
        case .sources: "Sources"
        case .account: "Account"
        case .home: "Home"
        case .playback: "Playback"
        case .subtitles: "Subtitles and audio"
        case .downloads: "Downloads"
        case .device: "This device"
        case .about: "About"
        }
    }

    var tone: SettingsTone {
        switch self {
        case .profiles: .blue
        case .sources: .green
        case .account: .sky
        case .home: .mint
        case .playback: .teal
        case .subtitles: .sky
        case .downloads: .blue
        case .device: .sea
        case .about: .grey
        }
    }

    /// The phone's list says what each section holds (the profile is the header).
    var summary: String {
        switch self {
        case .profiles: ""
        case .sources: "Addons, plugins, debrid services, Trakt and import"
        case .account: "Sign in and sync with your other devices"
        case .home: "Which rows Home shows"
        case .playback: "Stream choice and up next"
        case .subtitles: "Subtitle and audio languages, size and a second line"
        case .downloads: "Smart downloads and storage"
        case .device: "Settings kept on this device only"
        case .about: "Version, credits and licences"
        }
    }

    var glyph: String {
        switch self {
        case .profiles: "user"
        case .sources: "puzzle"
        case .account: "monitor-smartphone"
        case .home: "rows-3"
        case .playback: "circle-play"
        case .subtitles: "captions"
        case .downloads: "arrow-down-to-line"
        case .device: "monitor"
        case .about: "info"
        }
    }

    /// This device holds the trailer previews switch; tvOS has none, so it has no such section (as Android TV).
    /// Downloads shows where the device downloads (plans/offline-downloads.md 3.7), never on TV. Incognito has
    /// nothing of the profiles, the account or Downloads' settings (plans/batch-10-incognito.md 6). A kids profile
    /// has no sources, account, Home rows or Downloads' settings: none is its to change, and the core refuses each
    /// with `kids` (plans/m11-kids.md 5).
    static func shown(tv: Bool, downloads: Bool, incognito: Bool = false, kids: Bool = false) -> [SettingsSection] {
        allCases.filter { s in
            switch s {
            case .profiles: !incognito
            case .account: !incognito && !kids
            case .sources, .home: !kids
            case .device: !tv
            case .downloads: downloads && !tv && !incognito && !kids
            default: true
            }
        }
    }

    /// The section beside the menu: the one chosen while it shows, else the first after the header (a kids
    /// profile's Settings opens on Playback, not on a Sources it has not got).
    static func beside(_ chosen: SettingsSection, in shown: [SettingsSection]) -> SettingsSection {
        shown.contains(chosen) ? chosen : shown.first { $0 != .profiles } ?? .about
    }
}

/// An icon tile's fill: one of the blue-to-green steps (an owner-picked exception to the one key color), never
/// red or orange; a white glyph keeps 3:1 on each.
enum SettingsTone: String, CaseIterable {
    case blue, sky, sea, teal, mint, green, grey

    /// Its name in design/tokens.json.
    var token: String { "tile" + rawValue.prefix(1).uppercased() + rawValue.dropFirst() }

    var fill: Color {
        switch self {
        case .blue: Theme.Palette.tileBlue
        case .sky: Theme.Palette.tileSky
        case .sea: Theme.Palette.tileSea
        case .teal: Theme.Palette.tileTeal
        case .mint: Theme.Palette.tileMint
        case .green: Theme.Palette.tileGreen
        case .grey: Theme.Palette.tileGrey
        }
    }
}

enum SettingsRules {
    /// The page's sizes per form: the web's TV points, its desktop frame (1440 wide) on the Mac and iPad, the
    /// iPhone's own.
    struct Sizes {
        let column, menu, gap, header, avatar, name, line, row, rowTile, menuRow, menuTile, heading, radius, sectionGap, foot: CGFloat
        let logo: CGSize

        static let tv = Sizes(
            column: 1600, menu: 420, gap: 64, header: 600, avatar: 168, name: Theme.typeTV.title1, line: Theme.typeTV.body,
            row: 104, rowTile: 48, menuRow: 84, menuTile: 48, heading: Theme.typeTV.title3, radius: Theme.Radius.l,
            sectionGap: 56, foot: 64, logo: CGSize(width: 380, height: 150))
        static let wide = Sizes(
            column: 1120, menu: 236, gap: 56, header: 380, avatar: 124, name: 50, line: 18,
            row: 64, rowTile: 32, menuRow: 44, menuTile: 28, heading: 22, radius: 12,
            sectionGap: 28, foot: 36, logo: CGSize(width: 230, height: 96))
        static let phone = Sizes(
            column: .infinity, menu: 0, gap: 0, header: 340, avatar: 56, name: 19, line: 13,
            row: 51, rowTile: 28, menuRow: 0, menuTile: 0, heading: 15, radius: 26,
            sectionGap: 30, foot: 18, logo: .zero)
    }

    /// The side space that centers the content column in a `width` page, never under the page's gutter.
    static func pad(width: CGFloat, gutter: CGFloat, column: CGFloat) -> CGFloat {
        guard column.isFinite else { return gutter }
        return max(gutter, (width - column) / 2)
    }

    /// The header's art: the first Continue Watching title with a backdrop, else Home's first hero with one; a
    /// kids profile its own Continue Watching only. Decided once Home's rows are in, so a hero's art never shows
    /// first and gives way to Continue Watching's.
    static func art(rows: [HomeRow], heroes: [Hero], kids: Bool) -> Tile? {
        guard !rows.isEmpty else { return nil }
        if let watching = rows.first(where: { $0.rowId == continueRow })?.tiles.first(where: { $0.background != nil }) {
            return watching
        }
        return kids ? nil : heroes.first { $0.tile.background != nil }?.tile
    }

    static let continueRow = "continue-watching"

    /// How far the menu moves down to stay in sight once its column scrolls past `stick`: never above where it
    /// sits, never past the end of the lists.
    static func menuShift(columnTop: CGFloat, stick: CGFloat, room: CGFloat) -> CGFloat {
        min(max(0, stick - columnTop), max(0, room))
    }

    /// Edit profile and Manage profiles: a kids profile has neither (leaving it needs the PIN); Manage is the owner's.
    static func canEdit(kids: Bool) -> Bool { !kids }
    static func canManage(kids: Bool, owner: Bool) -> Bool { !kids && owner }

    static func profileLine(many: Bool) -> String {
        many ? "Switch profile" : "Add a profile for someone else, or switch"
    }
}

extension View {
    /// The Settings header's one height from its first frame: `art` is drawn behind and can never grow or
    /// shrink it; only words taller than the header (large text) grow it.
    func settingsHeader<Art: View>(height: CGFloat, @ViewBuilder art: () -> Art) -> some View {
        frame(maxWidth: .infinity, minHeight: height, alignment: .bottomLeading)
            .background(alignment: .bottom) { art() }
    }
}
