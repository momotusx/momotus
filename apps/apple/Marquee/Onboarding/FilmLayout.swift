import CoreGraphics

/// The Mac's first-run sheet: the window's size, so the poster wall runs edge to edge, never under its floor.
enum FilmRules {
    static let sheetFloor = CGSize(width: 760, height: 640)

    static func macSheet(window: CGSize) -> CGSize {
        CGSize(width: max(window.width, sheetFloor.width), height: max(window.height, sheetFloor.height))
    }
}
