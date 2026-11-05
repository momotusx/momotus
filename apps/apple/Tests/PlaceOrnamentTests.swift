import AppKit
import Foundation
import SwiftUI
import Testing

// The Mac's ornament (plans/owner-picks.md): the four places and the profile, no wordmark, in the window's toolbar
// row at the top, centred between Back and Search (the owner, 2026-11-04: the places at the top on desktop; the
// capsule sat at the window's foot from 2026-10-25). Its size is laid out for real; where it sits is read from the
// sources, since a toolbar is not laid out in a hosted view.

@MainActor @Test func theOrnamentHoldsThePlacesButSettingsAndNoWordmark() {
    // Settings opens from the profile's menu at the ornament's end (plans/nav-profile.md).
    #expect(OrnamentLayout.places == [.home, .discover, .library, .calendar])
    // 40 tall, inside the toolbar row's 52 beside the 36 pt Search.
    #expect(OrnamentLayout.height == 40)
}

@MainActor @Test func theOrnamentFitsTheToolbarRowOfTheNarrowestWindow() {
    // With the profile at its end, as the shell draws it (a stand-in of the avatar item's size).
    let profile = Color.clear.frame(width: OrnamentLayout.itemHeight + 8, height: OrnamentLayout.itemHeight)
    let size = NSHostingView(rootView: PlaceOrnament(selection: .home, select: { _ in }, profile: profile)).fittingSize
    #expect(abs(size.height - OrnamentLayout.height) < 1, "the ornament is \(size)")
    // Centred, it keeps clear of the traffic lights and Back at the left and of Search at the right.
    #expect(size.width + 2 * OrnamentLayout.sideClear <= BackdropStops.macMinWidth, "the ornament is \(size.width) wide")
}

private func source(_ path: String) throws -> String {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    return try String(contentsOf: app.appending(path: path), encoding: .utf8)
}

@Test func theOrnamentIsTheToolbarsCentreAtTheTopNotAtTheFoot() throws {
    let root = try source("App/RootView.swift")
    let tools = try body(of: "private struct TopTools<Places: View>: ToolbarContent", in: root)
    // The centre is the places on both macOS paths, so Back stays at the left and Search at the right.
    #expect(tools.components(separatedBy: "ToolbarItem(placement: .principal) { centre }").count - 1 == 2, "the centre is not the places")
    #expect(tools.contains("if shown { places }"), "the centre does not hold the places")
    // The shell hands the ornament to the toolbar, hidden while the player covers the places.
    // The profile panel leaves them live for the pointer and hides them from VoiceOver (plans/profile-panel.md).
    #expect(root.contains("TopTools(find: find, shown: !immersion.active, covered: showPanel) {\n                    // Settings' first page"))
    #expect(root.contains("PlaceOrnament(selection: tab, strong: ornamentStrong, select: { select($0) }, profile:"))
    // Nothing is left at the window's foot.
    #expect(!root.contains(".placeOrnament("), "the ornament is back at the window's foot")
    #expect(!(try source("App/PlaceOrnament.swift")).contains("safeAreaInset(edge: .bottom"), "the ornament is back at the window's foot")
}
