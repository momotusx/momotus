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

@Test func theMacToolbarGivesSearchAndTheProfileButtonAnItemEach() throws {
    // Read from the sources: a toolbar is not laid out in a hosted view.
    let tools = try body(of: "private struct TopTools: ToolbarContent", in: source("App/RootView.swift"))
    #expect(!tools.contains("HStack"), "Search and the profile button share one toolbar item again")
    #expect(tools.components(separatedBy: "ProfileButton(size: 32, action: profiles)").count - 1 == 2, "the profile button needs its own item on both macOS paths")
}
