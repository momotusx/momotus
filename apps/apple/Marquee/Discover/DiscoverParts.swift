import SwiftUI

#if !os(tvOS)
/// Discover's controls on the Mac, iPad and iPhone (plans/discover-v2.md 6.1, the owner's pick): Movies,
/// Series and Anime as a segmented control with any other kind in a More pull-down, the services as
/// name tiles with All first, then Genre and Year, Clear when a filter is set, and Sort at the end.
/// Every word comes from `DiscoverWords`.
struct DiscoverFilters: View {
    let snapshot: DiscoverSnapshot
    let select: ([String: any Sendable]) -> Void
    @Environment(\.metrics) private var metrics

    var body: some View {
        VStack(alignment: .leading, spacing: metrics.wide ? 18 : 12) {
            HStack(spacing: 12) { kinds; more }
            rail
            filters
            missing
        }
    }

    @ViewBuilder private var kinds: some View {
        let segments = DiscoverWords.segments(snapshot)
        if segments.count > 1 || !DiscoverWords.more(snapshot).isEmpty {
            KindSegment(items: segments, selected: snapshot.kind) { select(["kind": $0]) }
        }
    }

    @ViewBuilder private var more: some View {
        let s = snapshot
        if !DiscoverWords.more(s).isEmpty {
            pull(DiscoverWords.moreTitle(s)) {
                ForEach(DiscoverWords.more(s), id: \.id) { item in
                    choice(item.title, on: item.id == s.kind) { select(["kind": item.id]) }
                }
            }
        }
    }

    /// The services as their logos (the name when there is none), All first; the chosen one ringed and kept in view,
    /// with room at the ends for its ring.
    @ViewBuilder private var rail: some View {
        let services = DiscoverWords.services(snapshot)
        if !services.isEmpty {
            ScrollViewReader { proxy in
                ScrollView(.horizontal) {
                    HStack(spacing: metrics.wide ? 10 : 8) {
                        ForEach(services, id: \.title) { item in
                            let on = item.id == snapshot.platform
                            Button { select(["platform": item.id ?? NSNull()]) } label: {
                                if let logo = item.logo {
                                    ArtImage(url: logo, fallback: item.title)
                                        .frame(width: metrics.wide ? 44 : 46, height: metrics.wide ? 44 : 46)
                                } else {
                                    Text(item.title)
                                }
                            }
                            .buttonStyle(ServiceTileStyle(on: on, wide: metrics.wide, logo: item.logo != nil))
                            .accessibilityLabel(item.title)
                            .accessibilityAddTraits(on ? .isSelected : [])
                            .id(item.title)
                        }
                    }
                    .padding(.vertical, 8)
                    .padding(.horizontal, 8)
                }
                .scrollIndicators(.hidden)
                .scrollClipDisabled()
                .onChange(of: snapshot.platform, initial: true) { _, chosen in
                    if let title = services.first(where: { $0.id == chosen })?.title { proxy.scrollTo(title) }
                }
            }
            .accessibilityElement(children: .contain)
            .accessibilityLabel("Service")
        }
    }

    @ViewBuilder private var filters: some View {
        let s = snapshot
        HStack(spacing: metrics.wide ? 12 : 18) {
            if !DiscoverWords.genreChoices(s).isEmpty, !(s.genreChoices ?? []).isEmpty || !s.genres.isEmpty {
                pull(DiscoverWords.genreTitle(s), field: s.genreLabel ?? "Genre") {
                    ForEach(DiscoverWords.genreChoices(s), id: \.title) { item in
                        choice(item.title, on: item.id == s.genre) { select(["genre": item.id ?? NSNull()]) }
                    }
                }
            }
            if !DiscoverWords.decades(s).isEmpty {
                pull(DiscoverWords.decadeTitle(s), field: "Year") {
                    ForEach(DiscoverWords.decades(s), id: \.title) { item in
                        choice(item.title, on: item.id == s.decade) { select(["decade": item.id ?? NSNull()]) }
                    }
                }
            }
            if DiscoverWords.filtered(s) {
                Button("Clear") { select(["platform": NSNull(), "genre": NSNull(), "decade": NSNull()]) }
                    .buttonStyle(.plain)
                    .textFont(metrics.wide ? 13 : 17, weight: .semibold, relativeTo: .body)
                    .foregroundStyle(Theme.Palette.key)
                    .accessibilityLabel("Clear filters")
            }
            Spacer(minLength: 0)
            if !DiscoverWords.sorts(s).isEmpty {
                pull(DiscoverWords.sort(s), field: "Sort") {
                    ForEach(DiscoverWords.sorts(s), id: \.id) { item in
                        choice(item.title, on: item.id == s.sort) { select(["sort": item.id]) }
                    }
                }
            }
        }
    }

