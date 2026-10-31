import Combine
import SwiftUI

/// New and upcoming episodes of the series in the library, from a week back
/// to a month ahead, by date. It opens on today; an episode leads to its
/// series on that season (a date never promises a stream).
struct CalendarScreen: View {
    @State private var calendar: LiveRecord<CalendarSnapshot>
    @State private var scrolled = false
    @Environment(\.metrics) private var metrics
    @Environment(\.scenePhase) private var phase
    @Environment(\.openTab) private var openTab

    init(core: CoreClient) {
        _calendar = State(initialValue: LiveRecord("calendar", params: ["today": Self.today()], core: core))
    }

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView(.vertical) {
                LazyVStack(alignment: .leading, spacing: Theme.Space.m) {
                    ScreenTitle(text: "Calendar")
                    if case .failed(let failure) = calendar.stage {
                        failureLine(failure)
                    }
                    if let s = calendar.snapshot {
                        notes(s)
                        ForEach(s.days, id: \.date) { day in
                            DaySection(day: day, today: s.today).id(day.date)
                        }
                    } else if calendar.stage == .loading {
                        AgendaSkeleton()
                    }
                }
                .frame(maxWidth: 880, alignment: .leading)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, metrics.gutter)
                .padding(.vertical, Theme.Space.l)
            }
            .scrollIndicators(.hidden)
            // Over a kept snapshot the page has scrolled to today: the line is pinned where it is seen.
            .safeAreaInset(edge: .top, spacing: 0) {
                if case .failedKept(let failure) = calendar.stage {
                    failureLine(failure)
                        .padding(.horizontal, metrics.gutter)
                        .padding(.vertical, Theme.Space.xs)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(Theme.Palette.raised)
                }
            }
            .onChange(of: calendar.snapshot?.startDay, initial: true) { _, day in
                guard !scrolled, let day else { return }
                scrolled = true
                proxy.scrollTo(day, anchor: .top)
            }
        }
        .background(Theme.Palette.bg)
        .topScrim()
        .task(id: calendar.taskId) { calendar.start() }
        // The core needs the local date: at midnight and whenever the app comes back.
        .onChange(of: phase) { _, now in if now == .active { setToday() } }
        .onReceive(NotificationCenter.default.publisher(for: .NSCalendarDayChanged).receive(on: DispatchQueue.main)) { _ in
            setToday()
        }
    }

    private func failureLine(_ failure: String) -> some View {
        StateLine(symbol: "triangle-alert", text: failure, action: ("Try again", calendar.retry), spoken: true)
    }

    /// Loading, failed and unscheduled series, in words.
    @ViewBuilder private func notes(_ s: CalendarSnapshot) -> some View {
        if s.pending > 0 {
            StateLine(symbol: "clock", text: s.pending == 1 ? "Loading 1 series" : "Loading \(s.pending) series")
        }
        if !s.failed.isEmpty {
            StateLine(
                symbol: "triangle-alert",
                text: "Could not load " + s.failed.map { series in series.reason.map { "\(series.name) (\($0))" } ?? series.name }.joined(separator: ", "), spoken: true)
        }
        if !s.noSchedule.isEmpty {
            StateLine(symbol: "calendar-x", text: "No dated episodes yet: " + s.noSchedule.map(\.name).joined(separator: ", "))
        }
        if let empty = s.emptyWords {
            StateLine(symbol: "calendar", text: empty, action: (CalendarSnapshot.findSeries, { openTab(.discover) }))
        }
    }

    private func setToday() {
        calendar.command(["type": "setToday", "today": Self.today()])
    }

    /// The local date as "YYYY-MM-DD".
    static func today() -> String {
        let c = Calendar.current.dateComponents([.year, .month, .day], from: .now)
        return String(format: "%04d-%02d-%02d", c.year ?? 0, c.month ?? 0, c.day ?? 0)
    }
}

/// "Today", "Tomorrow", "Yesterday" or "Wednesday, 21 October", then its episodes.
private struct DaySection: View {
    let day: CalendarDay
    let today: String

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            Text(label)
                .display(Theme.type.title2)
                .foregroundStyle(day.date == today ? Theme.Palette.accent : Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            ForEach(day.items, id: \.videoId) { item in
                NavigationLink(value: TitleRoute(kind: "series", id: item.itemId, name: item.name, season: item.season, videoId: item.videoId)) {
                    CalendarRow(item: item)
                }
                .buttonStyle(.row)
            }
        }
    }

    private var label: String { dayHeading(day.date, today: today) }
}

/// The episode's still (or the series poster), series, "S1:E2 Title", watched.
private struct CalendarRow: View {
    let item: CalendarItem
    @State private var hovered = false
    /// The episode still, the size of an episode row's on TV beside its larger text.
    #if os(tvOS)
    private static let still = CGSize(width: 224, height: 126)
    #else
    private static let still = CGSize(width: 128, height: 72)
    #endif

    var body: some View {
        HStack(spacing: Theme.Space.s) {
            ArtImage(url: item.still ?? item.poster, maxPixels: 480, fallback: item.name)
                .frame(width: Self.still.width, height: Self.still.height)
                .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous))
            VStack(alignment: .leading, spacing: 4) {
                Text(item.name)
                    .textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                    .foregroundStyle(Theme.Palette.text)
                Text(episode)
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.muted)
                    .lineLimit(2)
                    .accessibilityLabel(spokenCodes(episode))
            }
            Spacer(minLength: 0)
            if item.watched {
                Glyph("check")
                    .foregroundStyle(Theme.Palette.muted)
                    .accessibilityLabel("Watched")
            }
            Glyph("chevron-right")
                .foregroundStyle(Theme.Palette.muted)
                .accessibilityHidden(true)
        }
        .padding(Theme.Space.xs)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
        .brightness(hovered ? 0.04 : 0)
        .contentShape(Rectangle())
        #if !os(tvOS)
        .onHover { hovered = $0 }
        #endif
        .accessibilityElement(children: .combine)
    }

    private var episode: String {
        guard let season = item.season, let number = item.episode else { return item.title }
        return item.title.isEmpty ? "S\(season):E\(number)" : "S\(season):E\(number) \(item.title)"
    }
}

/// The agenda still coming: a day's heading and its rows as blank shapes, no words.
private struct AgendaSkeleton: View {
    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            ForEach(0..<2, id: \.self) { _ in
                RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous)
                    .fill(Theme.Palette.surface)
                    .frame(width: 160, height: 22)
                ForEach(0..<2, id: \.self) { _ in
                    RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous)
                        .fill(Theme.Palette.surface)
                        .frame(height: 72 + 2 * Theme.Space.xs)
                }
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Loading")
    }
}
