import SwiftUI

#if !os(tvOS)
/// The page's search field (the narrow Mac, the iPad, the iPhone): 42 tall at least, the track grey,
/// no edge (plans/design-v3-phase7-apple.md 2). The keyboard ring shows for keyboard focus.
struct SearchField: View {
    @Bindable var input: SearchInput
    let prompt: String
    var focused: FocusState<Bool>.Binding

    var body: some View {
        HStack(spacing: 8) {
            Glyph("search", size: 18).foregroundStyle(Theme.Palette.muted)
            TextField(prompt, text: $input.text)
                .textFieldStyle(.plain)
                .textFont(17, relativeTo: .body)
                .foregroundStyle(Theme.Palette.text)
                .focused(focused)
                .autocorrectionDisabled()
                #if os(iOS)
                .textInputAutocapitalization(.never)
                .submitLabel(.search)
                #endif
                // The core searches as you type: Return only moves to the first result.
                .onSubmit { input.submit += 1 }
                .onKeyPress(.escape) {
                    switch SearchRules.escape(input.text) {
                    case .clear: input.text = ""
                    case .leave: focused.wrappedValue = false
                    }
                    return .handled
                }
            if !input.text.isEmpty {
                Button { input.text = "" } label: {
                    Glyph("circle-x", size: 17).foregroundStyle(Theme.Palette.muted).frame(width: 44, height: 44)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Clear search")
            }
        }
        .padding(.leading, 14)
        .padding(.trailing, input.text.isEmpty ? 14 : 0)
        .frame(minHeight: 42)
        .background(Capsule().fill(ChipFill.trackColor.opacity(ChipFill.rest)))
        .modifier(FocusRing(shape: Capsule(), active: focused.wrappedValue))
        .accessibilityElement(children: .contain)
    }
}

/// Recent searches (the core's `recent`, up to eight): the clock and the query; a row fills the field.
struct RecentList: View {
    let recent: [String]
    let input: SearchInput
    @Environment(\.metrics) private var metrics

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(SearchWords.recent)
                .display(metrics.wide ? 17 : 20)
                .foregroundStyle(Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            ForEach(recent.prefix(8), id: \.self) { query in
                Button { input.text = query } label: {
                    HStack(spacing: 10) {
                        Glyph("clock", size: metrics.wide ? 15 : 17).foregroundStyle(Theme.Palette.dim)
                        Text(query).foregroundStyle(Theme.Palette.text).lineLimit(1)
                        Spacer(minLength: 0)
                    }
                    .textFont(metrics.wide ? 15 : 17, relativeTo: .body)
                    .frame(minHeight: 44)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
    }
}

/// One of Top results on the Mac and iPad (tv.apple.com's 440 by 100 card): the plain poster, the name,
/// the kind and the year, the score as text.
struct TopResultCard: View {
    let tile: Tile

    var body: some View {
        HStack(alignment: .center, spacing: 14) {
            ResultThumb(tile: tile, radius: 8)
            VStack(alignment: .leading, spacing: 3) {
                Text(tile.name)
                    .textFont(15, relativeTo: .subheadline)
                    .foregroundStyle(Theme.Palette.text)
                    .lineLimit(2)
                    .multilineTextAlignment(.leading)
                ResultMeta(tile: tile, size: 13)
            }
            Spacer(minLength: 0)
        }
        .padding(EdgeInsets(top: 14, leading: 14, bottom: 14, trailing: 16))
        .frame(maxWidth: .infinity, minHeight: Theme.Search.topCard.height, alignment: .leading)
        .background(.white.opacity(0.1), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(posterLabel([tile.name, tile.releaseInfo], tile.ratings))
    }
}

/// A result on the iPhone (the iOS TV app's 92 pt row): the plain poster 48 wide, the name, the kind and
/// the year with the score, a hairline from the text's edge.
struct SearchResultRow: View {
    let tile: Tile

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            ResultThumb(tile: tile, radius: 7)
            VStack(alignment: .leading, spacing: 3) {
                Text(tile.name)
                    .textFont(15, relativeTo: .subheadline)
                    .foregroundStyle(Theme.Palette.text)
                    .lineLimit(2)
                    .multilineTextAlignment(.leading)
                ResultMeta(tile: tile, size: 12)
            }
            Spacer(minLength: 0)
        }
        .frame(minHeight: 92)
        .overlay(alignment: .bottom) {
            Rectangle().fill(Theme.Palette.line).frame(height: 0.5).padding(.leading, Theme.Search.thumb.width + 12)
        }
        .contentShape(Rectangle())
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(posterLabel([tile.name, tile.releaseInfo], tile.ratings))
    }
}

/// A result's plain poster (never the cover's ratings band), 48 by 72, the poster keyline.
private struct ResultThumb: View {
    let tile: Tile
    let radius: CGFloat

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
        ArtImage(url: tile.plainPoster, fallback: nil)
            .frame(width: Theme.Search.thumb.width, height: Theme.Search.thumb.height)
            .clipShape(shape)
            .overlay { shape.strokeBorder(Theme.Palette.lineStrong, lineWidth: 0.5) }
    }
}

/// "Movie · 2010" then "IMDb 8.8", the score as the ratings row writes it.
private struct ResultMeta: View {
    let tile: Tile
    let size: CGFloat

    var body: some View {
        let meta = SearchWords.meta(tile)
        let score = ratingParts(tile.ratings).first
        HStack(spacing: 6) {
            if !meta.isEmpty { Text(meta).foregroundStyle(Theme.Palette.dim) }
            if let score { ratingText(score) }
        }
        .textFont(size, relativeTo: .footnote)
        .lineLimit(1)
    }
}

/// Results still coming: blank cards or rows in their places, no words.
struct ResultSkeleton: View {
    let count: Int
    let wide: Bool
    var columns = 1

    var body: some View {
        LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 20), count: columns), alignment: .leading, spacing: wide ? 20 : 0) {
            ForEach(0..<count, id: \.self) { _ in
                RoundedRectangle(cornerRadius: wide ? 14 : 7, style: .continuous)
                    .fill(Theme.Palette.surface)
                    .frame(height: wide ? Theme.Search.topCard.height : 72)
                    .frame(minHeight: wide ? 0 : 92)
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Loading")
    }
}
#endif