    @ViewBuilder private var missing: some View {
        if let line = DiscoverWords.missingGenre(snapshot.missingGenre) {
            Text(line)
                .textFont(13, relativeTo: .footnote)
                .foregroundStyle(Theme.Palette.dim)
                .accessibilityElement(children: .combine)
        }
    }

    @ViewBuilder private func pull<Items: View>(_ title: String, field: String? = nil, @ViewBuilder items: @escaping () -> Items) -> some View {
        Group {
            if metrics.wide {
                PullDownPill(title: title, size: 13, items: items)
            } else {
                PhonePull(title: title, items: items)
            }
        }
        // Spoken as the field and its value ("Genre, Drama").
        .accessibilityLabel(field.map { "\($0), \(title)" } ?? title)
    }

    private func choice(_ title: String, on: Bool, _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            if on { Label(title, glyph: "check") } else { Text(title) }
        }
        .accessibilityAddTraits(on ? .isSelected : [])
    }
}

/// A service on the rail (plans/discover-v2.md 6.6, the owner, 2026-10-26): its logo on a rounded square, or
/// its name on a neutral tile as wide as the name when there is no logo; the chosen one has a white ring.
private struct ServiceTileStyle: ButtonStyle {
    let on: Bool
    let wide: Bool
    var logo = false

    func makeBody(configuration: Configuration) -> some View {
        Tile(configuration: configuration, on: on, wide: wide, logo: logo)
    }

    private struct Tile: View {
        let configuration: ButtonStyleConfiguration
        let on: Bool
        let wide: Bool
        let logo: Bool
        @KeyboardFocused private var focused

        var body: some View {
            let shape = RoundedRectangle(cornerRadius: 10, style: .continuous)
            configuration.label
                .textFont(wide ? 14 : 15, weight: .semibold, relativeTo: .subheadline)
                .foregroundStyle(on ? Color.white : Theme.Palette.muted)
                .lineLimit(1)
                .fixedSize()
                .padding(.horizontal, logo ? 0 : (wide ? 18 : 14))
                .frame(minHeight: wide ? 44 : 46)
                .clipShape(shape)
                .background(shape.fill(on ? Theme.Palette.selectedFill : Theme.Palette.surface))
                .overlay { if on { shape.inset(by: -4).stroke(Color.white, lineWidth: 2) } }
                .contentShape(shape)
                .pressVeil(shape, pressed: configuration.isPressed)
                .modifier(FocusRing(shape: shape, active: focused))
        }
    }
}

/// The kinds as a segmented control: the track grey, the selected segment on `selected.fill` with white
/// words, 13/600 and 32 tall on the Mac and iPad, 15/600 and 44 on the iPhone.
struct KindSegment: View {
    let items: [(id: String, title: String)]
    let selected: String?
    let select: (String) -> Void
    @Environment(\.metrics) private var metrics

    var body: some View {
        HStack(spacing: 0) {
            ForEach(items, id: \.id) { item in
                let on = item.id == selected
                Button(item.title) { select(item.id) }
                    .buttonStyle(SegmentStyle(on: on, wide: metrics.wide))
                    .accessibilityAddTraits(on ? .isSelected : [])
            }
        }
        .padding(3)
        .background(Capsule().fill(ChipFill.trackColor.opacity(ChipFill.rest)))
        .fixedSize(horizontal: metrics.wide, vertical: false)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Kind")
    }
}

