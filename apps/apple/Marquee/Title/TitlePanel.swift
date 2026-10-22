import SwiftUI

// iOS and macOS only: tvOS keeps its own title page (D9).
#if !os(tvOS)

/// Stremio's panel in its episodes state (D1; plans/design-v3-phase3-apple.md 2.3): Prev, the season
/// menu, Next, an episode search, then the dated rows with their actions. On the Mac and iPad it is an
/// overlay beside the info (`TitleScreen`); on the iPhone it is the Episodes page. Phase 6 adds the
/// streams state.
struct EpisodePanel: View {
    let seasons: [Int]
    let selected: Int?
    let episodes: [Episode]
    let minutes: Int?
    let select: (Int) -> Void
    let actions: EpisodeActionsSet
    /// The episode a Calendar link named: the rows scroll to it once loaded.
    var scrollTo: String?
    /// Mac and iPad: the panel's close button (Escape too).
    var close: (() -> Void)?
    @Environment(\.metrics) private var metrics
    @State private var query = ""
    @FocusState private var seasonFocused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                step(-1)
                Spacer(minLength: 0)
                seasonMenu
                Spacer(minLength: 0)
                step(1)
                if let close {
                    Button(action: close) {
                        Glyph("x", size: 14)
                            .foregroundStyle(Theme.Palette.text)
                            .frame(width: 32, height: 32)
                            .background(.white.opacity(0.1), in: Circle())
                    }
                    .buttonStyle(FocusPlainStyle(shape: Circle(), scale: 1))
                    .keyboardShortcut(.cancelAction)
                    .accessibilityLabel("Close")
                }
            }
            HStack(spacing: 8) {
                Glyph("search", size: 15).foregroundStyle(Theme.Palette.dim)
                TextField("Search episodes", text: $query)
                    .textFieldStyle(.plain)
                    .textFont(15, relativeTo: .body)
                    .foregroundStyle(Theme.Palette.text)
            }
            .padding(.horizontal, 12)
            .frame(height: metrics.wide ? 36 : 44)
            .background(Color(red: 118 / 255, green: 118 / 255, blue: 128 / 255).opacity(0.24), in: Capsule())
            ScrollViewReader { proxy in
                ScrollView(.vertical) {
                    LazyVStack(alignment: .leading, spacing: 4) {
                        ForEach(filterEpisodes(episodes, query: query), id: \.videoId) { episode in
                            PanelRow(episode: episode, minutes: minutes, actions: actions).id(episode.videoId)
                        }
                    }
                }
                .scrollIndicators(.hidden)
                .onChange(of: episodes.map(\.videoId), initial: true) { _, ids in
                    guard let target = scrollTo, ids.contains(target) else { return }
                    proxy.scrollTo(target, anchor: .top)
                }
            }
        }
        .onAppear { seasonFocused = true }
    }

    /// Prev and Next: the season before or after, off at either end.
    private func step(_ by: Int) -> some View {
        let next = neighbour(by)
        return Button { if let next { select(next) } } label: {
            HStack(spacing: 6) {
                if by < 0 { Glyph("chevron-left", size: 14) }
                Text(by < 0 ? "Prev" : "Next")
                if by > 0 { Glyph("chevron-right", size: 14) }
            }
            .textFont(13, weight: .semibold, relativeTo: .footnote)
            .foregroundStyle(next == nil ? Theme.Palette.text.opacity(0.4) : Theme.Palette.text)
            .padding(.horizontal, 14)
            .frame(height: metrics.wide ? 32 : 44)
            .background(.white.opacity(0.1), in: Capsule())
        }
        .buttonStyle(FocusPlainStyle(shape: Capsule(), scale: 1))
        .disabled(next == nil)
        .accessibilityLabel(next.map { by < 0 ? "Previous season, \(seasonName($0))" : "Next season, \(seasonName($0))" } ?? (by < 0 ? "Previous season" : "Next season"))
    }

    private var seasonMenu: some View {
        Menu {
            ForEach(ordered, id: \.self) { season in
                Button { select(season) } label: {
                    if season == selected { Label(seasonName(season), glyph: "check") } else { Text(seasonName(season)) }
                }
            }
        } label: {
            HStack(spacing: 6) {
                Text(seasonName(selected ?? ordered.first ?? 1))
                Glyph("chevron-down", size: 14)
            }
            .textFont(15, weight: .semibold, relativeTo: .subheadline)
            .foregroundStyle(Theme.Palette.text)
            .padding(.horizontal, 14)
            .frame(height: metrics.wide ? 32 : 44)
            .background(.white.opacity(0.1), in: Capsule())
        }
        .menuStyle(.button)
        .buttonStyle(.plain)
        .menuIndicator(.hidden)
        .fixedSize()
        .focused($seasonFocused)
    }

    private var ordered: [Int] { seasons.filter { $0 > 0 } + seasons.filter { $0 == 0 } }

    private func neighbour(_ by: Int) -> Int? {
        guard let selected, let index = ordered.firstIndex(of: selected) else { return nil }
        let next = index + by
        return ordered.indices.contains(next) ? ordered[next] : nil
    }
}

