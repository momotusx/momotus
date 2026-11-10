import Foundation
import Testing

// Touch controls keep a 44 pt frame: the player's pills, Picture in Picture, AirPlay and the season pull-down.

private func source(_ path: String) throws -> String {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    return try String(contentsOf: app.appending(path: path), encoding: .utf8)
}

@Test func theMinimumTargetIsFortyFourPoints() {
    #expect(Theme.minTarget == 44)
}

@Test func everyTouchControlTakesItsFrameFromTheMinimum() throws {
    // Read from the sources: a Menu's label builds no frames in a hosted view outside a running app.
    let controls = try source("Player/PlayerControls.swift")
    let pill = try body(of: "private func action(", in: controls)
    #expect(pill.contains(".frame(minWidth: Theme.minTarget, minHeight: Theme.minTarget)"), "the player's pills lost the minimum")
    // The bar's strips and its quick shift (plans/player-bar.md section 8): each chip, step, Close and each of the
    // shift's three parts.
    let strip = try source("Player/PlayerStrip.swift")
    let chip = try body(of: "struct StripChip<Mark: View>: View", in: strip)
    #expect(chip.contains(".frame(minWidth: Theme.minTarget, minHeight: Theme.minTarget)"), "the strip's chips lost the minimum")
    let shift = try body(of: "struct QuickShift: View", in: strip)
    #expect(shift.contains(".frame(minWidth: BarFit.shiftValue, minHeight: Theme.minTarget)"), "the shift's value lost the minimum")
    #expect(try body(of: "private func side(", in: shift).contains(".frame(minWidth: Theme.minTarget, minHeight: Theme.minTarget)"),
            "Earlier and Later lost the minimum")
    #expect(try body(of: "private func stepButton(", in: strip).contains(".frame(width: Theme.minTarget, height: Theme.minTarget)"))
    #expect(try body(of: "private func closeButton(", in: strip).contains(".frame(width: Theme.minTarget, height: Theme.minTarget)"))
    #expect(BarFit.shiftValue >= Theme.minTarget)
    let top = try body(of: "private var top: some View", in: controls)
    #expect(top.components(separatedBy: ".frame(width: Theme.minTarget, height: Theme.minTarget)").count - 1 >= 3,
            "Back, Picture in Picture and AirPlay each keep the minimum")
    let back = try body(of: "private var leaveBack: some ToolbarContent", in: source("Profiles/ManageProfiles.swift"))
    #expect(back.contains(".frame(minWidth: Theme.minTarget, minHeight: Theme.minTarget)"), "the editor's own Back lost the minimum")
    #expect(back.contains(".contentShape(Rectangle())"), "the frame alone does not widen the tap area")
    let shelf = try source("Title/TitleEpisodes.swift")
    let start = try #require(shelf.range(of: "struct SeasonShelf")).lowerBound
    let season = try body(of: "var body: some View", in: String(shelf[start...]))
    #expect(season.contains(".frame(minHeight: Theme.minTarget)"), "the season pull-down lost the minimum")
}

@Test func styleAndCloseStayInReachOfAStripWiderThanTheScreen() throws {
    // Pixel walk 2026-11-09 L21, the same row shape on the iPhone: the languages and the shift made the strip wider
    // than a phone across and Style and Close started past its edge. Only the row's start scrolls; they stay pinned.
    let strip = try source("Player/PlayerStrip.swift")
    let row = try body(of: "private struct StripRow<Content: View, Pinned: View>: View", in: strip)
    #expect(row.contains("ScrollView(.horizontal) { row }"))
    #expect(try body(of: "ViewThatFits(in: .horizontal)", in: row).contains("pinned") == false, "the pinned end scrolls")
    let subtitles = try body(of: "struct SubtitleStrip: View", in: strip)
    #expect(subtitles.contains("StripRow { mainRow } pinned: { mainEnd }"))
    let main = try body(of: "@ViewBuilder private var mainRow: some View", in: subtitles)
    #expect(!main.contains("closeButton(") && !main.contains("bar.style"), "Style or Close scrolls with the chips")
    let end = try body(of: "@ViewBuilder private var mainEnd: some View", in: subtitles)
    #expect(end.contains("closeButton(close)") && end.contains("title: bar.style"))
    let audio = try body(of: "struct AudioStrip: View", in: strip)
    #expect(audio.contains("} pinned: {\n            closeButton(close)"), "the audio strip's Close scrolls")
}

@Test func theMacToolbarHoldsSearchAloneAndTheOrnamentEndsWithTheProfile() throws {
    // Read from the sources: a toolbar is not laid out in a hosted view. The profile took Settings' place in the
    // ornament (plans/nav-profile.md), so it shows once: at the ornament's end, never as a toolbar item of its own.
    let root = try source("App/RootView.swift")
    let tools = try body(of: "private struct TopTools<Places: View>: ToolbarContent", in: root)
    #expect(tools.components(separatedBy: "ToolbarItem(placement: .primaryAction) { search }").count - 1 == 2, "Search keeps its item on both macOS paths")
    #expect(!tools.contains("Profile"), "the profile is back in the toolbar")
    let ornament = try #require(root.range(of: "PlaceOrnament(selection: tab, strong: ornamentStrong")).lowerBound
    #expect(root[ornament...].prefix(400).contains("ProfileMenu(onSettings: { select(.settings, link: true) }, onPin: pin, panel: $showPanel)"), "the ornament lost the profile's panel")
    #expect(root.contains("OrnamentProfile(lit: tab == .settings)"), "the ornament's profile is not lit on Settings")
}

@Test func thePhonesProfileTabKeepsTheMinimum() throws {
    let tab = try body(of: "private func profile(iconsOnly: Bool) -> some View", in: source("App/RootView.swift"))
    #expect(tab.contains(".frame(minWidth: Theme.minTarget, minHeight: Theme.minTarget)"), "the profile tab lost the minimum")
    #expect(tab.contains(".contentShape(Rectangle())"), "the frame alone does not widen the tap area")
}
