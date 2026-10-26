import CoreGraphics

/// Discover's layout from the window's width (plans/design-v3-phase7-apple.md 4.2, 4.3).
enum DiscoverLayout {
    /// The page's side margin on the Mac and iPad.
    static let margin: CGFloat = 40
    static let poster = Theme.TileDesktop.Grid.poster
    static let gap = Theme.TileDesktop.Grid.gap

    /// The preview shows beside at least four columns: 1120 with the tokens of the day.
    static func previewFits(width: CGFloat) -> Bool {
        width >= 2 * margin + 4 * poster.width + 3 * gap + Theme.Preview.gap + Theme.Preview.width
    }

    /// The grid's columns in a window `width` wide, beside the preview when it shows.
    static func columns(width: CGFloat) -> Int {
        let preview = previewFits(width: width) ? Theme.Preview.gap + Theme.Preview.width : 0
        return max(1, Int(((width - 2 * margin - preview + gap) / (poster.width + gap)).rounded(.down)))
    }

    /// The iPhone's cover in a grid `width` wide: the columns share it, 2:3.
    static func phonePoster(width: CGFloat) -> CGSize {
        let grid = Theme.Tile.Grid.self
        let side = ((width - grid.gap * CGFloat(grid.columns - 1)) / CGFloat(grid.columns)).rounded(.down)
        return CGSize(width: side, height: (side * grid.aspect).rounded())
    }
}

/// How a screen offers the core's kinds (DECISION Grids). Search: a segmented control for two to four, a
/// pull-down for more, nothing for one (`for(count:)`). Discover: `DiscoverWords.kindControl`.
enum KindControl: Equatable {
    case none, segmented, pullDown

    static func `for`(count: Int) -> KindControl {
        switch count {
        case ..<2: .none
        case 2...4: .segmented
        default: .pullDown
        }
    }
}

/// The selected poster on the Mac and iPad, where the preview shows it: a first activation selects,
/// the selected one opens.
struct DiscoverSelection: Equatable {
    /// The tiles the preview finds its selection in: the grid's. While a change loads, the grid is the list before
    /// (dimmed), so the preview stays on what is on screen instead of closing and opening again.
    static func tiles(_ s: DiscoverSnapshot?, previous: DiscoverSnapshot?) -> [Tile] {
        guard let s else { return [] }
        if s.items.isEmpty, s.state == .loading, let previous { return previous.items }
        return s.items
    }

    private(set) var key: String?
    /// The kind, catalog and genre the selection belongs to.
    private var scope: String?

    /// A new kind, catalog or genre (or a first page) selects the first poster; more of the same
    /// catalog keeps the selection.
    mutating func update(scope: String, keys: [String]) {
        if scope != self.scope || key.map({ !keys.contains($0) }) ?? true {
            self.scope = scope
            key = keys.first
        }
    }

    /// A click, Return or Space on `key`: true opens it (it was selected), else it becomes selected.
    mutating func activate(_ key: String) -> Bool {
        if self.key == key { return true }
        self.key = key
        return false
    }

    /// Focus (the keyboard's or VoiceOver's) or the custom action moved here.
    mutating func select(_ key: String) { self.key = key }

    enum Move { case left, right, up, down }

    /// The index an arrow key moves to in a grid of `count` posters, `columns` a row; it stays put at an edge.
    static func moved(from index: Int, _ move: Move, columns: Int, count: Int) -> Int {
        let to: Int
        switch move {
        case .left: to = index % columns == 0 ? index : index - 1
        case .right: to = index % columns == columns - 1 ? index : index + 1
        case .up: to = index - columns
        case .down: to = index + columns
        }
        return (0..<count).contains(to) ? to : index
    }
}

/// The item Discover asks the core's details of (`preview {id}`), with the list it is in: a new list
/// clears the core's details, so the same item in it is asked again.
struct DiscoverPreviewAsk: Equatable {
    let id: String
    let list: String
}

/// Search's Browse by genre on the phone (plans/design-v3-genre-tiles.md).
enum GenreGridRules {
    static let columns = 3
    static let gap: CGFloat = 12
    /// The share of black under the name, from 38 pt into the 90 pt ramp to the tile's foot: white
    /// keeps 4.5:1 over white art on it.
    static let rampFloor = 0.62

    /// A tile in a grid `width` wide: three 2:3 tiles sharing it.
    static func tile(width: CGFloat) -> CGSize {
        let side = (width - gap * CGFloat(columns - 1)) / CGFloat(columns)
        return CGSize(width: side, height: side * 1.5)
    }

    /// The kinds the grid offers: those with genres, in the core's order.
    static func kinds(_ s: GenresSnapshot) -> [KindGenres] { s.kinds.filter { !$0.genres.isEmpty } }
}