/// One dated row: the still with its progress, "2. Name", the date and caption, the Watched or
/// Upcoming flag, and Choose version and the watched mark (pills shown on hover or keyboard focus on
/// the Mac, always on iPad; round buttons on the iPhone; the row's actions and context menu too).
private struct PanelRow: View {
    let episode: Episode
    let minutes: Int?
    let actions: EpisodeActionsSet
    @Environment(\.metrics) private var metrics
    @State private var hovered = false
    @FocusState private var focused: Bool

    private var unaired: Bool { airsLater(episode.released) }

    var body: some View {
        HStack(alignment: metrics.wide ? .top : .center, spacing: 12) {
            Button { actions.play(episode) } label: { content }
                .buttonStyle(FocusPlainStyle(shape: RoundedRectangle(cornerRadius: 12, style: .continuous), scale: 1))
                .focused($focused)
                .disabled(unaired)
                .accessibilityElement(children: .combine)
                .accessibilityHint(unaired ? "" : "Plays this episode")
                .accessibilityActions { items }
                .contextMenu { items }
            if !metrics.wide && !unaired { phoneButtons }
        }
        .padding(8)
        .background(.white.opacity(metrics.wide && (hovered || focused) ? 0.06 : 0), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        .onHover { hovered = $0 }
    }

    private var content: some View {
        HStack(alignment: .top, spacing: 12) {
            ArtImage(url: episode.thumbnail, maxPixels: 320)
                .frame(width: 112, height: metrics.wide ? 70 : 63)
                .overlay(alignment: .bottom) {
                    if let progress = episode.progress, !episode.watched, progress > 0 {
                        GeometryReader { geo in
                            ZStack(alignment: .leading) {
                                Rectangle().fill(ProgressLine.track)
                                Rectangle().fill(ProgressLine.fill).frame(width: geo.size.width * min(max(progress, 0), 1))
                            }
                        }
                        .frame(height: 3)
                    }
                }
                .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
            VStack(alignment: .leading, spacing: 2) {
                Text("\(episode.episode). \(episode.title.isEmpty ? "Episode \(episode.episode)" : episode.title)")
                    .textFont(15, weight: .semibold, relativeTo: .subheadline)
                    .foregroundStyle(Theme.Palette.text)
                    .lineLimit(2)
                    .multilineTextAlignment(.leading)
                Text(episodeCaption(episode, minutes: minutes))
                    .textFont(13, weight: metrics.wide ? .medium : .regular, relativeTo: .footnote)
                    .foregroundStyle(Theme.Palette.dim)
                    .monospacedDigit()
                if episode.watched || unaired {
                    Text(episode.watched ? "Watched" : "Upcoming")
                        .textFont(11, weight: .semibold, relativeTo: .caption2)
                        .foregroundStyle(.white)
                        .padding(.horizontal, 7)
                        .padding(.vertical, 4)
                        .background(.white.opacity(0.16), in: RoundedRectangle(cornerRadius: 6, style: .continuous))
                        .padding(.top, 4)
                }
                if metrics.wide && !unaired && (hovered || focused || touchPad) { pills.padding(.top, 6) }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    /// iPad has no hover: its rows show their actions always.
    private var touchPad: Bool {
        #if os(iOS)
        return true
        #else
        return false
        #endif
    }

    private var pills: some View {
        HStack(spacing: 6) {
            pill("Choose version", glyph: "layers") { actions.chooseVersion(episode) }
            pill(episode.watched ? "Mark as not watched" : "Mark as watched", glyph: episode.watched ? "eye-off" : "eye") {
                actions.setWatched(episode.videoId, !episode.watched)
            }
        }
    }

    private func pill(_ title: String, glyph: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Label(title, glyph: glyph, size: 14)
                .labelStyle(PillLabelStyle())
                .textFont(13, weight: .semibold, relativeTo: .footnote)
                .foregroundStyle(.white)
                .padding(.horizontal, 12)
                .frame(height: 28)
                .background(SecondaryFill(shape: Capsule()))
        }
        .buttonStyle(FocusPlainStyle(shape: Capsule(), scale: 1))
    }

    private var phoneButtons: some View {
        HStack(spacing: 0) {
            round("layers", label: "Choose a version") { actions.chooseVersion(episode) }
            round(episode.watched ? "eye-off" : "eye", label: episode.watched ? "Mark as not watched" : "Mark as watched") {
                actions.setWatched(episode.videoId, !episode.watched)
            }
        }
    }

    private func round(_ glyph: String, label: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Glyph(glyph, size: 17)
                .foregroundStyle(.white)
                .frame(width: 36, height: 36)
                .background(SecondaryFill(shape: Circle()))
                .frame(width: 44, height: 44)
                .contentShape(Rectangle())
        }
        .buttonStyle(FocusPlainStyle(shape: Circle(), scale: 1))
        .accessibilityLabel(label)
    }

    @ViewBuilder private var items: some View {
        if !unaired {
            Button("Choose Version", glyph: "layers") { actions.chooseVersion(episode) }
            Button(episode.watched ? "Mark as Not Watched" : "Mark as Watched", glyph: episode.watched ? "eye-off" : "eye") {
                actions.setWatched(episode.videoId, !episode.watched)
            }
        }
    }
}
#endif
