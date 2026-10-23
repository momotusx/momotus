import SwiftUI

/// The stream picker as the core lays it out (plans/play-every-version.md Step 3): the pick with its
/// Play, the Smaller file and Sharper shortcuts, then the quality sections. When nothing can play, the
/// core's line with the shared table's actions, and the rows that can still start listed dimmed. No
/// addon, no hidden count, no device line, no account card.
struct VersionsScreen: View {
    let model: VersionsModel
    let heading: String
    let core: CoreClient
    /// Over the paused player: a pick goes back to it (`choose`), Back returns to it as it was, and
    /// the rows hold their order while the page is open.
    var choose: ((String) -> Void)?
    var back: (() -> Void)?
    @State private var playing: PlayRoute?
    @State private var addSource = false
    @State private var addAccount = false
    /// The learning note was undone here: it goes at once.
    @State private var undone = false
    @Environment(\.metrics) private var metrics
    @Environment(\.dismiss) private var dismiss
    @State private var expanded: Set<String> = []
    /// Phone: the one row a tap has opened (its details and Play).
    @State private var opened: String?
    /// The sections' keys as first shown, kept while over the player.
    @State private var held: [String: [String]] = [:]
    /// TV: the page enters on the pick's Play, or the empty state's first action.
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
                        lead.frame(width: Self.pickWidth)
                        table
                    }
                } else {
                    lead
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
        .onChange(of: snapshot?.sections, initial: true) { _, sections in
            if choose != nil { held = VersionWords.hold(sections, shown: held) }
        }
        #if DEBUG
        .onChange(of: snapshot, initial: true) { pickRowForWalk() }
        #endif
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

    /// Plays `key`: back in the player under the page, else a new player.
    private func play(_ key: String) {
        if let choose { choose(key) } else { playing = model.play(key) }
    }

    private func leave() {
        if let back { back() } else { dismiss() }
    }

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
            .textFont(Theme.type.headline, relativeTo: .headline)
            .foregroundStyle(Theme.Palette.muted)
            .fixedSize(horizontal: false, vertical: true)
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 6) {
            if let back {
                // TV: the remote's Back (the player's exit command) returns to it.
                Button(action: back) { Label(StreamChoice.words.page.back, glyph: "chevron-left") }
                    .buttonStyle(PillButtonStyle())
                    #if !os(tvOS)
                    .keyboardShortcut(.cancelAction)
                    #endif
                    .padding(.bottom, 6)
            }
            // What the versions are for goes under the title, whole, when the two do not fit on one line.
            ViewThatFits(in: .horizontal) {
                HStack(alignment: .firstTextBaseline, spacing: 12) { title; subject }
                VStack(alignment: .leading, spacing: 2) { title; subject }
            }
            .foregroundStyle(Theme.Palette.text)
            // The count, then the reserved status line: held from the first frame, so nothing moves
            // when "Finding versions" clears.
            HStack(spacing: 14) {
                if let count = VersionWords.count(snapshot) {
                    DotLine(text: count, tone: snapshot?.empty == nil ? .ok : .idle)
                }
                Text(VersionWords.status(snapshot) ?? " ")
                    .foregroundStyle(Theme.Palette.muted)
                    .accessibilityHidden(VersionWords.status(snapshot) == nil)
            }
            .textFont(Theme.type.callout, relativeTo: .callout)
            .foregroundStyle(Theme.Palette.text)
        }
    }

    /// The card's place: why nothing can play with its actions, else the pick and its shortcuts, else
    /// the pick's shape while versions are found.
    @ViewBuilder private var lead: some View {
        if let empty = snapshot?.empty {
            EmptyCard(empty: empty, act: act, focus: $pickFocused)
        } else if let best = snapshot?.recommended {
            VStack(alignment: .leading, spacing: 10) {
                PickCard(row: best, play: { play(best.key) }, focus: $pickFocused)
                ForEach(VersionWords.shortcuts(snapshot), id: \.row.key) { shortcut in
                    Button { play(shortcut.row.key) } label: {
                        Label(shortcut.label, glyph: shortcut.row.key == snapshot?.smaller?.key ? "gauge" : "gem")
                    }
                    .buttonStyle(PillButtonStyle())
                    .accessibilityHint("Plays this version")
                }
            }
        } else if snapshot?.done != true {
            // The pick's shape while the addons answer, so nothing below moves when it fills.
            VStack(alignment: .leading, spacing: 10) {
                PickCard.kicker
                ForEach([(160.0, 36.0), (240, 20), (200, 20)], id: \.0) { width, height in
                    RoundedRectangle(cornerRadius: 6, style: .continuous).fill(Theme.Palette.surface).frame(width: width, height: height)
                }
                Capsule().fill(Theme.Palette.surface).frame(height: 48)
            }
            .padding(Theme.Space.m)
            .frame(maxWidth: .infinity, alignment: .leading)
            .glass(RoundedRectangle(cornerRadius: Theme.Radius.l, style: .continuous), strong: true)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(StreamChoice.words.page.finding)
        }
    }

    private func act(_ act: VersionWords.EmptyAct) {
        switch act {
        case .settingsAddons: addSource = true
        case .settingsAccounts: addAccount = true
        case .playFromDevice(let key): play(key)
        case .tryAgain: model.retry()
        case .back: leave()
        }
    }

    private var table: some View {
        VStack(alignment: .leading, spacing: 14) {
            if let note = snapshot?.note, !undone, let text = StreamChoice.learnedNote(note, device: StreamChoice.device) {
                LearnedNoteBar(text: text) { undo(note) }
            }
            let sections = VersionWords.sections(snapshot, held: choose == nil ? nil : held)
            if columns, !sections.isEmpty {
                ColumnHeadings()
            }
            ForEach(sections, id: \.quality) { section in
                VStack(alignment: .leading, spacing: 0) {
                    if let title = section.title {
                        HStack {
                            Text(caps: title)
                                .tracking(1.6)
                            Spacer()
                            Text("\(section.rows.count)").monospacedDigit()
                        }
                        .textFont(Theme.type.caption, weight: .semibold, relativeTo: .caption)
                        .foregroundStyle(Theme.Palette.text)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 9)
                        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
                        .accessibilityElement(children: .combine)
                        .accessibilityAddTraits(.isHeader)
                    }
                    let open = expanded.contains(section.quality)
                    ForEach(open ? section.rows : Array(section.rows.prefix(3))) { row in
                        VersionRowView(row: row, wide: columns, dimmed: VersionWords.dimmedStatus(snapshot?.empty?.kind),
                                       tapped: opened == row.key, openRow: { opened = row.key }) {
                            play(row.key)
                        }
                        Rectangle().fill(Theme.Palette.line.opacity(0.6)).frame(height: 1)
                    }
                    if section.rows.count > 3 {
                        Button {
                            if open { expanded.remove(section.quality) } else { expanded.insert(section.quality) }
                        } label: {
                            Label {
                                Text(open ? StreamChoice.words.page.showFewer : VersionWords.showAll(section.rows.count))
                            } icon: {
                                Glyph("chevron-down").rotationEffect(.degrees(open ? 180 : 0))
                            }
                        }
                        .buttonStyle(.textLink)
                        .textFont(Theme.type.callout, weight: .medium, relativeTo: .callout)
                        .foregroundStyle(Theme.Palette.muted)
                        .padding(.vertical, 10)
                        .padding(.horizontal, 14)
                    }
                }
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

    #if DEBUG
    /// `-MQPickRow N`: row N of the done list, once, as a hand pick.
    private func pickRowForWalk() {
        let rows = VersionWords.sections(snapshot).flatMap(\.rows)
        let n = DebugHooks.pickRow
        guard !DebugHooks.pickRowTaken, n > 0, snapshot?.done == true, rows.count >= n, playing == nil else { return }
        DebugHooks.pickRowTaken = true
        play(rows[n - 1].key)
    }
    #endif
}

/// Why nothing can play: the core's line, then the shared table's actions, the first the primary
/// with focus, Back last. No account card: Settings is where an account is added.
private struct EmptyCard: View {
    let empty: EmptyVersions
    let act: (VersionWords.EmptyAct) -> Void
    var focus: FocusState<Bool>.Binding

    var body: some View {
        let acts = VersionWords.emptyActs(empty)
        VStack(alignment: .leading, spacing: 12) {
            Text(VersionWords.emptyLine(empty.kind) ?? StreamChoice.words.empty["noResults"]?.text ?? "")
                .display(Theme.type.title2)
                .foregroundStyle(Theme.Palette.text)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityAddTraits(.isHeader)
            if let first = acts.first {
                button(first).buttonStyle(PillButtonStyle(prominent: true)).focused(focus)
            }
            ForEach(acts.dropFirst(), id: \.self) { item in
                button(item).buttonStyle(PillButtonStyle())
            }
        }
        .padding(Theme.Space.m)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.l, style: .continuous), strong: true)
    }

    private func button(_ item: VersionWords.EmptyAct) -> some View {
        Button { act(item) } label: { Label(VersionWords.emptyActWord(item), glyph: Self.glyph(item)) }
    }

    static func glyph(_ act: VersionWords.EmptyAct) -> String {
        switch act {
        case .settingsAddons, .settingsAccounts: "settings"
        case .playFromDevice: "play-fill"
        case .tryAgain: "refresh-cw"
        case .back: "chevron-left"
        }
    }
}

