import SwiftUI

/// The title's Library and Watched marks as one capsule (Mac, iPad; the desktop frame's `.pair2`):
/// one 40 pt pill on the secondary fill, a cell per mark, each a toggle with its state in its label.
/// A series has the Library cell alone (`titleMarks`).
struct MarkCapsule: View {
    let marks: [TitleMark]
    let toggle: (TitleMark) -> Void

    var body: some View {
        HStack(spacing: 0) {
            ForEach(Array(marks.enumerated()), id: \.offset) { index, mark in
                if index > 0 { Rectangle().fill(.white.opacity(0.1)).frame(width: 1) }
                Button { toggle(mark) } label: {
                    Label(mark.words, glyph: mark.glyph, size: HeroLayout.icon)
                        .labelStyle(PillLabelStyle())
                        .textFont(15, weight: .semibold, relativeTo: .body)
                        .foregroundStyle(.white)
                        .padding(.horizontal, 18)
                        .frame(height: 40)
                }
                .buttonStyle(FocusPlainStyle(shape: Capsule(), scale: 1))
                // A state, spoken as one: "In Library, toggle, on".
                .accessibilityLabel(mark.name)
                .accessibilityAddTraits(.isToggle)
                .accessibilityValue(mark.on ? "On" : "Off")
            }
        }
        .fixedSize()
        .background(SecondaryFill(shape: Capsule()))
    }
}

/// Apple's Library control on the iPhone: a 48 pt round button whose plus turns into a check once the
/// title is in the library; its label says the state and the action.
struct LibraryControl: View {
    let on: Bool
    let toggle: () -> Void

    var body: some View {
        Button(action: toggle) {
            Glyph(on ? "check" : "plus", size: 22)
                .dynamicTypeSize(...DynamicTypeSize.xxxLarge)
                .foregroundStyle(.white)
                .frame(width: 48, height: 48)
                .background(SecondaryFill(shape: Circle()))
        }
        .buttonStyle(FocusPlainStyle(shape: Circle(), scale: Theme.Focus.scaleCard))
        .accessibilityLabel(on ? "In Library, remove" : "Add to Library")
    }
}

private extension TitleMark {
    var on: Bool {
        switch self { case .library(let on), .watched(let on): on }
    }
    var name: String {
        switch self { case .library: "In Library"; case .watched: "Watched" }
    }
    var words: String {
        switch self {
        case .library(let on): on ? "In Library" : "Add to Library"
        case .watched(let on): on ? "Watched" : "Mark as watched"
        }
    }
    var glyph: String {
        switch self {
        case .library(let on): on ? "check" : "plus"
        case .watched: "eye"
        }
    }
}

/// What the title hero shows, as plain values, so a test lays the hero out without the core.
struct TitleHeroFacts {
    let name: String
    let logo: String?
    /// "Series · Drama · Thriller": the kind, then two genres (the iPhone's line).
    let kindLine: String
    let genres: [String]
    let directors: [String]
    /// The years, then the runtime.
    let meta: [String]
    let ratings: [String]
    let imdbLink: URL?
    let synopsis: String?
    let cast: [String]

    init(_ details: TitleDetails) {
        name = details.name
        logo = details.logo
        let kind = details.kind == "movie" ? "Movie" : details.kind == "series" ? "Series" : details.kind.capitalized
        kindLine = ([kind] + details.genres.prefix(2)).joined(separator: " \u{B7} ")
        genres = Array(details.genres.prefix(3))
        directors = Array(details.directors.prefix(3))
        meta = [details.releaseInfo, details.runtime].compactMap { $0 }
        ratings = ratingParts(details.ratings)
        imdbLink = details.ratings?.imdb == nil ? nil : details.imdbUrl.flatMap(URL.init(string:))
        synopsis = details.description
        cast = details.cast
    }
}

/// Where a linked chip goes: a genre to Discover, a director to Search.
enum TitleLink: Equatable {
    case genre(String)
    case director(String)
}

/// A part of the hero a test measures (TitleHeroTests); nothing reads it in the app.
enum HeroPart: Hashable { case meta, chips, synopsis, actions, kindLine, box }

/// Where a test hears a hero part's frame; a class, so the environment compares it by identity.
final class HeroProbeSink {
    let report: (HeroPart, CGRect) -> Void
    init(_ report: @escaping (HeroPart, CGRect) -> Void) { self.report = report }
}

