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
