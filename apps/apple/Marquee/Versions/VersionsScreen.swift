import SwiftUI

/// The stream picker: the recommended version with its reasons, then every
/// other version grouped by intent, then what was hidden and why.
struct VersionsScreen: View {
    let model: VersionsModel
    let heading: String
    let core: CoreClient
    @State private var playing: PlayRoute?
    @State private var addSource = false
    @State private var addAccount = false
    /// Blocked (3.11): "Show them" lists the versions as they would rank once fixed.
    @State private var showBlocked = false
    /// The learning note was undone here: it goes at once.
    @State private var undone = false
    @Environment(\.metrics) private var metrics
    @State private var expanded: Set<String> = []
    /// TV: the page enters on the pick's Play.
    @FocusState private var pickFocused: Bool
    @Environment(\.dynamicTypeSize) private var typeSize
    /// The page's width, for the layouts below.
    @State private var width: CGFloat = 0

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                header
                if sideBySide {
                    HStack(alignment: .top, spacing: Theme.Space.l) {
                        pick.frame(width: Self.pickWidth)
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
        .defaultFocus($pickFocused, true)
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
        .navigationDestination(item: $playing) { route in
            PlayerScreen(route: route, versions: model, core: core)
        }
        .navigationDestination(isPresented: $addSource) { AddonsPage(core: core, sourcesOnly: true) }
        .navigationDestination(isPresented: $addAccount) { DebridScreen(core: core) }
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
    }

    private var snapshot: VersionsSnapshot? { model.snapshot }

    /// The page inside its gutters, before the first width arrives the size class's guess.
    private var page: CGFloat { width > 0 ? width - 2 * metrics.gutter : (metrics.wide ? 1180 : 360) }

    /// The pick card beside the table: wider on TV, where its type is twice the size.
    #if os(tvOS)
    static let pickWidth: CGFloat = 600
    #else
    static let pickWidth: CGFloat = 360
    #endif

    /// The pick beside the table only with room for both (not an iPad in portrait).
    private var sideBySide: Bool { metrics.wide && WideLayout.pickBesideTable(page: page) }

    /// The table's columns only with room for them; else each row reads in two lines.
    private var columns: Bool {
        // Fixed columns do not grow with the text: at the accessibility sizes rows read in two lines.
        metrics.wide && !typeSize.isAccessibilitySize
            && WideLayout.tableColumns(table: sideBySide ? page - Self.pickWidth - Theme.Space.l : page)
    }

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
            Label(snapshot?.conditions.map { StreamChoice.conditionsHeader(device: StreamChoice.device, $0) } ?? DeviceCaps.line, glyph: "monitor-check")
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.muted)
                .padding(.top, 4)
        }
    }

    @ViewBuilder private var pick: some View {
        if let best = snapshot?.recommended {
            PickCard(row: best, play: { playing = model.play(best.key) }, focus: $pickFocused)
        } else if let blocked = snapshot?.blocked, let words = StreamChoice.blockedWords(blocked) {
            // Every addon answered and the hidden rows say why: the way on, as one action.
            VStack(alignment: .leading, spacing: 10) {
                Text("No playable version found")
                    .display(Theme.type.title2)
                    .foregroundStyle(Theme.Palette.text)
                    .accessibilityAddTraits(.isHeader)
                Text(words.text)
                    .font(.text(Theme.type.body))
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
                if blocked.reason == "needsAccount" {
                    Button { addAccount = true } label: { Label(words.action, glyph: "plus") }
                        .buttonStyle(PillButtonStyle(prominent: true))
                        .focused($pickFocused)
                        .padding(.top, 6)
                } else if !showBlocked {
                    Button { showBlocked = true } label: { Label(words.action, glyph: "eye") }
                        .buttonStyle(PillButtonStyle(prominent: true))
                        .focused($pickFocused)
                        .padding(.top, 6)
                }
            }
            .padding(Theme.Space.m)
            .frame(maxWidth: .infinity, alignment: .leading)
            .glass(RoundedRectangle(cornerRadius: Theme.Radius.l, style: .continuous), strong: true)
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
            if let note = snapshot?.note, !undone, let text = StreamChoice.learnedNote(note, device: StreamChoice.device) {
                LearnedNoteBar(text: text) { undo(note) }
            }
            if columns, !groups.isEmpty {
                ColumnHeadings()
            }
            ForEach(groups, id: \.0) { group, rows in
                VStack(alignment: .leading, spacing: 0) {
                    HStack {
                        Text(caps: VersionWords.group(group))
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
                        VersionRowView(row: row, wide: columns) {
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
                    Text("\(Text("Hidden (\(hidden.count))").fontWeight(Font.Weight.semibold.legible))  \(reasons)")
                } icon: {
                    Glyph("eye-off")
                }
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.text)
                .padding(14)
                .frame(maxWidth: .infinity, alignment: .leading)
                .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
                .modifier(ReadableOnTV())
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    /// The note's Undo: that choice goes back to zero on that screen class and stops learning there.
    private func undo(_ note: LearnedNote) {
        undone = true
        Task {
            do {
                try await core.perform("undoLearned", ["screenClass": note.screenClass, "choice": note.choice])
            } catch {
                undone = false
            }
        }
    }

    /// Rows by intent group, in the design's order, empty groups left out;
    /// blocked versions once the user asked to see them.
    private var groups: [(String, [VersionRow])] {
        let rows = showBlocked ? snapshot?.blocked?.rows ?? [] : snapshot?.rows ?? []
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
    /// TV: the page enters on Play.
    var focus: FocusState<Bool>.Binding

    static var kicker: some View {
        Text(caps: "Recommended for this screen")
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
            #if os(tvOS)
            // TV: Play under the heading, where the page enters, so it is in
            // view whatever the details below run to.
            playButton
            #endif
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
            #if !os(tvOS)
            playButton.padding(.top, 6)
            #endif
            Label(VersionWords.failover(row), glyph: "refresh-cw")
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.muted)
        }
        .padding(Theme.Space.m)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.l, style: .continuous), strong: true)
    }

    private var playButton: some View {
        Button(action: play) {
            Label("Play this version", glyph: "play-fill").frame(maxWidth: .infinity)
        }
        .buttonStyle(PillButtonStyle(prominent: true))
        .focused(focus)
    }

    private func fact(_ label: String, _ value: String, dot: Color? = nil) -> some View {
        GridRow {
            Text(caps: label)
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

/// The wide table's column names, over the same widths as its rows.
private struct ColumnHeadings: View {
    var body: some View {
        HStack(spacing: 16) {
            Text("Quality").frame(width: VersionRowView.qualityWidth, alignment: .leading)
            Text("Features").frame(maxWidth: .infinity, alignment: .leading)
            Text("Size").frame(width: VersionRowView.sizeWidth, alignment: .trailing)
            Text("Status").frame(width: VersionRowView.statusWidth, alignment: .leading)
            Text("Via").frame(width: VersionRowView.viaWidth, alignment: .leading)
        }
        .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
        .foregroundStyle(Theme.Palette.muted)
        .padding(.horizontal, 14)
        .accessibilityHidden(true)
    }
}

/// One alternative: quality, features, size, status and source; on a wide
/// table, focus or the pointer opens it to its release name, languages and
/// sources (a remote has no hover).
private struct VersionRowView: View {
    let row: VersionRow
    let wide: Bool
    let play: () -> Void
    @State private var hovered = false
    @FocusState private var focused: Bool
    /// Phone: the first tap opens the row (its details and Play), as on the web.
    @State private var tapped = false

    #if os(tvOS)
    static let qualityWidth: CGFloat = 120, sizeWidth: CGFloat = 150, statusWidth: CGFloat = 330, viaWidth: CGFloat = 220
    #else
    static let qualityWidth: CGFloat = 80, sizeWidth: CGFloat = 90, statusWidth: CGFloat = 200, viaWidth: CGFloat = 120
    #endif

    private var open: Bool { hovered || focused }

    var body: some View {
        Button(action: { if wide || tapped { play() } else { tapped = true } }) {
            VStack(alignment: .leading, spacing: 6) {
                if wide {
                    HStack(spacing: 16) {
                        Text(VersionWords.cell(row.quality))
                            .font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                            .frame(width: Self.qualityWidth, alignment: .leading)
                        Text(row.features.joined(separator: "  "))
                            .frame(maxWidth: .infinity, alignment: .leading)
                        Text(VersionWords.size(row.sizeBytes) ?? "")
                            .monospacedDigit()
                            .frame(width: Self.sizeWidth, alignment: .trailing)
                        status.frame(width: Self.statusWidth, alignment: .leading)
                        Text(row.via).foregroundStyle(Theme.Palette.muted).frame(width: Self.viaWidth, alignment: .leading)
                    }
                } else {
                    // Phone: two lines, "720p · HDR10 · 1.2 GB" over "Torrent, 12 seeds · Torrentio".
                    Text(([VersionWords.cell(row.quality)] + row.features + [VersionWords.size(row.sizeBytes)].compactMap { $0 })
                        .joined(separator: " \u{B7} "))
                        .font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                    HStack(spacing: 6) {
                        status
                        Text("\u{B7} \(row.via)").foregroundStyle(Theme.Palette.muted)
                    }
                }
                // The line's numbers: "7 seeders", "Ready on TorBox, 2.1 GB".
                if open || (!wide && tapped), let details = words?.details, !details.isEmpty {
                    Text(details)
                        .foregroundStyle(Theme.Palette.muted)
                }
                if !wide || open {
                    // One line on a phone, cut in the middle so the name never splits inside a token;
                    // an opened wide row shows it whole.
                    Text(row.releaseName)
                        .font(.mono(Theme.type.caption))
                        .foregroundStyle(Theme.Palette.muted)
                        .lineLimit(wide ? 3 : 1)
                        .truncationMode(.middle)
                }
                if wide && open {
                    Text(VersionWords.detail(row))
                        .foregroundStyle(Theme.Palette.muted)
                }
                if !wide && tapped {
                    Label("Play this version", glyph: "play-fill")
                        .font(.text(Theme.type.callout, weight: .semibold, relativeTo: .callout))
                        .padding(.top, 2)
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
        .focused($focused)
        #if !os(tvOS)
        .onHover { hovered = $0 }
        #endif
        .accessibilityLabel(StreamChoice.spoken(row))
    }

    /// The core's line in words, else (an older core) the status.
    private var words: LineWords? { row.line.flatMap { StreamChoice.lineWords($0) } }

    /// How it will start, with its tone as an icon and a word (stream choice 3.7).
    @ViewBuilder private var status: some View {
        if let words {
            Label {
                Text(words.text)
            } icon: {
                Glyph(words.tone.glyph).foregroundStyle(words.tone.color)
            }
            .labelStyle(.titleAndIcon)
        } else {
            HStack(spacing: 6) {
                Circle().fill(VersionWords.tone(row.status).color).frame(width: 8, height: 8)
                Text(VersionWords.status(row.status))
            }
        }
    }
}

/// The one-time learning note over the table, with Undo (stream choice 3.9).
private struct LearnedNoteBar: View {
    let text: String
    let undo: () -> Void

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            Glyph("sparkle").foregroundStyle(Theme.Palette.text).accessibilityHidden(true)
            Text(text)
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.text)
                .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 0)
            Button(action: undo) { Label(StreamChoice.words.learned.undo, glyph: "undo-2") }
                .buttonStyle(PillButtonStyle())
                .accessibilityLabel("\(StreamChoice.words.learned.undo): \(text)")
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
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
    /// The tone as an icon beside its word, never color alone.
    var glyph: String {
        switch self {
        case .ok: "circle-check"
        case .warn: "triangle-alert"
        case .bad: "circle-x"
        case .idle: "circle-dashed"
        }
    }

    var color: Color {
        switch self {
        case .idle: Theme.Palette.dim
        case .ok: Theme.Palette.ok
        case .warn: Theme.Palette.warn
        case .bad: Theme.Palette.bad
        }
    }
}