extension EnvironmentValues {
    @Entry var heroProbe: HeroProbeSink? = nil
}

extension View {
    func heroProbe(_ part: HeroPart) -> some View { modifier(HeroProbe(part: part)) }
}

private struct HeroProbe: ViewModifier {
    let part: HeroPart
    @Environment(\.heroProbe) private var probe

    func body(content: Content) -> some View {
        if let probe {
            content.onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { probe.report(part, $0) }
        } else {
            content
        }
    }
}

/// The Mac's (and iPad's) title column in tv.apple.com's order: the logo, the meta row with the
/// ratings, the linked chips, three lines of synopsis with MORE, then the action row. The meta row,
/// the chips and the synopsis stay within \`cap\`, where the title wash holds them (the glass check).
struct TitleHeroWide<Actions: View>: View {
    let facts: TitleHeroFacts
    var cap: CGFloat = HeroLayout.synopsisWidth
    /// The column's width: 900, or narrower beside the open panel (`WideLayout.panelColumnWidth`).
    var columnWidth: CGFloat = 900
    let open: (TitleLink) -> Void
    let more: () -> Void
    @ViewBuilder let actions: Actions
    @Environment(\.metrics) private var metrics

    var body: some View {
        VStack(alignment: .leading, spacing: HeroLayout.gap) {
            TitleLogo(logo: facts.logo, name: facts.name, maxWidth: HeroLayout.logoWidth, height: HeroLayout.logoHeight, nameSize: Theme.type.hero + 12)
            MetaLine(ratings: facts.ratings, parts: facts.meta, size: HeroLayout.meta, imdbLink: facts.imdbLink)
                .foregroundStyle(Theme.Palette.dim)
                .frame(maxWidth: min(cap, columnWidth), alignment: .leading)
                .padding(.top, 7)
                .heroProbe(.meta)
            if !facts.genres.isEmpty || !facts.directors.isEmpty {
                ChipGroups(genres: facts.genres, directors: facts.directors, open: open)
                    .frame(maxWidth: min(cap, columnWidth), alignment: .leading)
                    .padding(.top, 5)
                    .heroProbe(.chips)
            }
            if let synopsis = facts.synopsis {
                HeroSynopsis(text: synopsis, lines: 3, more: more)
                    .frame(maxWidth: min(cap, columnWidth), alignment: .leading)
                    .padding(.top, 3)
                    .heroProbe(.synopsis)
            }
            actions
                .padding(.top, 7)
                .heroProbe(.actions)
        }
        .frame(maxWidth: columnWidth, alignment: .leading)
        .padding(.horizontal, metrics.gutter)
        .padding(.bottom, HeroLayout.foot)
    }
}

/// The iPhone's title hero, centered over the art: the logo, the kind and two genres, the primary
/// with its Library control, then the secondaries. The synopsis and the meta row follow under the
/// hero, on the page, so the words over art stay where the foot holds them (the glass check).
struct TitleHeroPhone<Buttons: View, Secondaries: View>: View {
    let facts: TitleHeroFacts
    @ViewBuilder let buttons: Buttons
    @ViewBuilder let secondaries: Secondaries

    var body: some View {
        VStack(spacing: 0) {
            TitleLogo(logo: facts.logo, name: facts.name, maxWidth: 300, height: 86, nameSize: Theme.type.title1, alignment: .bottom)
            Text(facts.kindLine)
                .textFont(15, weight: .semibold, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.text)
                .multilineTextAlignment(.center)
                .padding(.top, 12)
                .heroProbe(.kindLine)
            buttons.padding(.top, 14)
            secondaries.padding(.top, 8)
        }
        .padding(.horizontal, 20)
        .padding(.bottom, 12)
    }
}

