import SwiftUI

/// The services whose marks the title page's ratings row shows (Marks.xcassets, from design/marks).
enum ScoreMark: String, Hashable, Sendable {
    case imdb, tmdb, trakt

    var asset: String { "mark-\(rawValue)" }
}

/// One item of the title page's ratings row: the service's mark and its number, nil while pending, and the title's
/// page there when the core gave a checked one (linked off TV only).
struct ScoreItem: Hashable, Sendable {
    let mark: ScoreMark
    let text: String?
    let spoken: String
    var url: URL? = nil
}

extension ScoreMark {
    var name: String {
        switch self { case .imdb: "IMDb"; case .tmdb: "TMDB"; case .trakt: "Trakt" }
    }
}

/// A badge's link: only an https page on IMDb, TMDB or Trakt; anything else is no link.
func outLink(_ url: String?) -> URL? {
    let hosts: Set<String> = ["imdb.com", "www.imdb.com", "themoviedb.org", "www.themoviedb.org", "trakt.tv"]
    guard let url, let parsed = URL(string: url), parsed.scheme == "https", let host = parsed.host(), hosts.contains(host) else {
        return nil
    }
    return parsed
}

/// The title page's ratings row (plans/title-page-ratings-genres-similar.md): IMDb as x/10 (the addon's score),
/// TMDB and Trakt as percentages, in that order. A pending slot keeps its place; a none slot and a missing IMDb
/// score have none.
func scoreItems(imdb: String?, scores: TitleScores?, imdbUrl: String? = nil) -> [ScoreItem] {
    var items: [ScoreItem] = []
    if let imdb { items.append(ScoreItem(mark: .imdb, text: "\(imdb)/10", spoken: "IMDb \(imdb) out of 10", url: outLink(imdbUrl))) }
    func slot(_ mark: ScoreMark, _ name: String, _ slot: Slot?) {
        switch slot {
        case .shown(let text, let url):
            items.append(ScoreItem(mark: mark, text: text, spoken: "\(name) \(text.replacingOccurrences(of: "%", with: " percent"))", url: outLink(url)))
        case .pending: items.append(ScoreItem(mark: mark, text: nil, spoken: ""))
        default: break
        }
    }
    slot(.tmdb, "TMDB", scores?.tmdb)
    slot(.trakt, "Trakt", scores?.trakt)
    return items
}

/// Whether the title page shows, which the core hears as `titleVisible` (TMDB and Trakt are asked only then): in its
/// stack's front, placed in the window, the app active, and nothing over it (the player, Versions).
func titlePageShows(inFront: Bool, placed: Bool, active: Bool, covered: Bool) -> Bool {
    inFront && placed && active && !covered
}

/// The ratings row's sizes: the badge is legible, never cap height (TV 28 in a 36 row; else 18 in 24).
enum ScoreRow {
    #if os(tvOS)
    static let height: CGFloat = 36
    static let mark: CGFloat = 28
    #else
    static let height: CGFloat = 24
    static let mark: CGFloat = 18
    #endif
}

/// One score: the service's mark in its own colours, then the number. A pending one keeps its width and
/// shows nothing; the meta line speaks the names (`MetaLine`).
struct ScoreMarkView: View {
    let item: ScoreItem

    var body: some View {
        HStack(spacing: ScoreRow.mark * 0.45) {
            Image(item.mark.asset)
                .resizable()
                .aspectRatio(contentMode: .fit)
                .frame(height: ScoreRow.mark)
            Text(item.text ?? "00%")
                .fontWeight(.semibold)
                .foregroundStyle(.white)
                .lineLimit(1)
                .fixedSize()
        }
        .frame(height: ScoreRow.height)
        .opacity(item.text == nil ? 0 : 1)
        .accessibilityHidden(true)
    }
}

/// The title page's rated cover: RPDB's poster with the IMDb, Metacritic and Rotten Tomatoes band drawn in, the
/// only place Rotten Tomatoes shows on the page. A fixed 2:3 poster, decoration only (never a focus stop), one
/// spoken name. It holds its place while it loads and hides itself when the image fails, never an empty box.
struct RatedCover: View {
    let url: String
    let width: CGFloat
    @State private var art: DecodedArt?
    @State private var failed = false

    var body: some View {
        if !failed {
            Theme.Palette.surface
                .overlay {
                    if let art {
                        Image(decorative: art.image, scale: 1)
                            .resizable()
                            .aspectRatio(contentMode: .fill)
                            .transition(.opacity)
                    }
                }
                .frame(width: width, height: width * 1.5)
                .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous))
                .accessibilityElement()
                .accessibilityLabel("Cover with ratings")
                .accessibilityAddTraits(.isImage)
                .task(id: url) {
                    failed = false
                    guard let link = URL(string: url), let loaded = await ArtLoader.shared.image(link, maxPixels: ArtSize.poster) else {
                        art = nil
                        failed = true
                        return
                    }
                    withAnimation(.easeOut(duration: 0.2)) { art = loaded }
                }
        }
    }
}
