import AppKit
import Foundation
import SwiftUI
import Testing

// The narrowest iPhone (375 pt) on the token's inset (20): the phone rows with fixed parts still fit
// (the lead, 2026-10-23). The device shots at 390 show the rest; the title hero is laid out at 320 in
// TitleHeroTests.

private let content = 375 - 2 * Metrics.compact.gutter

@Test func thePhonesFixedRowsFitAt375() {
    #expect(content == 335)
    // Discover: two covers and their gap, each wide enough for a two-line name.
    let cover = DiscoverLayout.phonePoster(width: content)
    #expect(2 * cover.width + Theme.Tile.Grid.gap <= content && cover.width >= 150)
    // Browse by genre: three tiles and two gaps.
    let tile = GenreGridRules.tile(width: content)
    #expect(3 * tile.width + 2 * GenreGridRules.gap <= content + 0.01 && tile.width >= 100)
    // A search result: the thumb, its gap, and 200 pt for the name.
    #expect(Theme.Search.thumb.width + 12 + 200 <= content)
    // Home's hero: Play (178), 12, the 48 pt circle (HeroView.swift).
    #expect(178 + 12 + 48 <= content)
}

@MainActor @Test func fourKindsFitTheSegmentedControlAt375() {
    // The core's longest set of kinds, at the phone's 15/600: each segment its words and 28 pt, the
    // track's 6, so no kind's name is cut.
    let width = { (text: String) in
        NSHostingView(rootView: Text(text).textFont(15, weight: .semibold, relativeTo: .subheadline).fixedSize()).fittingSize.width
    }
    let needed = ["Movies", "Series", "Channels", "Live TV"].map { width($0) + 28 }.reduce(6, +)
    #expect(needed <= content, "the segments need \(needed)")
}

@Test func calendarSettingsAndThePlayersActionsFitAt375() throws {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    func read(_ path: String) throws -> String { try String(contentsOf: app.appending(path: path), encoding: .utf8) }
    // Calendar: a flexible column inside the gutter, no fixed width wider than the phone's content.
    let calendar = try read("Library/CalendarScreen.swift")
    #expect(calendar.contains(".frame(maxWidth: 880, alignment: .leading)") && calendar.contains(".padding(.horizontal, metrics.gutter)"))
    let fixed = calendar.matches(of: /\.frame\(width: (\d+)/).compactMap { Double($0.1) }
    #expect(fixed.allSatisfy { $0 <= content }, "Calendar fixes \(fixed)")
    // A Settings row: its padding, the 28 pt glyph, the gaps and the chevron leave the words 200 pt or more,
    // and the words wrap (no line limit).
    #expect(2 * Theme.Space.s + 28 + Theme.Space.s + Theme.Space.s + 20 + 200 <= content)
    let row = try body(of: "struct SettingsRow: View", in: read("Settings/SettingsScreen.swift"))
    #expect(!row.contains("lineLimit"))
    // The player's actions on a narrow window (PlayerControls.swift:76, not the iPhone's own layout) scroll
    // sideways rather than overflow.
    let controls = try read("Player/PlayerControls.swift")
    #expect(controls.contains("} else {\n                scrolling(actionRow(values: true))\n            }"))
}
