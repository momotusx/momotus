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
    /// The download purpose (plans/offline-downloads.md 3.2): a pick keeps that version on this device.
    var keep: ((String) -> Void)?
    @State private var playing: PlayRoute?
    @State private var addSource = false
    /// The learning note was undone here: it goes at once.
    @State private var undone = false
    @Environment(\.metrics) private var metrics
    @Environment(\.dismiss) private var dismiss
    @State private var expanded: Set<String> = []
    /// The sections' keys as first shown, kept while over the player.
    @State private var held: [String: [String]] = [:]
    /// TV: the page enters on the pick's Play, or the empty state's first action.
    @FocusState private var pickFocused: Bool
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
        .environment(\.versionAct, keep == nil ? .play : .keep)
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
    }

    private var snapshot: VersionsSnapshot? { model.snapshot }

    /// Plays `key`: back in the player under the page, else a new player.
    private func play(_ key: String) {
        if let keep { keep(key) } else if let choose { choose(key) } else { playing = model.play(key) }
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
                    .accessibilityHint(keep == nil ? VersionAct.play.hint : VersionAct.keep.hint)
                }
            }
        } else if snapshot?.done != true {
            // The pick's shape while the addons answer, so nothing below moves when it fills.
            VStack(alignment: .leading, spacing: 10) {
                PickCard.kicker(keep == nil ? .play : .keep)
                ForEach([(160.0, 36.0), (240, 20), (200, 20)], id: \.0) { width, height in
                    RoundedRectangle(cornerRadius: 6, style: .continuous).fill(Theme.Palette.surface).frame(width: width, height: height)
                }
                Capsule().fill(Theme.Palette.surface).frame(height: 48)
            }
            .flatCard()
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(StreamChoice.words.page.finding)
        }
    }

    private func act(_ act: VersionWords.EmptyAct) {
        switch act {
        case .settingsAddons: addSource = true
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
            ForEach(sections, id: \.quality) { section in
                VStack(alignment: .leading, spacing: 0) {
                    if let title = section.title {
                        // A plain heading, no outline: "1080p  2".
                        HStack(alignment: .firstTextBaseline, spacing: 10) {
                            Text(title).textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                            Text("\(section.rows.count)").monospacedDigit().foregroundStyle(Theme.Palette.dim)
                        }
                        .foregroundStyle(Theme.Palette.text)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 8)
                        .accessibilityElement(children: .ignore)
                        .accessibilityLabel("\(title), \(StreamChoice.counted("versions", section.rows.count))")
                        .accessibilityAddTraits(.isHeader)
                    }
                    let open = expanded.contains(section.quality)
                    ForEach(open ? section.rows : Array(section.rows.prefix(3))) { row in
                        VersionRowView(row: row, dimmed: VersionWords.dimmedStatus(snapshot?.empty?.kind)) { play(row.key) }
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
        .flatCard()
    }

    private func button(_ item: VersionWords.EmptyAct) -> some View {
        Button { act(item) } label: { Label(VersionWords.emptyActWord(item), glyph: Self.glyph(item)) }
    }

    static func glyph(_ act: VersionWords.EmptyAct) -> String {
        switch act {
        case .settingsAddons: "settings"
        case .playFromDevice: "play-fill"
        case .tryAgain: "refresh-cw"
        case .back: "chevron-left"
        }
    }
}

extension View {
    /// A card on the page: a flat `surface` with no edge (design/DECISION.md, Surfaces).
    func flatCard() -> some View {
        padding(Theme.Space.m)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: Theme.Radius.l, style: .continuous).fill(Theme.Palette.surface))
    }
}

/// The recommended version: the kicker, its badges, one why line, Play.
private struct PickCard: View {
    let row: VersionRow
    let play: () -> Void
    /// TV: the page enters on Play.
    var focus: FocusState<Bool>.Binding

    @Environment(\.versionAct) private var act

