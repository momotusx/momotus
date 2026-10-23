import SwiftUI

/// The subtitle and audio pickers (plans/player-polish.md 2.4, design/DECISION.md "Subtitle picker"), as the web's
/// subtitle-picker.tsx and Android TV's: the languages first, with a search field, Off and a flag each, then one
/// language's options and the timing row. The Mac and iPad step between the two levels in one strong-glass panel
/// over the picture (arrows, Return, Right to open, Left or Esc back); the iPhone shows it in a full-height sheet;
/// tvOS in two columns, focus moving right into the options. Picking a language turns on its best option at once.
struct TrackPickerPanel: View {
    enum Kind: Hashable, Identifiable {
        case subtitles, audio
        var id: Self { self }
    }

    let kind: Kind
    let model: PlayerModel
    let close: () -> Void
    @State private var query = ""
    /// The language whose options show (the second level; on tvOS the right column).
    @State private var open: String?
    @FocusState private var focus: Row?
    /// The keyboard's row on the Mac, where a plain button takes no focus without Full Keyboard Access: the panel
    /// holds the focus and moves this highlight (it mirrors `focus` elsewhere).
    @State private var current: Row?
    @FocusState private var panelFocused: Bool
    /// The languages, built once per change of the tracks, not on every pass (a search runs per keystroke).
    @State private var languages: [TrackPicker.Language] = []
    @Environment(\.metrics) private var metrics

    typealias Row = TrackPicker.Row

    /// What the languages are built from.
    private struct Inputs: Equatable {
        let subtitles: [SubtitleTrack]
        let file: [TrackPicker.FileTrack]
    }

    var body: some View {
        content
            .foregroundStyle(Theme.Palette.text)
            #if os(macOS)
            // The panel takes the keys, so Esc and the arrows are the picker's, never the player's under it.
            .focusable(interactions: .edit)
            .focusEffectDisabled()
            .focused($panelFocused)
            .onKeyPress(.downArrow) { key(.down) }
            .onKeyPress(.upArrow) { key(.up) }
            .onKeyPress(.rightArrow) { key(.right) }
            .onKeyPress(.leftArrow) { key(.left) }
            .onKeyPress(.escape) { key(.escape) }
            .onKeyPress(.return) { key(.enter) }
            #endif
            .onChange(of: Inputs(subtitles: model.playback?.subtitles ?? [], file: model.fileTextTracks), initial: true) { _, inputs in
                languages = TrackPicker.languages(inputs.subtitles, file: inputs.file)
            }
            .onAppear {
                setRow(kind == .subtitles ? (chosen.map(Row.language) ?? .off) : audioChosen.map(Row.audio))
                #if os(macOS)
                panelFocused = true
                #endif
                #if DEBUG
                if kind == .subtitles, let tag = DebugHooks.pickerLanguage, let l = languages.first(where: { $0.language == tag }) { show(l) }
                #endif
            }
    }

    // MARK: the data

    private var shown: [TrackPicker.Language] { languages.filter { TrackPicker.matches($0, query) } }
    private var chosen: String? { TrackPicker.chosen(languages, subtitle: model.subtitle, fileIndex: model.textTrack) }
    private var openLanguage: TrackPicker.Language? { open.flatMap { tag in languages.first { $0.language == tag } } }
    private var audioChosen: Int? { model.audioTrack }

    private var audio: [TrackPicker.FileTrack] {
        model.audioTracks.indices.map { TrackPicker.FileTrack(index: $0, name: model.audioTracks[$0], language: model.audioTrackLanguages[safe: $0] ?? nil) }
    }

    private var shownAudio: [TrackPicker.FileTrack] {
        guard TrackPicker.audioSearch(audio.count), !query.isEmpty else { return audio }
        return audio.filter { TrackPicker.matches(audioLanguage($0), query) }
    }

    private func audioLanguage(_ t: TrackPicker.FileTrack) -> TrackPicker.Language {
        let tag = t.language.map(TrackPicker.fileTag) ?? ""
        return TrackPicker.Language(language: tag, name: t.name, own: nil, codes: [tag.lowercased()], file: [t], tracks: [])
    }

    // MARK: the forms