/// The recommended version: what it is, why it was picked, and Play.
private struct PickCard: View {
    let row: VersionRow
    let play: () -> Void
    /// TV: the page enters on Play.
    var focus: FocusState<Bool>.Binding

    /// The pick's heading: a plain 64 percent line, not an eyebrow.
    static var kicker: some View {
        Text("Best for this screen")
            .textFont(Theme.type.caption, weight: .semibold, relativeTo: .caption)
            .foregroundStyle(Theme.Palette.dim)
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
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.text)
                }
                // A version that plays in a window says so, with its minutes ahead.
                if let line = StreamChoice.cardLine(row) {
                    Label {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(line.text).foregroundStyle(Theme.Palette.text)
                            Text(line.details)
                                .textFont(Theme.type.caption, relativeTo: .caption)
                                .foregroundStyle(Theme.Palette.muted)
                        }
                    } icon: {
                        Glyph("check").foregroundStyle(Theme.Palette.ok)
                    }
                    .textFont(Theme.type.callout, relativeTo: .callout)
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
                .textFont(Theme.type.callout, relativeTo: .callout)
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
                .textFont(Theme.type.caption, weight: .semibold, relativeTo: .caption)
                .tracking(1.4)
                .foregroundStyle(Theme.Palette.muted)
            HStack(spacing: 6) {
                if let dot { Circle().fill(dot).frame(width: 8, height: 8) }
                Text(value)
            }
            .textFont(Theme.type.body)
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
        }
        .textFont(Theme.type.caption, weight: .semibold, relativeTo: .caption)
        .foregroundStyle(Theme.Palette.muted)
        .padding(.horizontal, 14)
        .accessibilityHidden(true)
    }
}

