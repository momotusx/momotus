import Foundation
import Testing

// The title panel's season pill and Discover's filters are one view (design v3 phase 7, 4.1).

private func source(_ path: String) throws -> String {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    return try String(contentsOf: app.appending(path: path), encoding: .utf8)
}

@Test func theSeasonPillKeepsItsPhaseThreeLookOnTheSharedPill() throws {
    // Read from the sources: a Menu's label builds no frames in a hosted view outside a running app.
    let pill = try body(of: "var body: some View", in: source("Common/PullDownPill.swift"))
    #expect(try source("Common/PullDownPill.swift").contains("var size: CGFloat = 15"))
    for part in [
        "Glyph(\"chevron-down\", size: 14)", ".padding(.horizontal, 14)",
        ".frame(height: metrics.wide ? Theme.Pill.desktop : Theme.Pill.phone)",
        ".background(.white.opacity(0.1), in: Capsule())", ".menuIndicator(.hidden)", ".fixedSize()",
    ] {
        #expect(pill.contains(part), "PullDownPill lost \(part)")
    }
    #expect((Theme.Pill.desktop, Theme.Pill.phone) == (32, 44))
    let menu = try body(of: "private var seasonMenu: some View", in: source("Title/TitlePanel.swift"))
    #expect(menu.contains("PullDownPill(title: seasonName(selected ?? ordered.first ?? 1))"))
    #expect(!menu.contains("size:"), "the season pill takes the default 15")
}