    static func kicker(_ act: VersionAct = .play) -> some View {
        Text(act == .keep ? DownloadCopy.pick : VersionWords.kicker)
            .textFont(Theme.type.caption, weight: .semibold, relativeTo: .caption)
            .foregroundStyle(Theme.Palette.dim)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Self.kicker(act)
            BadgeRow(badges: VersionWords.badges(row), big: true)
            // One line; the whole sentence is read out.
            Text(VersionWords.why(row))
                .textFont(Theme.type.callout, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.muted)
                .lineLimit(1)
                .truncationMode(.tail)
            Button(action: play) {
                Label(act.label, glyph: act.glyph).frame(maxWidth: .infinity)
            }
            .buttonStyle(PillButtonStyle(prominent: true))
            .focused(focus)
        }
        .flatCard()
        .accessibilityElement(children: .contain)
        .accessibilityLabel(VersionWords.cardSpoken(row))
    }
}

/// Apple's badge row: the quality filled, Dolby words plain, short marks outlined.
private struct BadgeRow: View {
    let badges: [VersionWords.Badge]
    var big = false
    /// A dimmed row's badges take the muted tone with its words.
    var tone = Theme.Palette.text

    var body: some View {
        HStack(spacing: 8) {
            // No badge (a quality not stated, no format): an unseen one keeps the line's height.
            if badges.isEmpty { badge(.init(text: "4K", look: .filled)).hidden() }
            ForEach(badges, id: \.self, content: badge)
        }
        .accessibilityElement(children: .combine)
        // No badge is no VoiceOver stop.
        .accessibilityHidden(badges.isEmpty)
    }

    @ViewBuilder private func badge(_ badge: VersionWords.Badge) -> some View {
        let label = Text(badge.text)
            .textFont(big ? Theme.type.headline : Theme.type.callout, weight: .semibold, relativeTo: .headline)
            .lineLimit(1)
        switch badge.look {
        case .filled:
            label.foregroundStyle(Theme.Palette.bg)
                .padding(.horizontal, 6).padding(.vertical, 1)
                .background(RoundedRectangle(cornerRadius: 4, style: .continuous).fill(tone))
        case .outlined:
            label.foregroundStyle(tone)
                // 1 pt as the filled badge's, so a row of outlined badges only is as tall.
                .padding(.horizontal, 5).padding(.vertical, 1)
                .overlay(RoundedRectangle(cornerRadius: 4, style: .continuous).stroke(Theme.Palette.muted, lineWidth: 1))
        case .plain:
            label.foregroundStyle(tone)
        }
    }
}

/// What a row shows, the same in the row and atop its details sheet, so the sheet drops nothing: its badges and
/// language tag, then its one line (the size first, then its source, the core's warning or why it is dimmed).
private struct RowFace: View {
    let row: VersionRow
    let dimmed: (text: String, glyph: String)?
    var big = false

    var body: some View {
        HStack(spacing: 8) {
            BadgeRow(badges: VersionWords.badges(row), big: big, tone: dimmed == nil ? Theme.Palette.text : Theme.Palette.muted)
            if let tag = VersionWords.languageTag(row) {
                Text(tag)
                    .textFont(Theme.type.caption, relativeTo: .caption)
                    .lineLimit(1)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(RoundedRectangle(cornerRadius: 4, style: .continuous).fill(Theme.Palette.raised))
            }
        }
        RowLine(row: row, dimmed: dimmed)
    }
}

/// One version: its badges and language tag, one quiet line with the size first, and a details line
/// that is always laid out and fades in on focus or hover, so a row never changes height. Dimmed, it
/// still starts: its words go to the muted tone (never a faded row) and its line says why.
private struct VersionRowView: View {
    let row: VersionRow
    /// The empty state's word and icon for a row that may not play here.
    let dimmed: (text: String, glyph: String)?
    let play: () -> Void
    @State private var hovered = false
    @FocusState private var focused: Bool
    @Environment(\.metrics) private var metrics
    /// iPhone and iPad: the details sheet, opened from the row's info button.
    @State private var sheet = false
    /// VoiceOver goes back to the row when the sheet closes.
    @AccessibilityFocusState private var rowFocused: Bool
    @Environment(\.versionAct) private var act

    /// Touch (iPhone, and an iPad with no pointer): no hover or focus shows the details, so an info
    /// button opens them in a sheet. An iPad with a pointer has both.
    private var touch: Bool {
        #if os(iOS)
        true
        #else
        false
        #endif
    }

    /// The reserved details line: wherever a pointer or focus can show it (not on the iPhone).
    private var detailsLine: Bool { !touch || metrics.wide }