/// One alternative: quality, features, size and status; on a wide table,
/// focus or the pointer opens it to its release name, languages and sources
/// (a remote has no hover). Dimmed, it still starts: its words go to the muted
/// tone (never a faded row) and its status says why it may not play.
private struct VersionRowView: View {
    let row: VersionRow
    let wide: Bool
    /// The empty state's word and icon for a row that may not play here.
    let dimmed: (text: String, glyph: String)?
    /// Phone: the first tap opens the row (its details and Play), as on the
    /// web; one row is open at a time, so a tap on another closes this one.
    let tapped: Bool
    let openRow: () -> Void
    let play: () -> Void
    @State private var hovered = false
    @FocusState private var focused: Bool

    #if os(tvOS)
    // Beside the 600 pt pick the table has 960 pt: these leave Features about 320.
    static let qualityWidth: CGFloat = 150, sizeWidth: CGFloat = 150, statusWidth: CGFloat = 250
    #else
    static let qualityWidth: CGFloat = 80, sizeWidth: CGFloat = 90, statusWidth: CGFloat = 200
    #endif

    private var open: Bool { hovered || focused }

    var body: some View {
        Button(action: { if wide || tapped { play() } else { openRow() } }) {
            VStack(alignment: .leading, spacing: 6) {
                if wide {
                    HStack(spacing: 16) {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(VersionWords.cell(row.quality))
                                .textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                                // One word: smaller before it would break inside it.
                                .lineLimit(1)
                                .minimumScaleFactor(0.75)
                            languageChip
                        }
                        .frame(width: Self.qualityWidth, alignment: .leading)
                        Text(row.features.joined(separator: "  "))
                            .frame(maxWidth: .infinity, alignment: .leading)
                        Text(VersionWords.size(row.sizeBytes) ?? "")
                            .monospacedDigit()
                            .frame(width: Self.sizeWidth, alignment: .trailing)
                        status.frame(width: Self.statusWidth, alignment: .leading)
                    }
                } else {
                    // Phone: two lines, "720p · HDR10 · 1.2 GB" over "Torrent, 12 seeds".
                    HStack(spacing: 8) {
                        Text(([VersionWords.cell(row.quality)] + row.features + [VersionWords.size(row.sizeBytes)].compactMap { $0 })
                            .joined(separator: " \u{B7} "))
                            .textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                        languageChip
                    }
                    status
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
                        .textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout)
                        .padding(.top, 2)
                }
            }
            .textFont(Theme.type.callout, relativeTo: .callout)
            .foregroundStyle(dimmed == nil ? Theme.Palette.text : Theme.Palette.muted)
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
        .accessibilityLabel([StreamChoice.spoken(row), dimmed?.text].compactMap { $0 }.joined(separator: ", "))
        // The label already says the details: one activation plays.
        .accessibilityAction { play() }
        .accessibilityHint("Plays this version")
    }

    /// The core's line in words, else (an older core) the status.
    private var words: LineWords? { row.line.flatMap { StreamChoice.lineWords($0) } }

    /// Another language than the viewer's: its language beside the quality.
    @ViewBuilder private var languageChip: some View {
        if row.group == "otherLanguages", !row.languages.isEmpty {
            Text(VersionWords.languages(row))
                .textFont(Theme.type.caption, relativeTo: .caption)
                .lineLimit(1)
                .padding(.horizontal, 6)
                .padding(.vertical, 2)
                .background(RoundedRectangle(cornerRadius: 4, style: .continuous).fill(Theme.Palette.surface))
        }
    }

    /// How it will start, with its tone as an icon and a word (stream choice 3.7); dimmed, why it may not.
    @ViewBuilder private var status: some View {
        if let dimmed {
            Label(dimmed.text, glyph: dimmed.glyph).labelStyle(.titleAndIcon).lineLimit(1)
        } else if let words {
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
                .textFont(Theme.type.callout, relativeTo: .callout)
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

/// A line led by a dot in its tone: "6 versions".
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
