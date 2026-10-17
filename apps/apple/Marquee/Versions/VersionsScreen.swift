import SwiftUI

/// The stream picker: the recommended version with its reasons, then every
/// other version grouped by intent, then what was hidden and why.
struct VersionsScreen: View {
    let model: VersionsModel
    let heading: String
    let core: CoreClient
    @State private var playing: PlayRoute?
    @Environment(\.metrics) private var metrics
    @State private var expanded: Set<String> = []

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                header
                if metrics.wide {
                    HStack(alignment: .top, spacing: Theme.Space.l) {
                        pick.frame(width: 360)
                        table
                    }
                } else {
                    pick
                    table
                }
            }
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .onAppear(perform: model.freeze)
        .navigationDestination(item: $playing) { route in
            PlayerScreen(route: route, versions: model, core: core)
        }
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
    }

    private var snapshot: VersionsSnapshot? { model.snapshot }

    private var header: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .firstTextBaseline, spacing: 12) {
                Text("Versions")
                    .font(.display(Theme.type.title1))
                    .accessibilityAddTraits(.isHeader)
                Text("for \(heading)")
                    .font(.text(Theme.type.headline, relativeTo: .headline))
                    .foregroundStyle(Theme.Palette.muted)
                    .lineLimit(1)
            }
            .foregroundStyle(Theme.Palette.text)
            if let progress = progressLine {
                Label(progress, systemImage: snapshot?.done == true ? "checkmark.circle" : "clock")
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
            }
        }
    }

    /// Which addons are still answering, slow, or failed.
    private var progressLine: String? {
        guard let s = snapshot else { return "Asking your addons" }
        var parts: [String] = []
        let count = s.rows.count + (s.recommended == nil ? 0 : 1)
        parts.append(count == 1 ? "1 version" : "\(count) versions")
        if !s.pendingAddons.isEmpty { parts.append("still asking \(s.pendingAddons.joined(separator: ", "))") }
        if !s.slowAddons.isEmpty { parts.append("\(s.slowAddons.joined(separator: ", ")) is slow") }
        if !s.failedAddons.isEmpty { parts.append("\(s.failedAddons.joined(separator: ", ")) did not answer") }
        return parts.joined(separator: "  ·  ")
    }

    @ViewBuilder private var pick: some View {
        if let best = snapshot?.recommended {
            PickCard(row: best) {
                playing = model.play(best.key)
            }
        } else if snapshot?.done == true {
            VStack(alignment: .leading, spacing: 10) {
                Text("No version to play")
                    .font(.display(Theme.type.title2))
                    .foregroundStyle(Theme.Palette.text)
                Text(snapshot.map { $0.hidden.count > 0 ? VersionWords.hidden($0.hidden) : "None of your addons had a source for this." } ?? "")
                    .font(.text(Theme.type.body))
                    .foregroundStyle(Theme.Palette.muted)
            }
            .padding(Theme.Space.m)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous).fill(Theme.Palette.surface))
        } else {
            RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous)
                .fill(Theme.Palette.surface)
                .frame(height: 280)
                .accessibilityLabel("Finding versions")
        }
    }

    private var table: some View {
        VStack(alignment: .leading, spacing: 14) {
            ForEach(groups, id: \.0) { group, rows in
                VStack(alignment: .leading, spacing: 0) {
                    HStack {
                        Text(VersionWords.group(group).uppercased())
                            .tracking(1.6)
                        Spacer()
                        Text("\(rows.count)").monospacedDigit()
                    }
                    .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
                    .foregroundStyle(Theme.Palette.text)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 9)
                    .background(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous).fill(Theme.Palette.surface))
                    let shown = expanded.contains(group) ? rows : Array(rows.prefix(3))
                    ForEach(shown) { row in
                        VersionRowView(row: row, wide: metrics.wide) {
                            playing = model.play(row.key)
                        }
                        Rectangle().fill(Theme.Palette.line.opacity(0.6)).frame(height: 1)
                    }
                    if rows.count > 3 && !expanded.contains(group) {
                        Button {
                            expanded.insert(group)
                        } label: {
                            Label("Show all \(rows.count)", systemImage: "chevron.down")
                        }
                        .buttonStyle(.plain)
                        .font(.text(Theme.type.callout, weight: .medium, relativeTo: .callout))
                        .foregroundStyle(Theme.Palette.muted)
                        .padding(.vertical, 10)
                        .padding(.horizontal, 14)
                    }
                }
            }
            if let hidden = snapshot?.hidden, hidden.count > 0 {
                Label {
                    let reasons = Text(VersionWords.hidden(hidden)).foregroundStyle(Theme.Palette.muted)
                    Text("\(Text("Hidden (\(hidden.count))").fontWeight(.semibold))  \(reasons)")
                } icon: {
                    Image(systemName: "eye.slash")
                }
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.text)
                .padding(14)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous).fill(Theme.Palette.surface))
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    /// Rows by intent group, in the design's order, empty groups left out.
    private var groups: [(String, [VersionRow])] {
        let rows = snapshot?.rows ?? []
        let known = VersionWords.groupOrder
        let others = Set(rows.map(\.group)).subtracting(known).sorted()
        return (known + others).compactMap { group in
            let members = rows.filter { $0.group == group }
            return members.isEmpty ? nil : (group, members)
        }
    }
}

