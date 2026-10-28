import AppKit
import SwiftUI
import Testing

// Every sideways strip of cards comes to rest on a card, lined up with the page's text column (the owner,
// 2026-10-28: a trackpad swipe on the Mac stopped between two cards). The snap itself runs only under a real
// gesture; these pin its wiring and lay the strip out for real to check where its first card sits.

private func source(_ path: String) throws -> String {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    return try String(contentsOf: app.appending(path: path), encoding: .utf8)
}

@MainActor private final class Box { var x: CGFloat? }

@MainActor @Test func aShelfStartsOnTheTextColumn() {
    let box = Box()
    let view = Shelf(spacing: 10, inset: 40, room: 12) {
        ForEach(0..<8, id: \.self) { i in
            Color.gray.frame(width: 100, height: 150)
                .onGeometryChange(for: CGFloat.self) { $0.frame(in: .global).minX } action: { if i == 0 { box.x = $0 } }
        }
    }
    let host = NSHostingView(rootView: view.frame(width: 500, height: 200))
    host.frame = CGRect(x: 0, y: 0, width: 500, height: 200)
    host.layoutSubtreeIfNeeded()
    RunLoop.main.run(until: Date().addingTimeInterval(0.1))
    #expect(box.x == 40, "the first card sits at \(String(describing: box.x)), not on the text column at 40")
}

@Test func aShelfSnapsToACardOnMacAndPhone() throws {
    let shelf = try source("Theme/Components.swift")
    #expect(shelf.contains("LazyHStack(alignment: .top, spacing: spacing) { content }\n                .scrollTargetLayout()"))
    // The inset is a content margin: the snap aligns a card with it, where padding would align it with the edge.
    #expect(shelf.contains(".contentMargins(.horizontal, inset, for: .scrollContent)"))
    #expect(shelf.contains("#if !os(tvOS)\n        .scrollTargetBehavior(.viewAligned)\n        #endif"))
}

@Test func everyCardStripIsAShelf() throws {
    #expect(try source("Home/RowView.swift").contains("Shelf(spacing: metrics.wide ? Theme.TileDesktop.gap : Theme.Tile.gap, inset: metrics.gutter, room: 12)"))
    #expect(try source("Title/SimilarRow.swift").contains("Shelf(spacing: metrics.wide ? Theme.TileDesktop.gap : Theme.Tile.gap, inset: metrics.wide ? metrics.gutter : 20, room: 12)"))
    let title = try source("Title/TitleEpisodes.swift")
    #expect(title.contains("Shelf(spacing: metrics.wide ? Theme.TileDesktop.gap : Theme.Tile.gap, room: 8)"))
    #expect(title.contains("Shelf(spacing: 20) {"))
    // No strip of cards scrolls sideways on its own; the hero pages, a page at a time, as it always did.
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    let files = FileManager.default.enumerator(atPath: app.path)?.compactMap { $0 as? String }.filter { $0.hasSuffix(".swift") } ?? []
    #expect(files.count > 100)
    for file in files where file != "Theme/Components.swift" && file != "Home/HeroCarousel.swift" {
        let text = try source(file)
        #expect(text.range(of: #"ScrollView\(\.horizontal\) \{\s*LazyHStack"#, options: .regularExpression) == nil, "\(file) has a card strip outside Shelf")
    }
}
