import Foundation
import Testing

// The Mac window's top right: Search and the profile sit the page's gutter in from the window's edge, as the page's
// content does from the left (the owner, 2026-10-28: flush in the corner). Measured in the walk build's window; these
// pin the lines that place them.

@Test func theMacsTopToolsKeepThePagesGutterFromTheWindowsEdge() throws {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    let root = try String(contentsOf: app.appending(path: "App/RootView.swift"), encoding: .utf8)
    #expect(root.contains("static let edge = Metrics.regular.gutter - 4.5"))
    #expect(root.contains("ProfileButton(size: 32, action: profiles).padding(.trailing, Self.edge)"))
    #expect(Metrics.regular.gutter == 40)
}