/// The recommended version: what it is, why it was picked, and Play.
private struct PickCard: View {
    let row: VersionRow
    let play: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("RECOMMENDED FOR THIS DEVICE")
                .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
                .tracking(1.6)
                .foregroundStyle(Theme.Palette.muted)
            Text(row.quality == "Unknown" ? "Quality unknown" : row.quality)
                .font(.display(Theme.type.title1 + 6))
                .foregroundStyle(Theme.Palette.text)
            Grid(alignment: .leading, horizontalSpacing: 18, verticalSpacing: 6) {
                if !row.features.isEmpty { fact("Features", row.features.joined(separator: ", ")) }
                if let size = VersionWords.size(row.sizeBytes) { fact("Size", size) }
                fact("Status", VersionWords.status(row.status), dot: VersionWords.statusColor(row.status))
                fact("Via", row.via)
                if !row.languages.isEmpty { fact("Audio", row.languages.joined(separator: ", ")) }
            }
            VStack(alignment: .leading, spacing: 8) {
                ForEach(row.reasons, id: \.self) { reason in
                    Label(VersionWords.reason(reason), systemImage: "checkmark")
                        .font(.text(Theme.type.callout, relativeTo: .callout))
                        .foregroundStyle(Theme.Palette.text)
                }
            }
            Text(row.releaseName)
                .font(.mono(Theme.type.caption + 1))
                .foregroundStyle(Theme.Palette.muted)
                .selectableText()
            Button(action: play) {
                Label("Play this version", systemImage: "play.fill")
            }
            .buttonStyle(PillButtonStyle(prominent: true))
            .padding(.top, 6)
        }
        .padding(Theme.Space.m)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous).fill(Theme.Palette.surface))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous).strokeBorder(Theme.Palette.line, lineWidth: 1))
    }

    private func fact(_ label: String, _ value: String, dot: Color? = nil) -> some View {
        GridRow {
            Text(label.uppercased())
                .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
                .tracking(1.4)
                .foregroundStyle(Theme.Palette.muted)
            HStack(spacing: 6) {
                if let dot { Circle().fill(dot).frame(width: 8, height: 8) }
                Text(value)
            }
            .font(.text(Theme.type.body))
            .foregroundStyle(Theme.Palette.text)
        }
    }
}

/// One alternative: quality, features, size, status and source; the full
/// release name shows where there is room or on hover.
private struct VersionRowView: View {
    let row: VersionRow
    let wide: Bool
    let play: () -> Void
    @State private var hovered = false

    var body: some View {
        Button(action: play) {
            VStack(alignment: .leading, spacing: 6) {
                if wide {
                    HStack(spacing: 16) {
                        Text(row.quality == "Unknown" ? "Unknown" : row.quality)
                            .font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                            .frame(width: 80, alignment: .leading)
                        Text(row.features.joined(separator: "  "))
                            .frame(maxWidth: .infinity, alignment: .leading)
                        Text(VersionWords.size(row.sizeBytes) ?? "")
                            .monospacedDigit()
                            .frame(width: 90, alignment: .trailing)
                        status.frame(width: 200, alignment: .leading)
                        Text(row.via).foregroundStyle(Theme.Palette.muted).frame(width: 120, alignment: .leading)
                    }
                } else {
                    HStack {
                        Text(row.quality == "Unknown" ? "Quality unknown" : row.quality)
                            .font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                        Text(row.features.joined(separator: "  "))
                        Spacer()
                        Text(VersionWords.size(row.sizeBytes) ?? "").monospacedDigit()
                    }
                    HStack {
                        status
                        Spacer()
                        Text(row.via).foregroundStyle(Theme.Palette.muted)
                    }
                }
                if !wide || hovered {
                    Text(row.releaseName)
                        .font(.mono(Theme.type.caption))
                        .foregroundStyle(Theme.Palette.muted)
                        .lineLimit(2)
                }
            }
            .font(.text(Theme.type.callout, relativeTo: .callout))
            .foregroundStyle(Theme.Palette.text)
            .padding(.vertical, 12)
            .padding(.horizontal, 14)
            .contentShape(Rectangle())
            .background(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous).fill(Theme.Palette.surface.opacity(hovered ? 0.8 : 0)))
        }
        .buttonStyle(.plain)
        #if !os(tvOS)
        .onHover { hovered = $0 }
        #endif
        .accessibilityLabel("\(row.quality), \(VersionWords.status(row.status)), via \(row.via)")
    }

    private var status: some View {
        HStack(spacing: 6) {
            Circle().fill(VersionWords.statusColor(row.status)).frame(width: 8, height: 8)
            Text(VersionWords.status(row.status))
        }
    }
}