private struct SegmentStyle: ButtonStyle {
    let on: Bool
    let wide: Bool

    func makeBody(configuration: Configuration) -> some View {
        Segment(configuration: configuration, on: on, wide: wide)
    }

    private struct Segment: View {
        let configuration: ButtonStyleConfiguration
        let on: Bool
        let wide: Bool
        @KeyboardFocused private var focused

        var body: some View {
            configuration.label
                .textFont(wide ? 13 : 15, weight: .semibold, relativeTo: wide ? .footnote : .subheadline)
                .foregroundStyle(on ? Color.white : Theme.Palette.muted)
                .lineLimit(1)
                .padding(.horizontal, 14)
                .frame(maxWidth: wide ? nil : .infinity, minHeight: (wide ? Theme.Pill.desktop : Theme.Pill.phone) - 6)
                .background { if on { Capsule().fill(Theme.Palette.selectedFill) } }
                .contentShape(Capsule())
                .pressVeil(Capsule(), pressed: configuration.isPressed)
                .modifier(FocusRing(shape: Capsule(), active: focused))
        }
    }
}

/// The iPhone's pull-down (phone.ts `.pull`): plain 17/600 words and the up-down chevrons, no fill.
struct PhonePull<Items: View>: View {
    let title: String
    @ViewBuilder let items: () -> Items

    var body: some View {
        Menu(content: items) {
            HStack(spacing: 5) {
                Text(title).lineLimit(1)
                Glyph("chevrons-up-down", size: 15)
            }
            .textFont(17, weight: .semibold, relativeTo: .body)
            .foregroundStyle(Theme.Palette.text)
            .frame(minHeight: Theme.Pill.phone)
            .contentShape(Rectangle())
        }
        .menuStyle(.button)
        .buttonStyle(.plain)
        .menuIndicator(.hidden)
        .fixedSize(horizontal: true, vertical: false)
    }
}

/// Discover's grid on the Mac and iPad: 139 by 208 covers with no caption (the cover's band carries the
/// scores). Beside the preview a first activation selects and the selected poster opens; without it, a
/// click opens. Focus (the keyboard's and VoiceOver's) moves the selection.
struct DiscoverGrid: View {
    let tiles: [Tile]
    let columns: Int
    /// Select then open, beside the preview.
    let selects: Bool
    @Binding var selection: DiscoverSelection
    var onEnd: (() -> Void)?
    /// The sort value on a poster's corner.
    var corner: (Tile) -> String? = { _ in nil }
    @Environment(\.openTitle) private var openTitle
    @FocusState private var focused: String?
    @AccessibilityFocusState private var voiceOver: String?
    #if os(macOS)
    /// The grid holds the keys (arrows, Return), with or without Full Keyboard Access.
    @FocusState private var gridFocused: Bool
    #endif

