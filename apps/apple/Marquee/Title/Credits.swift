import SwiftUI

/// Starring, direction, runtime and where the details came from.
struct Credits: View {
    let details: TitleDetails
    let series: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Rectangle().fill(Theme.Palette.line).frame(height: 1)
            if !details.cast.isEmpty { entry("Starring", details.cast.prefix(4).joined(separator: ", ")) }
            if !details.directors.isEmpty { entry("Directed by", details.directors.joined(separator: ", ")) }
            if let runtime = details.runtime { entry("Runtime", series ? "\(runtime) an episode" : runtime) }
            entry("Details from", details.source)
        }
    }

    private func entry(_ label: String, _ value: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(label.uppercased())
                .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
                .tracking(1.6)
                .foregroundStyle(Theme.Palette.muted)
            Text(value)
                .font(.display(Theme.type.headline, italic: true, relativeTo: .body))
                .foregroundStyle(Theme.Palette.text)
                .fixedSize(horizontal: false, vertical: true)
        }
        .accessibilityElement(children: .combine)
    }
}

/// Rating, years, up to three genres and the season count, joined by dots.
func metaLine(_ details: TitleDetails, seasons: [Int]) -> String {
    let regular = seasons.filter { $0 > 0 }.count
    let parts: [String?] = [
        details.rating.map { "\($0) IMDb" },
        details.releaseInfo,
        details.genres.isEmpty ? nil : details.genres.prefix(3).joined(separator: ", "),
        regular == 0 ? nil : regular == 1 ? "1 season" : "\(regular) seasons",
    ]
    return parts.compactMap { $0 }.joined(separator: "  ·  ")
}

/// "21:00" or "1:02:03".
func clock(ms: Int64) -> String {
    let total = Int(ms / 1000)
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60)
    return h > 0 ? String(format: "%d:%02d:%02d", h, m, s) : String(format: "%d:%02d", m, s)
}

/// Minutes from a runtime like "57 min", "2h 9min" or "1h".
func minutes(_ runtime: String?) -> Int? {
    guard let runtime else { return nil }
    let hours = runtime.firstMatch(of: /(\d+)\s*h/).flatMap { Int($0.1) } ?? 0
    let mins = runtime.firstMatch(of: /(\d+)\s*m/).flatMap { Int($0.1) } ?? 0
    let total = hours * 60 + mins
    return total > 0 ? total : nil
}

/// A core date ("2013-08-11") as a date at the start of that day, UTC.
func releaseDate(_ value: String?) -> Date? {
    guard let value else { return nil }
    return try? Date(value, strategy: .iso8601.year().month().day())
}
