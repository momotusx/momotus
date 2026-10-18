import SwiftUI

/// The stream picker: the recommended version with its reasons, then every
/// other version grouped by intent, then what was hidden and why.
struct VersionsScreen: View {
    let model: VersionsModel
    let heading: String
    let core: CoreClient
    @State private var playing: PlayRoute?
    @State private var addSource = false
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
        .topScrim()
        .onAppear(perform: model.freeze)
        .navigationDestination(item: $playing) { route in
            PlayerScreen(route: route, versions: model, core: core)
        }
        .navigationDestination(isPresented: $addSource) { AddonsPage(core: core, sourcesOnly: true) }
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
    }

    private var snapshot: VersionsSnapshot? { model.snapshot }

    private var title: some View {
        Text("Versions")
            .display(Theme.type.title1)
            .accessibilityAddTraits(.isHeader)
    }

    private var subject: some View {
        Text("for \(heading)")
            .font(.text(Theme.type.headline, relativeTo: .headline))
            .foregroundStyle(Theme.Palette.muted)
            .fixedSize(horizontal: false, vertical: true)
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 6) {
            // What the versions are for goes under the title, whole, when the two do not fit on one line.
            ViewThatFits(in: .horizontal) {
                HStack(alignment: .firstTextBaseline, spacing: 12) { title; subject }
                VStack(alignment: .leading, spacing: 2) { title; subject }
            }
            .foregroundStyle(Theme.Palette.text)
            let progress = VersionWords.progress(snapshot)
            DotLine(text: progress.text, tone: progress.tone)
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.text)
            Label(DeviceCaps.line, glyph: "monitor-check")
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.muted)
                .padding(.top, 4)
        }
    }

    @ViewBuilder private var pick: some View {
        if let best = snapshot?.recommended {
            PickCard(row: best) {
                playing = model.play(best.key)
            }
        } else {
            VStack(alignment: .leading, spacing: 10) {
                if let snapshot, snapshot.done {
                    Label(VersionWords.waitingTitle(snapshot), glyph: "circle-alert")
                        .font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                        .foregroundStyle(Theme.Palette.text)
                    Text(VersionWords.waitingBody(snapshot))
                        .font(.text(Theme.type.body))
                        .foregroundStyle(Theme.Palette.muted)
                    // With no version at all a source is the way on; with some, choosing one is.
                    if snapshot.rows.isEmpty {
                        Button { addSource = true } label: { Label("Add a source", glyph: "plus") }
                            .buttonStyle(PillButtonStyle(prominent: true))
                            .padding(.top, 6)
                    }
                } else {
                    // The pick's shape while the addons answer, as on the web.
                    VStack(alignment: .leading, spacing: 10) {
                        PickCard.kicker
                        ForEach([(160.0, 36.0), (240, 20), (200, 20)], id: \.0) { width, height in
                            RoundedRectangle(cornerRadius: 6, style: .continuous).fill(Theme.Palette.surface).frame(width: width, height: height)
                        }
                        Capsule().fill(Theme.Palette.surface).frame(height: 48)
                    }
                    .accessibilityElement(children: .ignore)
                    .accessibilityLabel("Finding versions")
                }
            }
            .padding(Theme.Space.m)
            .frame(maxWidth: .infinity, alignment: .leading)
            .glass(RoundedRectangle(cornerRadius: Theme.Radius.l, style: .continuous), strong: true)
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
                    .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
                    let shown = expanded.contains(group) ? rows : Array(rows.prefix(3))
                    ForEach(shown) { row in
                        VersionRowView(row: row, wide: metrics.wide) {
                            playing = model.play(row.key)
                        }
                        Rectangle().fill(Theme.Palette.line.opacity(0.6)).frame(height: 1)
                    }
                    if rows.count > 3 {
                        let open = expanded.contains(group)
                        Button {
                            if open { expanded.remove(group) } else { expanded.insert(group) }
                        } label: {
                            Label {
                                Text(open ? "Show the best 3" : "Show all \(rows.count)")
                            } icon: {
                                Glyph("chevron-down").rotationEffect(.degrees(open ? 180 : 0))
                            }
                        }
                        .buttonStyle(.textLink)
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
                    Glyph("eye-off")
                }
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.text)
                .padding(14)
                .frame(maxWidth: .infinity, alignment: .leading)
                .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
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

    static var kicker: some View {
        Text("RECOMMENDED FOR THIS SCREEN")
            .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
            .tracking(1.6)
            .foregroundStyle(Theme.Palette.muted)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Self.kicker
            Text(VersionWords.pickHeading(row))
                .display(Theme.type.title1 + 6)
                .foregroundStyle(Theme.Palette.text)
            Grid(alignment: .leading, horizontalSpacing: 18, verticalSpacing: 6) {
                ForEach(VersionWords.facts(row), id: \.key) { fact in
                    self.fact(fact.key, fact.value, dot: fact.key == "Status" ? VersionWords.tone(row.status).color : nil)
                }
            }
            VStack(alignment: .leading, spacing: 8) {
                ForEach(row.reasons.prefix(3), id: \.self) { reason in
                    Label {
                        Text(VersionWords.reason(reason, status: row.status))
                    } icon: {
                        Glyph("check").foregroundStyle(Theme.Palette.ok)
                    }
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.text)
                }
            }
            // Wraps at the dots between tokens, never inside one ("mp" / "4"). Not
            // selectable: a copy would carry the zero-width break points.
            Text(row.releaseName.replacingOccurrences(of: ".", with: ".\u{200B}"))
                .font(.mono(Theme.type.caption + 1))
                .foregroundStyle(Theme.Palette.muted)
            Button(action: play) {
                Label("Play this version", glyph: "play-fill").frame(maxWidth: .infinity)
            }
            .buttonStyle(PillButtonStyle(prominent: true))
            .padding(.top, 6)
            Label(VersionWords.failover(row), glyph: "refresh-cw")
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.muted)
        }
        .padding(Theme.Space.m)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.l, style: .continuous), strong: true)
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
                        Text(VersionWords.cell(row.quality))
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
                    // Phone: two lines, "720p · HDR10 · 1.2 GB" over "Torrent, 12 seeds · Torrentio".
                    Text(([VersionWords.cell(row.quality)] + row.features + [VersionWords.size(row.sizeBytes)].compactMap { $0 })
                        .joined(separator: " \u{B7} "))
                        .font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                    HStack(spacing: 6) {
                        Circle().fill(VersionWords.tone(row.status).color).frame(width: 8, height: 8)
                        Text("\(VersionWords.status(row.status)) \u{B7} \(row.via)")
                    }
                }
                if !wide || hovered {
                    // One line on a phone, cut in the middle so the name never splits inside a token.
                    Text(row.releaseName)
                        .font(.mono(Theme.type.caption))
                        .foregroundStyle(Theme.Palette.muted)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
            }
            .font(.text(Theme.type.callout, relativeTo: .callout))
            .foregroundStyle(Theme.Palette.text)
            .padding(.vertical, 12)
            .padding(.horizontal, 14)
            .contentShape(Rectangle())
            .background(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous).fill(.white.opacity(hovered ? 0.06 : 0)))
        }
        .buttonStyle(FocusPlainStyle(shape: RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous)))
        #if !os(tvOS)
        .onHover { hovered = $0 }
        #endif
        .accessibilityLabel("\(row.quality), \(VersionWords.status(row.status)), via \(row.via)")
    }

    private var status: some View {
        HStack(spacing: 6) {
            Circle().fill(VersionWords.tone(row.status).color).frame(width: 8, height: 8)
            Text(VersionWords.status(row.status))
        }
    }
}

/// A line led by a dot in its tone: "6 versions", "Asking Torrentio".
private struct DotLine: View {
    let text: String
    let tone: VersionWords.Tone

    var body: some View {
        HStack(spacing: 8) {
            Circle().fill(tone.color).frame(width: 8, height: 8)
            Text(text)
        }
    }
}

extension VersionWords.Tone {
    var color: Color {
        switch self {
        case .idle: Theme.Palette.dim
        case .ok: Theme.Palette.ok
        case .warn: Theme.Palette.warn
        case .bad: Theme.Palette.bad
        }
    }
}