    @ViewBuilder private var content: some View {
        #if os(tvOS)
        HStack(alignment: .top, spacing: Theme.Space.l) {
            firstLevel.frame(width: 640)
            if kind == .subtitles, let language = openLanguage ?? languages.first(where: { $0.language == chosen }) {
                options(language).frame(width: 640)
            }
        }
        .padding(Theme.Space.l)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.l, style: .continuous), strong: true)
        #else
        Group {
            if kind == .subtitles, let language = openLanguage {
                options(language)
            } else {
                firstLevel
            }
        }
        .padding(Theme.Space.m)
        #endif
    }

    private var title: String { kind == .subtitles ? "Subtitles" : "Audio" }

    private var firstLevel: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text(title).textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                    .accessibilityAddTraits(.isHeader)
                Spacer()
                #if os(iOS)
                Button("Done", action: close).textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout)
                #endif
            }
            if kind == .subtitles || TrackPicker.audioSearch(audio.count) {
                searchField
            }
            ScrollViewReader { proxy in
                ScrollView(.vertical) {
                    LazyVStack(alignment: .leading, spacing: 2) {
                        if kind == .subtitles {
                            offRow
                            ForEach(shown) { language in languageRow(language) }
                            if !query.trimmingCharacters(in: .whitespaces).isEmpty, shown.isEmpty {
                                Text("No language matches \u{201C}\(query.trimmingCharacters(in: .whitespaces))\u{201D}")
                                    .textFont(Theme.type.callout, relativeTo: .callout)
                                    .foregroundStyle(Theme.Palette.muted)
                                    .padding(10)
                            }
                            notes
                        } else {
                            ForEach(shownAudio, id: \.index) { track in audioRow(track) }
                        }
                    }
                }
                .scrollIndicators(.hidden)
                .onChange(of: current) { _, row in if let row { withAnimation(Theme.Motion.focus) { proxy.scrollTo(row) } } }
            }
        }
    }

    private var searchField: some View {
        HStack(spacing: 8) {
            Glyph("search", size: 15).foregroundStyle(Theme.Palette.dim)
            TextField("Search languages", text: $query)
                .textFieldStyle(.plain)
                .textFont(15, relativeTo: .body)
                .focused($focus, equals: .search)
                .autocorrectionDisabled()
            if !query.isEmpty {
                Button { query = "" } label: { Glyph("x", size: 13) }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Clear the search")
            }
        }
        .padding(.horizontal, 12)
        .frame(height: 36)
        .background(.white.opacity(0.12), in: Capsule())
    }

    private var offRow: some View {
        let on = model.subtitle == nil && model.textTrack == nil
        return PickerRow(focused: lit(.off), selected: on) {
            Color.clear.frame(width: 28, height: 28)
            Text("Off").textFont(Theme.type.body, relativeTo: .body)
            Spacer(minLength: 8)
        } action: { run(.off) }
        .focused($focus, equals: .off)
        .id(Row.off)
        .accessibilityLabel(on ? "Off, selected" : "Off")
    }

    /// Flag, English name, own name in quiet text, Auto for the core's start pick, the count, the check; its
    /// chevron (Right on the Mac) opens the options.
    private func languageRow(_ l: TrackPicker.Language) -> some View {
        let on = chosen == l.language
        let auto = on && model.subtitleIsAuto
        return HStack(spacing: 4) {
            PickerRow(focused: lit(.language(l.language)), selected: on) {
                FlagMark(tag: l.language)
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 8) { name(l); own(l) }
                    name(l)
                }
                if auto { AutoMark() }
                Spacer(minLength: 8)
                Text("\(l.count)").monospacedDigit().foregroundStyle(Theme.Palette.dim)
                    .textFont(Theme.type.callout, relativeTo: .callout)
            } action: { run(TrackPicker.press(.language(l.language), languages: languages, open: openLanguage)) }
            .focused($focus, equals: .language(l.language))
            .accessibilityLabel(TrackPicker.spoken(l, auto: auto, selected: on))
            .accessibilityAction(named: "Options") { show(l) }
            #if !os(tvOS)
            Button { show(l) } label: {
                Glyph("chevron-right", size: 15).foregroundStyle(Theme.Palette.muted).frame(width: 36, height: 40)
            }
            .buttonStyle(FocusPlainStyle(shape: Circle()))
            .focused($focus, equals: .opener(l.language))
            // One stop per language for a reader: the row carries Options as an action.
            .accessibilityHidden(true)
            #endif
        }
        .id(Row.language(l.language))
        #if os(tvOS)
        .onChange(of: focus) { _, now in if now == .language(l.language) { open = l.language } }
        #endif
    }

    private func name(_ l: TrackPicker.Language) -> some View {
        Text(l.name).textFont(Theme.type.body, relativeTo: .body).lineLimit(1).fixedSize()
    }

    @ViewBuilder private func own(_ l: TrackPicker.Language) -> some View {
        if let own = l.own {
            Text(own).textFont(Theme.type.callout, relativeTo: .callout).foregroundStyle(Theme.Palette.muted).lineLimit(1).fixedSize()
        }
    }

    /// One audio track: its flag, its name, Default for the file's own default, the check.
    private func audioRow(_ t: TrackPicker.FileTrack) -> some View {
        let on = model.audioTrack == t.index
        // With one track there is nothing to tell apart.
        let isDefault = TrackPicker.marksDefault(index: t.index, defaultIndex: model.audioDefault, count: audio.count)
        return PickerRow(focused: lit(.audio(t.index)), selected: on) {
            FlagMark(tag: t.language.map(TrackPicker.fileTag) ?? "")
            Text(t.name).textFont(Theme.type.body, relativeTo: .body).lineLimit(1)
            if isDefault { AutoMark(text: "Default") }
            Spacer(minLength: 8)
        } action: { run(.audio(t.index)) }
        .focused($focus, equals: .audio(t.index))
        .id(Row.audio(t.index))
        .accessibilityLabel([t.name, isDefault ? "Default" : nil, on ? "selected" : nil].compactMap { $0 }.joined(separator: ", "))
    }

    /// A language's options: the file's own tracks first, then the core's by what tells them apart, then the
    /// timing row while a track that takes one is on.
    private func options(_ l: TrackPicker.Language) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            #if !os(tvOS)
            Button { open = nil; setRow(.language(l.language)) } label: {
                HStack(spacing: 8) {
                    Glyph("chevron-left", size: 15)
                    FlagMark(tag: l.language)
                    Text(l.name).textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                }
            }
            .buttonStyle(FocusPlainStyle(shape: Capsule()))
            .focused($focus, equals: .back)
            .accessibilityLabel("Back to the languages")
            #endif
            ScrollView(.vertical) {
                LazyVStack(alignment: .leading, spacing: 2) {
                    if !l.file.isEmpty {
                        Text("In the file").textFont(Theme.type.caption, weight: .semibold, relativeTo: .caption)
                            .foregroundStyle(Theme.Palette.dim).padding(.horizontal, 10).padding(.top, 4)
                            .accessibilityAddTraits(.isHeader)
                        ForEach(l.file, id: \.index) { f in
                            optionRow(id: TrackPicker.fileRow + f.index, title: f.name, detail: nil, selected: model.textTrack == f.index)
                        }
                    }
                    ForEach(Array(l.tracks.enumerated()), id: \.element.url) { i, t in
                        optionRow(id: i, title: SubtitleMenu.title(t), detail: SubtitleMenu.detail(t, max: 40), selected: model.subtitle == t)
                    }
                }
            }
            .scrollIndicators(.hidden)
            if model.timingKey != nil, chosen == l.language {
                #if os(tvOS)
                // A remote: the three steps as buttons, as the old menu had them.
                HStack(spacing: Theme.Space.s) {
                    Text("Timing \(PlayerLayout.delayText(model.subtitleDelay))").monospacedDigit()
                    Button { model.setSubtitleDelay(PlayerLayout.nudge(model.subtitleDelay, by: -PlayerLayout.subtitleStep)) } label: {
                        Label("\u{2212}0.1 s", glyph: "minus")
                    }
                    Button { model.setSubtitleDelay(PlayerLayout.nudge(model.subtitleDelay, by: PlayerLayout.subtitleStep)) } label: {
                        Label("+0.1 s", glyph: "plus")
                    }
                    if model.subtitleDelay != 0 {
                        Button { model.setSubtitleDelay(0) } label: { Label("Reset", glyph: "rotate-ccw") }
                    }
                }
                .buttonStyle(PillButtonStyle())
                #else
                // Its keys only when it is focused on purpose (a click, Tab): the panel's rows keep the arrows.
                SubtitleTimingStepper(value: model.subtitleDelay, track: model.timingKey ?? "", takesFocus: false) { by, spoken in
                    model.setSubtitleDelay(PlayerLayout.nudge(model.subtitleDelay, by: by), spoken: spoken)
                } reset: {
                    model.setSubtitleDelay(0)
                }
                #endif
            }
        }
    }

    private func optionRow(id: Int, title: String, detail: String?, selected: Bool) -> some View {
        PickerRow(focused: lit(.option(id)), selected: selected) {
            VStack(alignment: .leading, spacing: 2) {
                Text(title).textFont(Theme.type.body, relativeTo: .body).lineLimit(2)
                if let detail {
                    Text(detail).textFont(Theme.type.caption, relativeTo: .caption).foregroundStyle(Theme.Palette.muted).lineLimit(2)
                }
            }
            Spacer(minLength: 8)
        } action: { run(TrackPicker.press(.option(id), languages: languages, open: openLanguage)) }
        .focused($focus, equals: .option(id))
        .id(Row.option(id))
        .accessibilityLabel([title, detail, selected ? "selected" : nil].compactMap { $0 }.joined(separator: ", "))
    }

    /// Why a source's files did not load, under the languages, for as long as the core says so.
    @ViewBuilder private var notes: some View {
        if let notes = model.playback?.notes, !notes.isEmpty {
            ForEach(notes, id: \.self) { note in
                Text(note.words).textFont(Theme.type.caption, relativeTo: .caption).foregroundStyle(Theme.Palette.muted).padding(10)
            }
        }
    }

    // MARK: the actions

    /// One press or key, as TrackPicker decides it.
    private func run(_ act: TrackPicker.Act) {
        switch act {
        case .focus(let row): setRow(row)
        case .open(let tag): if let l = languages.first(where: { $0.language == tag }) { show(l) }
        case .back:
            if let was = open {
                open = nil
                setRow(.language(was))
            }
        case .close: close()
        case .off: model.chooseSubtitle(nil)
        case .turnOn(.file(let index)): model.selectText(index)
        case .turnOn(.track(let track)): model.chooseSubtitle(track)
        case .audio(let index): model.selectAudio(index)
        case .none: break
        }
    }

    /// A language's options, landing on the option playing when it is the chosen language, else its best.
    private func show(_ l: TrackPicker.Language) {
        open = l.language
        setRow(TrackPicker.landing(l, subtitle: model.subtitle, fileIndex: model.textTrack))
    }

    /// The keyboard's row: the highlight, and the system focus where the platform gives buttons one.
    private func setRow(_ row: Row?) {
        current = row
        focus = row
        #if os(macOS)
        // Back from the search field to the rows, or into the other level: the panel takes the keys again, once
        // the level it now shows is laid out.
        if row != .search {
            panelFocused = true
            Task { @MainActor in panelFocused = true }
        }
        #endif
    }

    private func lit(_ row: Row) -> Bool { focus == row || current == row }

    #if os(macOS)
    private func key(_ key: TrackPicker.Key) -> KeyPress.Result {
        let order = TrackPicker.order(audio: kind == .audio ? shownAudio : nil, open: openLanguage, shown: shown)
        let act = TrackPicker.key(key, focus: focus ?? current, order: order, languages: languages, open: openLanguage)
        // Nothing for the rows (Left and Right in the search field): the key goes on to the field.
        if act == .none { return .ignored }
        run(act)
        return .handled
    }
    #endif
}

