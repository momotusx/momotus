import SwiftUI

/// Settings, Stream choice (plans/m13-stream-choice.md 4.4): how Marquee picks
/// the version to play. Every choice is one `setPrefs` leaf; what the picks
/// taught is listed with Undo and Forget; "This device" says what the
/// ranking reads here, with Measure again once a speed was measured.
struct StreamChoiceScreen: View {
    let core: CoreClient
    @State private var prefs: LiveRecord<Prefs>
    @State private var conditions: StreamConditions?
    /// Measure again was pressed: it stays, spent, so focus is not lost.
    @State private var measuring = false
    @State private var problem: String?
    /// TV and keyboard: the first choice, and where focus goes when a learned row leaves.
    @FocusState private var focus: String?
    @Environment(\.metrics) private var metrics
    @Environment(DebridSession.self) private var debrid

    init(core: CoreClient) {
        self.core = core
        _prefs = State(initialValue: LiveRecord("settings", core: core))
    }

    private var w: StreamChoiceWords.Settings { StreamChoice.words.settings }
    private var hasAccount: Bool { (debrid.view.snapshot?.accounts.count ?? 0) > 0 }

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: w.title)
                Text(w.lede)
                    .textFont(Theme.type.body)
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
                if let failure = problem ?? prefs.failure {
                    StateLine(symbol: "triangle-alert", text: failure)
                }
                if let p = prefs.snapshot {
                    VStack(alignment: .leading, spacing: 0) { rows(p) }
                        .padding(.horizontal, Theme.Space.m)
                        .glass(RoundedRectangle(cornerRadius: Theme.Radius.l, style: .continuous))
                }
            }
            .frame(maxWidth: DefaultsRules.column(wide: metrics.wide, tv: onTV) ?? .infinity, alignment: .leading)
            .frame(maxWidth: .infinity)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .topScrim()
        .defaultFocus($focus, "size")
        .task(id: prefs.taskId) { prefs.start() }
        .task { conditions = try? await core.call("streamConditions", as: StreamConditions.self) }
    }

    @ViewBuilder private func rows(_ p: Prefs) -> some View {
        let s = p.streams
        ChoiceRow(label: w.size.label, items: w.size.choices.map { ($0.value, $0.label) },
                  isOn: { $0 == s.sizeLimitBytes }, focus: $focus, id: "size") { patch(["sizeLimitBytes": $0.map { $0 as any Sendable } ?? NSNull()]) }
        divider
        ChoiceRow(label: w.seeders.label, help: w.seeders.help, items: w.seeders.choices.compactMap { c in c.value.map { ($0, c.label) } },
                  isOn: { $0 == s.minSeeders }, focus: $focus, id: "seeders") { patch(["minSeeders": $0]) }
        divider
        ChoiceRow(label: w.resolution.label, items: w.resolution.choices.map { ($0.value, $0.label) },
                  isOn: { StreamChoice.resolutionPicked($0, maxHeight: s.maxHeight) }, focus: $focus, id: "height") {
            patch(["maxHeight": $0.map { $0 as any Sendable } ?? NSNull()])
        }
        divider
        ChoiceRow(label: w.hdr.label, items: w.hdr.choices.compactMap { c in c.value.map { ($0, c.label) } },
                  isOn: { $0 == s.hdr }, focus: $focus, id: "hdr") { patch(["hdr": $0]) }
        if hasAccount {
            divider
            ChoiceRow(label: w.ready.label, items: w.ready.choices.compactMap { c in c.value.map { ($0, c.label) } },
                      isOn: { $0 == s.preferReady }, focus: $focus, id: "ready") { patch(["preferReady": $0]) }
        }
        divider
        ChoiceRow(label: w.audio.label, help: w.audio.help, items: Language.items(chosen: s.languages, subtitle: false),
                  isOn: { s.languages.contains($0) }, focus: $focus, id: "audio") { toggleAudio($0) } more: {
            NavigationLink {
                LanguagePicker(title: "Audio languages", audio: true, core: core,
                               isOn: { prefs.snapshot?.streams.languages.contains($0) ?? false }, toggle: toggleAudio)
            } label: { Label("More languages", glyph: "search") }
            .buttonStyle(.textLink)
        }
        divider
        ChoiceRow(label: w.learn.label, items: w.learn.choices.compactMap { c in c.value.map { ($0, c.label) } },
                  isOn: { $0 == s.learn }, focus: $focus, id: "learn") { patch(["learn": $0]) } more: {
            if StreamChoice.anyLearned(p) { learned(p) }
        }
        divider
        device
    }

    /// What the picks taught, each with Undo, then Forget.
    private func learned(_ p: Prefs) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(StreamChoice.learnedRows(p), id: \.text) { row in
                HStack(spacing: 12) {
                    Text(row.text)
                        .textFont(Theme.type.callout, relativeTo: .callout)
                        .foregroundStyle(Theme.Palette.text)
                        .fixedSize(horizontal: false, vertical: true)
                    Button { learnOp("undoLearned", ["screenClass": row.screenClass, "choice": row.choice]) } label: {
                        Label(StreamChoice.words.learned.undo, glyph: "undo-2")
                    }
                    .buttonStyle(PillButtonStyle())
                    .accessibilityLabel("\(StreamChoice.words.learned.undo): \(row.text)")
                }
            }
            Button { learnOp("forgetLearned", [:]) } label: { Label(w.learn.forget, glyph: "rotate-ccw") }
                .buttonStyle(PillButtonStyle())
        }
        .padding(.top, 4)
    }

    /// "This device": the screen, the connection and the disk, and Measure again.
    private var device: some View {
        LabelledRow(label: w.device.label) {
            VStack(alignment: .leading, spacing: 10) {
                Label(DeviceCaps.conditionsLine(conditions, account: hasAccount), glyph: deviceGlyph)
                    .textFont(Theme.type.body)
                    .foregroundStyle(Theme.Palette.text)
                    .fixedSize(horizontal: false, vertical: true)
                if measuring || conditions.map(StreamChoice.measured) == true {
                    HStack(spacing: 12) {
                        Button { Task { await measureAgain() } } label: { Label(w.device.measureAgain, glyph: "rotate-ccw") }
                            .buttonStyle(PillButtonStyle())
                            .disabled(measuring)
                        if measuring { StatusDot(tone: .idle, text: w.device.measuring) }
                    }
                }
            }
        }
    }

    private var deviceGlyph: String {
        #if os(tvOS)
        "tv"
        #elseif os(macOS)
        "monitor-check"
        #else
        "monitor-smartphone"
        #endif
    }

    private var divider: some View {
        Rectangle().fill(Theme.Palette.line).frame(height: 1)
    }

    /// Adds an audio language at the end, or takes it off.
    private func toggleAudio(_ code: String) {
        let list = prefs.snapshot?.streams.languages ?? []
        patch(["languages": list.contains(code) ? list.filter { $0 != code } : list + [code]])
    }

    private func patch(_ streams: [String: any Sendable]) {
        problem = nil
        Task { await prefs.perform("setPrefs", ["patch": ["streams": streams] as [String: any Sendable]]) }
    }

    /// Undo and Forget take their row away: focus goes to the Learn choice, never to Forget.
    private func learnOp(_ op: String, _ fields: [String: any Sendable]) {
        focus = "learn"
        problem = nil
        Task {
            do { try await core.perform(op, fields) } catch { problem = error.localizedDescription }
        }
    }

    private func measureAgain() async {
        do {
            conditions = try await core.call("resetSpeed", as: StreamConditions.self)
            measuring = true
        } catch {
            problem = error.localizedDescription
        }
    }
}