    var body: some View {
        // The row and its info button are siblings, never a button inside a button.
        HStack(spacing: 0) {
            rowButton
            if touch {
                Button { sheet = true } label: {
                    Glyph("info", size: 18).foregroundStyle(Theme.Palette.muted).frame(width: 44, height: 44)
                }
                .buttonStyle(FocusPlainStyle(shape: Circle()))
                // One VoiceOver stop per row: the row carries Details as an action.
                .accessibilityHidden(true)
                .sheet(isPresented: $sheet, onDismiss: { rowFocused = true }) {
                    DetailsSheet(row: row, dimmed: dimmed) {
                        sheet = false
                        play()
                    }
                    .environment(\.versionAct, act)
                }
            }
        }
    }

    private var rowButton: some View {
        Button(action: play) {
            VStack(alignment: .leading, spacing: 6) {
                RowFace(row: row, dimmed: dimmed)
                if detailsLine {
                    Text(VersionWords.details(row))
                        .textFont(Theme.type.caption, relativeTo: .caption)
                        .foregroundStyle(Theme.Palette.muted)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .opacity(hovered || focused ? 1 : 0)
                        .animation(Theme.Motion.focus, value: hovered || focused)
                        .accessibilityHidden(true)
                }
            }
            .textFont(Theme.type.callout, relativeTo: .callout)
            .foregroundStyle(dimmed == nil ? Theme.Palette.text : Theme.Palette.muted)
            .frame(maxWidth: .infinity, alignment: .leading)
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
        .accessibilityLabel(VersionWords.rowSpoken(row, dimmed: dimmed?.text))
        // The details are read after the row (they show on focus or hover, which VoiceOver has not).
        .accessibilityValue(VersionWords.details(row))
        .accessibilityHint(act.hint)
        .accessibilityActions {
            if touch { Button(StreamChoice.words.page.details) { sheet = true } }
        }
        .accessibilityFocused($rowFocused)
    }
}

/// A row's quiet line with its icon when it warns or is dimmed.
private struct RowLine: View {
    let row: VersionRow
    let dimmed: (text: String, glyph: String)?

    var body: some View {
        let text = VersionWords.rowWords(row, dimmed: dimmed?.text)
        let tone = row.line.flatMap { StreamChoice.lineWords($0) }?.tone ?? .ok
        if let dimmed {
            Label(text, glyph: dimmed.glyph).labelStyle(.titleAndIcon).lineLimit(1)
        } else if row.onDevice == nil, tone == .warn || tone == .bad {
            Label { Text(text) } icon: { Glyph(tone.glyph).foregroundStyle(tone.color) }.lineLimit(1)
        } else {
            Text(text).foregroundStyle(Theme.Palette.muted).lineLimit(1)
        }
    }
}

/// iPhone: a row's details in a sheet, with its own Play. A sheet is modal for VoiceOver; closing it
/// puts VoiceOver back on the info button that opened it.
private struct DetailsSheet: View {
    let row: VersionRow
    let dimmed: (text: String, glyph: String)?
    let play: () -> Void
    @Environment(\.versionAct) private var act

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            RowFace(row: row, dimmed: dimmed, big: true)
                .textFont(Theme.type.callout, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.text)
            ForEach(VersionWords.detailLines(row, dimmed: dimmed?.text), id: \.self) { line in
                Text(line)
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Button(action: play) {
                Label(act.label, glyph: act.glyph).frame(maxWidth: .infinity)
            }
            .buttonStyle(PillButtonStyle(prominent: true))
            .padding(.top, 6)
            Spacer(minLength: 0)
        }
        .padding(Theme.Space.m)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.Palette.bg)
        .accessibilityAddTraits(.isModal)
        #if os(iOS)
        .presentationDetents([.medium, .large])
        #endif
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
        .background(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous).fill(Theme.Palette.surface))
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

/// What a version's action does here: play it, or keep it on this device (the download purpose).
enum VersionAct {
    case play, keep

    var label: String { self == .keep ? DownloadCopy.pickAction : "Play this version" }
    var glyph: String { self == .keep ? "arrow-down-to-line" : "play-fill" }
    var hint: String { self == .keep ? "Downloads this version" : "Plays this version" }
}

extension EnvironmentValues {
    @Entry var versionAct = VersionAct.play
}
