import SwiftUI

/// Starring, direction, runtime and where the details came from: a glass
/// table, keys dim on the left, values on the right.
struct Credits: View {
    let details: TitleDetails
    let series: Bool
    @Environment(\.metrics) private var metrics

    var body: some View {
        VStack(spacing: 0) {
            ForEach(Array(facts.enumerated()), id: \.offset) { index, fact in
                if index > 0 { Rectangle().fill(Theme.Palette.line).frame(height: 1) }
                HStack(alignment: .firstTextBaseline, spacing: Theme.Space.s) {
                    Text(fact.key).foregroundStyle(Theme.Palette.dim)
                    Spacer(minLength: 0)
                    Text(fact.value)
                        .foregroundStyle(Theme.Palette.text)
                        .multilineTextAlignment(.trailing)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .padding(.horizontal, metrics.wide ? 18 : 14)
                .padding(.vertical, metrics.wide ? 12 : 11)
                .accessibilityElement(children: .combine)
            }
        }
        .font(.text(metrics.cardName, weight: .semibold, relativeTo: .footnote))
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
    }

    private var facts: [(key: String, value: String)] {
        var rows: [(key: String, value: String)] = []
        if !details.cast.isEmpty { rows.append(("Starring", details.cast.prefix(4).joined(separator: ", "))) }
        if !details.directors.isEmpty { rows.append(("Directed by", details.directors.joined(separator: ", "))) }
        if let runtime = details.runtime { rows.append(("Runtime", series ? "\(runtime) an episode" : runtime)) }
        rows.append(("Details from", details.source))
        return rows
    }
}

/// Years, up to three genres and the season count: the meta line after the rating.
func metaParts(_ details: TitleDetails, seasons: [Int]) -> [String] {
    let regular = seasons.filter { $0 > 0 }.count
    let parts: [String?] = [
        details.releaseInfo,
        details.genres.isEmpty ? nil : details.genres.prefix(3).joined(separator: ", "),
        regular == 0 ? nil : regular == 1 ? "1 season" : "\(regular) seasons",
    ]
    return parts.compactMap { $0 }
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
