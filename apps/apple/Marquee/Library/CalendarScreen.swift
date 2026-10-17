import Combine
import SwiftUI

/// Pushes the calendar onto a stack.
struct CalendarRoute: Hashable {}

/// New and upcoming episodes of the series in the library, from a week back
/// to a month ahead, by date. It opens on today; an episode leads to its
/// series on that season (a date never promises a stream).
struct CalendarScreen: View {
    @State private var calendar: LiveRecord<CalendarSnapshot>
    @State private var scrolled = false
    @Environment(\.metrics) private var metrics
    @Environment(\.scenePhase) private var phase

    init(core: CoreClient) {
        _calendar = State(initialValue: LiveRecord("calendar", params: ["today": Self.today()], core: core))
    }

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView(.vertical) {
                LazyVStack(alignment: .leading, spacing: Theme.Space.m) {
                    ScreenTitle(text: "Calendar")
                    if let s = calendar.snapshot {
                        notes(s)
                        ForEach(s.days, id: \.date) { day in
                            DaySection(day: day, today: s.today).id(day.date)
                        }
                    } else if let failure = calendar.failure {
                        StateLine(symbol: "exclamationmark.triangle", text: failure)
                    }
                }
                .frame(maxWidth: 880, alignment: .leading)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, metrics.gutter)
                .padding(.vertical, Theme.Space.l)
            }
            .scrollIndicators(.hidden)
            .onChange(of: calendar.snapshot?.startDay, initial: true) { _, day in
                guard !scrolled, let day else { return }
                scrolled = true
                proxy.scrollTo(day, anchor: .top)
            }
        }
        .background(Theme.Palette.bg)
        .task(id: calendar.taskId) { calendar.start() }
        // The core needs the local date: at midnight and whenever the app comes back.
        .onChange(of: phase) { _, now in if now == .active { setToday() } }
        .onReceive(NotificationCenter.default.publisher(for: .NSCalendarDayChanged).receive(on: DispatchQueue.main)) { _ in
            setToday()
        }
    }

    /// Loading, failed and unscheduled series, in words.
    @ViewBuilder private func notes(_ s: CalendarSnapshot) -> some View {
        if s.pending > 0 {
            StateLine(symbol: "clock", text: s.pending == 1 ? "Loading 1 series" : "Loading \(s.pending) series")
        }
        if !s.failed.isEmpty {
            StateLine(
                symbol: "exclamationmark.triangle",
                text: "Could not load " + s.failed.map { series in series.reason.map { "\(series.name) (\($0))" } ?? series.name }.joined(separator: ", "))
        }
        if !s.noSchedule.isEmpty {
            StateLine(symbol: "calendar.badge.exclamationmark", text: "No dated episodes yet: " + s.noSchedule.map(\.name).joined(separator: ", "))
        }
        if s.days.isEmpty, s.pending == 0 {
            StateLine(symbol: "calendar", text: "Nothing airs from a week ago to a month ahead. Series you add to your Library show their episodes here.")
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
                .font(.display(Theme.type.title2))
                .foregroundStyle(day.date == today ? Theme.Palette.accent : Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            ForEach(day.items, id: \.videoId) { item in
                NavigationLink(value: TitleRoute(kind: "series", id: item.itemId, name: item.name, season: item.season)) {
                    CalendarRow(item: item)
                }
                .buttonStyle(.plain)
            }
        }
    }

    private var label: String {
        guard let date = releaseDate(day.date), let now = releaseDate(today) else { return day.date }
        switch Calendar.current.dateComponents([.day], from: now, to: date).day {
        case 0: return "Today"
        case 1: return "Tomorrow"
        case -1: return "Yesterday"
        default: return date.formatted(.dateTime.weekday(.wide).day().month(.wide))
        }
    }
}

/// The episode's still (or the series poster), series, "S1:E2 Title", watched.
private struct CalendarRow: View {
    let item: CalendarItem
    @State private var hovered = false

    var body: some View {
        HStack(spacing: Theme.Space.s) {
            ArtImage(url: item.still ?? item.poster, maxPixels: 480, fallback: item.name)
                .frame(width: 128, height: 72)
                .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous))
            VStack(alignment: .leading, spacing: 4) {
                Text(item.name)
                    .font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                    .foregroundStyle(Theme.Palette.text)
                Text(episode)
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
                    .lineLimit(2)
            }
            Spacer(minLength: 0)
            if item.watched {
                Image(systemName: "checkmark")
                    .foregroundStyle(Theme.Palette.muted)
                    .accessibilityLabel("Watched")
            }
            Image(systemName: "chevron.right")
                .foregroundStyle(Theme.Palette.muted)
                .accessibilityHidden(true)
        }
        .padding(Theme.Space.xs)
        .background(
            RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous)
                .fill(Theme.Palette.surface.opacity(hovered ? 1 : 0.6)))
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
