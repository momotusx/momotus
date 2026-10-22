import SwiftUI

/// "2011–2019 · Drama, Fantasy · 8 seasons · IMDb 9.2 · RT 89%": the year
/// first, the parts joined by dots, then the ratings row. Lines break only
/// between the parts. `imdbLink` makes IMDb's score open its page (never on TV).
struct MetaLine: View {
    let ratings: [String]
    let parts: [String]
    let size: CGFloat
    var imdbLink: URL? = nil
    @Environment(\.openURL) private var openURL

    var body: some View {
        FlowRow(spacing: size * 0.45) {
            ForEach(Array((parts + ratings).enumerated()), id: \.offset) { index, part in
                if index > 0 {
                    Text("\u{B7}").foregroundStyle(Theme.Palette.dim)
                }
                if index >= parts.count {
                    score(part, link: index == parts.count ? link : nil)
                } else {
                    Text(part)
                }
            }
        }
        .textFont(size, weight: .semibold, relativeTo: .callout)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel((parts + ratings.map(spokenScore)).joined(separator: ", "))
        .accessibilityActions {
            if let link { Button("Open on IMDb") { openURL(link) } }
        }
    }

    private var link: URL? {
        #if os(tvOS)
        nil
        #else
        imdbLink
        #endif
    }

    @ViewBuilder private func score(_ text: String, link: URL?) -> some View {
        // One line, never cut: the row breaks between parts, never inside a score. The ratings row
        // as plain text: the source at 70 percent, the value in white at 600 (D10).
        let label = ratingText(text).fontWeight(.regular)
            .lineLimit(1).fixedSize()
        if let link {
            Link(destination: link) { label }
                .buttonStyle(.plain)
                #if os(iOS)
                // A finger-sized target that does not grow the line (an 18 pt meta row).
                .padding(.vertical, 13)
                .contentShape(Rectangle())
                .padding(.vertical, -13)
                #endif
        } else {
            label
        }
    }
}

/// One score as the ratings row writes it (D10): the source's name at 70 percent and its value in
/// white at 600, plain text with no star and no logo ("IMDb 8.4", "RT 98%", "RT Fresh").
func ratingText(_ score: String) -> Text {
    let parts = score.split(separator: " ", maxSplits: 1).map(String.init)
    guard parts.count == 2 else { return Text(score).fontWeight(.semibold).foregroundStyle(.white) }
    return Text(parts[0]).foregroundStyle(Theme.Palette.muted) + Text(" ") + Text(parts[1]).fontWeight(.semibold).foregroundStyle(.white)
}

/// The title's logo art, or its name set in the display face when there is none.
struct TitleLogo: View {
    let logo: String?
    let name: String
    let maxWidth: CGFloat
    let height: CGFloat
    /// Size of the name when there is no logo.
    var nameSize = Theme.type.hero
    /// Where the art sits in its box: leading on wide screens, centered on the phone.
    var alignment: Alignment = .bottomLeading

    /// nil while loading; then the art, or none when there is no logo.
    @State private var loaded: DecodedArt??

    var body: some View {
        ZStack(alignment: alignment) {
            switch loaded {
            case .some(.some(let art)):
                Image(decorative: art.image, scale: 1)
                    .resizable()
                    .scaledToFit()
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: alignment)
                    .transition(.opacity)
            case .some(.none):
                Text(name)
                    .display(nameSize)
                    .foregroundStyle(Theme.Palette.text)
                    .lineLimit(2)
                    .minimumScaleFactor(0.6)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: alignment)
            case .none:
                Color.clear
            }
        }
        .task(id: logo) {
            if logo != nil { loaded = nil }
            let start = ContinuousClock.now
            let art = await ArtLoader.shared.image(logo, maxPixels: ArtSize.logo)
            withAnimation(reveal(since: start)) { loaded = .some(art) }
        }
        .frame(maxWidth: maxWidth, maxHeight: height, alignment: alignment)
        .frame(height: height, alignment: alignment)
        .accessibilityElement()
        .accessibilityLabel(name)
        .accessibilityAddTraits(.isHeader)
    }
}
