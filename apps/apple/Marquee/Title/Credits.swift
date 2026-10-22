import SwiftUI

/// Starring, direction and runtime: a glass table, keys dim on the left, values on the right. No
/// addon's name: it shows on the Addons pages alone (design/DECISION.md, Shelves).
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
        .textFont(metrics.cardName, weight: .semibold, relativeTo: .footnote)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
    }

    private var facts: [(key: String, value: String)] {
        var rows: [(key: String, value: String)] = []
        if !details.cast.isEmpty { rows.append(("Starring", details.cast.prefix(4).joined(separator: ", "))) }
        if !details.directors.isEmpty { rows.append(("Directed by", details.directors.joined(separator: ", "))) }
        if let runtime = details.runtime { rows.append(("Runtime", series ? "\(runtime) an episode" : runtime)) }
        return rows
    }
}

/// Years, up to three genres and the season count: the meta line after the scores.
func metaParts(_ details: TitleDetails, seasons: [Int]) -> [String] {
    let regular = seasons.filter { $0 > 0 }.count
    let parts: [String?] = [
        details.releaseInfo,
        details.genres.isEmpty ? nil : details.genres.prefix(3).joined(separator: ", "),
        regular == 0 ? nil : regular == 1 ? "1 season" : "\(regular) seasons",
    ]
    return parts.compactMap { $0 }
}