    var body: some View {
        let keyed = tiles.keyed
        let poster = DiscoverLayout.poster
        LazyVGrid(
            columns: Array(repeating: GridItem(.fixed(poster.width), spacing: DiscoverLayout.gap, alignment: .top), count: columns),
            alignment: .leading, spacing: Theme.TileDesktop.Grid.rowGap
        ) {
            ForEach(keyed, id: \.key) { key, tile in
                let on = selects && selection.key == key
                Button { activate(key, tile) } label: {
                    ArtImage(url: tile.poster, fallbackURL: tile.posterFallback, fallback: tile.name)
                        .frame(width: poster.width, height: poster.height)
                        .overlay { if let value = corner(tile) { CornerLabel(text: value, banded: tile.posterFallback != nil) } }
                        .cardArt(radius: Theme.TileDesktop.radius)
                }
                .buttonStyle(CardButtonStyle(lifted: on))
                .focused($focused, equals: key)
                .accessibilityFocused($voiceOver, equals: key)
                .accessibilityLabel(posterLabel([tile.name, tile.releaseInfo], tile.ratings))
                .accessibilityAddTraits(on ? .isSelected : [])
                .accessibilityAction(named: "Select") { selection.select(key) }
                .onAppear { if key == keyed.last?.key { onEnd?() } }
            }
        }
        // No minimum width of its own: the fixed columns follow the window, so they must never hold it open (a
        // window narrowed under the last layout kept that layout, and the Mac's content min size with it).
        .frame(minWidth: 0, idealWidth: 0, maxWidth: .infinity, alignment: .topLeading)
        .onChange(of: focused) { _, key in if selects, let key { selection.select(key) } }
        .onChange(of: voiceOver) { _, key in if selects, let key { selection.select(key) } }
        #if os(macOS)
        // The grid itself takes the keys, so the arrows and Return work without Full Keyboard Access (a
        // Button takes no keyboard focus then); a click in it gives it the keys, as the timing stepper does.
        .focusable(interactions: .edit)
        .focusEffectDisabled()
        .focused($gridFocused)
        .onKeyPress(.leftArrow) { move(.left, keys: keyed.map(\.key)) }
        .onKeyPress(.rightArrow) { move(.right, keys: keyed.map(\.key)) }
        .onKeyPress(.upArrow) { move(.up, keys: keyed.map(\.key)) }
        .onKeyPress(.downArrow) { move(.down, keys: keyed.map(\.key)) }
        #endif
        .onKeyPress(.return) {
            guard selects, let key = selection.key, let tile = keyed.first(where: { $0.key == key })?.tile else { return .ignored }
            openTitle(TitleRoute(tile))
            return .handled
        }
    }

    /// Every click, Return and Space goes through the selection: only the selected poster opens.
    private func activate(_ key: String, _ tile: Tile) {
        #if os(macOS)
        gridFocused = true
        #endif
        if !selects || selection.activate(key) { openTitle(TitleRoute(tile)) }
    }

    #if os(macOS)
    private func move(_ step: DiscoverSelection.Move, keys: [String]) -> KeyPress.Result {
        guard selects, let index = selection.key.flatMap(keys.firstIndex(of:)) else { return .ignored }
        let to = keys[DiscoverSelection.moved(from: index, step, columns: columns, count: keys.count)]
        selection.select(to)
        if focused != nil { focused = to }
        return .handled
    }
    #endif
}

/// The selected poster at the right of the Mac's and iPad's grid: a solid `surface` panel (DECISION
/// Grids), from the catalog item and the details the core sends for it (`preview`, C44a), never a title
/// view, so no torrent engine and no versions view.
struct DiscoverPreview: View {
    let tile: Tile
    /// The snapshot's `preview`: shown only when it is this tile's.
    let details: ItemDetails?
    let open: () -> Void
    /// Adds the item to the Library (true) or takes it out (`setInLibrary`, C44b).
    let setLibrary: (Bool) -> Void

