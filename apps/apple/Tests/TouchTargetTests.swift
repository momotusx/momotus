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
    #expect(pill.contains(".frame(minHeight: Theme.minTarget)"), "the player's pills lost the minimum")
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

@Test func theMacToolbarHoldsSearchAloneAndTheOrnamentEndsWithTheProfile() throws {
    // Read from the sources: a toolbar is not laid out in a hosted view. The profile took Settings' place in the
    // ornament (plans/nav-profile.md), so it shows once.
    let root = try source("App/RootView.swift")
    let tools = try body(of: "private struct TopTools: ToolbarContent", in: root)
    #expect(tools.components(separatedBy: "ToolbarItem(placement: .primaryAction) { search }").count - 1 == 2, "Search keeps its item on both macOS paths")
    #expect(!tools.contains("Profile"), "the profile is back in the toolbar")
    let ornament = try #require(root.range(of: ".placeOrnament(tab, strong: ornamentStrong")).lowerBound
    #expect(root[ornament...].prefix(400).contains("ProfileMenu(onSettings: { select(.settings, link: true) }, onPin: pin)"), "the ornament lost the profile's menu")
    #expect(root.contains("OrnamentProfile(lit: tab == .settings)"), "the ornament's profile is not lit on Settings")
}

@Test func thePhonesProfileTabKeepsTheMinimum() throws {
    let tab = try body(of: "private func profile(iconsOnly: Bool) -> some View", in: source("App/RootView.swift"))
    #expect(tab.contains(".frame(minWidth: Theme.minTarget, minHeight: Theme.minTarget)"), "the profile tab lost the minimum")
    #expect(tab.contains(".contentShape(Rectangle())"), "the frame alone does not widen the tap area")
}
