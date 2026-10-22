import AppKit
import SwiftUI
import Testing

// Every catalog tile is a poster, whatever shape its addon draws the catalog in, as Stremio shows them (the
// owner, 2026-10-23); only Continue Watching keeps 16:9 stills. Laid out for real at each form's poster size.

@MainActor private final class Box { var size = CGSize.zero }

private func tile(_ id: String, shape: String) throws -> Tile {
    let json = """
    {"id": "\(id)", "kind": "movie", "name": "Wide", "poster": null, "background": null, "logo": null,
     "posterShape": "\(shape)", "releaseInfo": "2026", "progress": null, "videoId": null, "rating": null}
    """
    return try JSONDecoder().decode(Tile.self, from: Data(json.utf8))
}

@MainActor @Test(arguments: ["poster", "landscape", "square"])
func aCatalogTileIsAPosterWhateverItsAddonsShape(shape: String) throws {
    // A non-IMDb id: the core leaves its addon's shape (an IMDb title's cover is RPDB's 2:3, shape "poster").
    let tile = try tile("kitsu:1", shape: shape)
    for metrics in [Metrics.compact, Metrics.regular] {
        let box = Box()
        let view = PosterTile(tile: tile)
            .onGeometryChange(for: CGSize.self) { $0.size } action: { box.size = $0 }
            .environment(\.metrics, metrics)
            .fixedSize()
        let host = NSHostingView(rootView: view)
        host.frame = CGRect(x: 0, y: 0, width: 600, height: 400)
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.1))
        #expect(box.size == metrics.poster, "a \(shape) tile laid out at \(box.size), not the poster's \(metrics.poster)")
    }
}