    var body: some View {
        let words = DiscoverWords.preview(tile, details)
        VStack(alignment: .leading, spacing: 14) {
            // The backdrop, else the plain poster from its top.
            ArtImage(url: tile.background ?? tile.plainPoster, maxPixels: ArtSize.backdrop, alignment: tile.background == nil ? .top : .center)
                .frame(height: 207)
                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                .accessibilityHidden(true)
            TitleLogo(logo: tile.logo, name: tile.name, maxWidth: 300, height: 54, nameSize: 28)
            MetaLine(ratings: ratingParts(tile.ratings), parts: words.parts, size: 13)
            if let synopsis = words.synopsis {
                Text(synopsis)
                    .textFont(15, relativeTo: .body)
                    .foregroundStyle(Theme.Palette.muted)
                    .lineLimit(5)
                    .fixedSize(horizontal: false, vertical: true)
            }
            HStack(spacing: 12) {
                Button("Show", action: open).buttonStyle(PillButtonStyle(prominent: true))
                // Hidden until the details say the Library can be set for this item (never on a kids profile).
                if let inLibrary = DiscoverWords.library(tile, details) {
                    Button { setLibrary(!inLibrary) } label: {
                        Label(inLibrary ? "In Library" : "Add to Library", glyph: inLibrary ? "bookmark-check" : "bookmark-plus")
                    }
                    .buttonStyle(PillButtonStyle())
                    // A state, spoken as one: "In Library, toggle, on".
                    .accessibilityLabel("In Library")
                    .accessibilityAddTraits(.isToggle)
                    .accessibilityValue(inLibrary ? "On" : "Off")
                }
            }
        }
        .padding(16)
        .frame(width: Theme.Preview.width, alignment: .topLeading)
        .background(Theme.Palette.surface, in: RoundedRectangle(cornerRadius: Theme.Preview.radius, style: .continuous))
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Preview")
    }
}

/// The iPhone's grid: two columns sharing the width at 2:3, the name (two lines) and the year under each,
/// no ratings line (the cover carries the scores). A tap opens the title.
struct PhoneDiscoverGrid: View {
    let tiles: [Tile]
    var onEnd: (() -> Void)?
    /// The sort value on a poster's corner.
    var corner: (Tile) -> String? = { _ in nil }
    @State private var width: CGFloat = 0

    var body: some View {
        VStack {
            if width > 0 { grid }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
    }

    private var grid: some View {
        let size = DiscoverLayout.phonePoster(width: width)
        let keyed = tiles.keyed
        return LazyVGrid(
            columns: Array(repeating: GridItem(.fixed(size.width), spacing: Theme.Tile.Grid.gap, alignment: .top), count: Theme.Tile.Grid.columns),
            alignment: .leading, spacing: Theme.Tile.Grid.rowGap
        ) {
            ForEach(keyed, id: \.key) { key, tile in
                NavigationLink(value: TitleRoute(tile)) {
                    VStack(alignment: .leading, spacing: 6) {
                        ArtImage(url: tile.poster, fallbackURL: tile.posterFallback, fallback: tile.name)
                            .frame(width: size.width, height: size.height)
                            .overlay { if let value = corner(tile) { CornerLabel(text: value, banded: tile.posterFallback != nil) } }
                            .cardArt(radius: Theme.Tile.radius)
                        Text(tile.name)
                            .textFont(15, weight: .semibold, relativeTo: .subheadline)
                            .foregroundStyle(Theme.Palette.text)
                            .lineLimit(2)
                            .multilineTextAlignment(.leading)
                        // The corner shows the year for the date sorts: the line under the name does not repeat it.
                        if corner(tile) != tile.releaseInfo, let year = tile.releaseInfo, !year.isEmpty {
                            Text(year)
                                .textFont(13, relativeTo: .footnote)
                                .foregroundStyle(Theme.Palette.dim)
                        }
                    }
                    .frame(width: size.width, alignment: .leading)
                }
                .buttonStyle(CardButtonStyle())
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(posterLabel([tile.name, tile.releaseInfo], tile.ratings))
                .accessibilityAddTraits(.isButton)
                .onAppear { if key == keyed.last?.key { onEnd?() } }
            }
        }
    }
}

/// A first page loading: one row of blank covers in the grid's places, no words.
struct DiscoverSkeleton: View {
    let count: Int
    let size: CGSize
    let gap: CGFloat
    let radius: CGFloat

    var body: some View {
        HStack(spacing: gap) {
            ForEach(0..<count, id: \.self) { _ in
                RoundedRectangle(cornerRadius: radius, style: .continuous)
                    .fill(Theme.Palette.surface)
                    .frame(width: size.width, height: size.height)
            }
        }
        .frame(minWidth: 0, idealWidth: 0, maxWidth: .infinity, alignment: .topLeading)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Loading")
    }
}
#endif