/// Genres, then Directors: a label and its chips, each chip a link (\`TitleLink\`). Over art the chips
/// are secondaries: a pure white label on the 16 percent fill (design/DECISION.md).
struct ChipGroups: View {
    let genres: [String]
    let directors: [String]
    let open: (TitleLink) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            group("Genres", genres, TitleLink.genre, hint: "Opens Discover")
            group("Directors", directors, TitleLink.director, hint: "Searches for their titles")
        }
    }

    /// One line a group, as the glass check's spots read them: the most names that fit (review P2-2).
    @ViewBuilder private func group(_ label: String, _ names: [String], _ link: @escaping (String) -> TitleLink, hint: String) -> some View {
        if !names.isEmpty {
            ViewThatFits(in: .horizontal) {
                ForEach((1...names.count).reversed(), id: \.self) { count in
                    line(label, Array(names.prefix(count)), link, hint: hint)
                }
            }
        }
    }

    private func line(_ label: String, _ names: [String], _ link: @escaping (String) -> TitleLink, hint: String) -> some View {
        HStack(spacing: 8) {
            Text(label)
                .textFont(13, weight: .semibold, relativeTo: .footnote)
                .foregroundStyle(Theme.Palette.text)
                .frame(height: 28)
                .padding(.trailing, 4)
                .accessibilityAddTraits(.isHeader)
            ForEach(names, id: \.self) { name in
                Button { open(link(name)) } label: {
                    Text(name)
                        .textFont(13, weight: .medium, relativeTo: .footnote)
                        .foregroundStyle(.white)
                        .lineLimit(1)
                        .padding(.horizontal, 12)
                        .frame(height: 28)
                        .background(SecondaryFill(shape: Capsule()))
                }
                .buttonStyle(FocusPlainStyle(shape: Capsule(), scale: 1))
                .accessibilityHint(hint)
            }
        }
    }
}

/// The synopsis held to \`lines\`, with Apple's MORE pill when something is cut. MORE opens the whole
/// text in a sheet, never in place: opened text would run past the ramps (the glass check).
struct HeroSynopsis: View {
    let text: String
    let lines: Int
    let more: () -> Void
    @State private var shown: CGFloat = 0
    @State private var full: CGFloat = 0

    var body: some View {
        HStack(alignment: .lastTextBaseline, spacing: 8) {
            Text(text)
                .textFont(15, relativeTo: .body)
                .foregroundStyle(Theme.Palette.muted)
                .lineLimit(lines)
                .fixedSize(horizontal: false, vertical: true)
                .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { shown = $0 }
                .background(alignment: .topLeading) {
                    Text(text)
                        .textFont(15, relativeTo: .body)
                        .fixedSize(horizontal: false, vertical: true)
                        .hidden()
                        .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { full = $0 }
                }
            if full > shown + 1 {
                Button(action: more) {
                    Text("MORE")
                        .textFont(11, weight: .semibold, relativeTo: .caption2)
                        .dynamicTypeSize(...DynamicTypeSize.xxxLarge)
                        .foregroundStyle(.black.opacity(0.88))
                        .padding(.horizontal, 6)
                        .background(.white.opacity(0.64), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                        // A 44 pt target on the phone around the small pill.
                        .padding(.vertical, 12)
                        .contentShape(Rectangle())
                        .padding(.vertical, -12)
                }
                .buttonStyle(FocusPlainStyle(shape: RoundedRectangle(cornerRadius: 8, style: .continuous), scale: 1))
                .accessibilityLabel("More")
                .accessibilityHint("Shows the whole synopsis")
            }
        }
    }
}

/// The whole synopsis, read in a sheet (iOS and macOS; tvOS has its own reader).
struct SynopsisSheet: View {
    let title: String
    let text: String
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ScrollView(.vertical) {
                Text(text)
                    .textFont(Theme.type.body)
                    .foregroundStyle(Theme.Palette.text)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: 640, alignment: .leading)
                    .padding(Theme.Space.l)
            }
            .background(Theme.Palette.surface)
            .navigationTitle(title)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }
        #if os(macOS)
        .frame(minWidth: 480, minHeight: 360)
        #endif
    }
}

/// "Starring" and the cast at the hero's right (Mac), shown where \`WideLayout.starringFits\` says it
/// clears the action row.
struct HeroStarring: View {
    let cast: [String]

    var body: some View {
        (Text("Starring ").foregroundStyle(Theme.Palette.dim) + Text(cast.prefix(4).joined(separator: ", ")).foregroundStyle(Theme.Palette.text))
            .textFont(13, relativeTo: .footnote)
            .multilineTextAlignment(.trailing)
            .frame(width: 360, alignment: .trailing)
            .fixedSize(horizontal: false, vertical: true)
    }
}