/// A row of the panel: its label (and help) beside the content on wide
/// screens, above it on a phone.
private struct LabelledRow<Content: View>: View {
    let label: String
    var help: String?
    @ViewBuilder let content: Content
    @Environment(\.metrics) private var metrics

    var body: some View {
        let layout = metrics.wide
            ? AnyLayout(HStackLayout(alignment: .top, spacing: Theme.Space.m))
            : AnyLayout(VStackLayout(alignment: .leading, spacing: 10))
        layout {
            VStack(alignment: .leading, spacing: 4) {
                Text(label)
                    .textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                    .foregroundStyle(Theme.Palette.text)
                    .accessibilityAddTraits(.isHeader)
                if let help {
                    Text(help)
                        .textFont(Theme.type.callout, relativeTo: .callout)
                        .foregroundStyle(Theme.Palette.muted)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .frame(width: metrics.wide ? 220 : nil, alignment: .leading)
            .frame(maxWidth: metrics.wide ? nil : .infinity, alignment: .leading)
            content.frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.vertical, Theme.Space.s + 2)
    }
}

/// One preference: its label and its choices as chips, the picked one white with a check.
private struct ChoiceRow<Value: Hashable, More: View>: View {
    let label: String
    var help: String?
    let items: [(Value, String)]
    let isOn: (Value) -> Bool
    let focus: FocusState<String?>.Binding
    let id: String
    let pick: (Value) -> Void
    @ViewBuilder var more: More

    init(label: String, help: String? = nil, items: [(Value, String)], isOn: @escaping (Value) -> Bool,
         focus: FocusState<String?>.Binding, id: String, pick: @escaping (Value) -> Void,
         @ViewBuilder more: () -> More = { EmptyView() }) {
        (self.label, self.help, self.items, self.isOn, self.focus, self.id, self.pick) = (label, help, items, isOn, focus, id, pick)
        self.more = more()
    }

    var body: some View {
        LabelledRow(label: label, help: help) {
            VStack(alignment: .leading, spacing: 12) {
                FlowRow(spacing: 10) {
                    ForEach(Array(items.enumerated()), id: \.offset) { index, item in
                        let on = isOn(item.0)
                        Button { pick(item.0) } label: {
                            HStack(spacing: 6) {
                                if on { Glyph("check", size: Theme.type.callout, relativeTo: .callout) }
                                Text(item.1)
                            }
                        }
                        .buttonStyle(ChipStyle(selected: on))
                        .accessibilityAddTraits(on ? .isSelected : [])
                        .focused(focus, equals: index == 0 ? id : "\(id).\(index)")
                    }
                }
                more
            }
        }
    }
}

#if os(tvOS)
private let onTV = true
#else
private let onTV = false
#endif