/// A picker row: the content, the check when it is the one on, a highlight when focused or under the pointer.
private struct PickerRow<Content: View>: View {
    let focused: Bool
    let selected: Bool
    @ViewBuilder let content: Content
    let action: () -> Void
    @State private var hovered = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 12) {
                content
                Glyph("check", size: 15).opacity(selected ? 1 : 0).accessibilityHidden(true)
            }
            .padding(.horizontal, 10)
            .frame(minHeight: 44)
            .contentShape(Rectangle())
            .background(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous).fill(.white.opacity(focused ? 0.16 : hovered ? 0.08 : 0)))
        }
        .buttonStyle(FocusPlainStyle(shape: RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous)))
        #if !os(tvOS)
        .onHover { hovered = $0 }
        #endif
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// A language's circle flag (circle-flags), or a neutral round mark with its two letters; never an emoji.
struct FlagMark: View {
    let tag: String

    var body: some View {
        Group {
            if let name = TrackPicker.flag(tag) {
                Image(name).resizable().interpolation(.high)
            } else {
                Text(TrackPicker.letters(tag))
                    .textFont(10, weight: .bold)
                    .foregroundStyle(Theme.Palette.text)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(Circle().fill(.white.opacity(0.18)))
            }
        }
        .frame(width: 24, height: 24)
        .clipShape(Circle())
        .accessibilityHidden(true)
    }
}

/// The quiet mark beside the language the core turned on by itself, or the file's default audio.
private struct AutoMark: View {
    var text = "Auto"

    var body: some View {
        Text(text)
            .textFont(Theme.type.caption, weight: .semibold, relativeTo: .caption)
            .foregroundStyle(Theme.Palette.muted)
            .padding(.horizontal, 6)
            .padding(.vertical, 1)
            .overlay(Capsule().stroke(Theme.Palette.muted, lineWidth: 1))
            .accessibilityHidden(true)
    }
}

extension Array {
    subscript(safe index: Int) -> Element? { indices.contains(index) ? self[index] : nil }
}
