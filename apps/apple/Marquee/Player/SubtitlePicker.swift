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
    /// A tab was taken off the Mac: once its languages are built, the focus lands on its chosen row.
    @State private var landOnTab = false
    @Environment(\.metrics) private var metrics

    typealias Row = TrackPicker.Row

    /// What the languages are built from: the tracks, the line the picker sets and both lines' languages.
    private struct Inputs: Equatable {
        let subtitles: [SubtitleTrack]
        let file: [TrackPicker.FileTrack]
        let line: TrackPicker.Line
        let first: String?
        let second: String?
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
            .onChange(of: inputs, initial: true) { _, inputs in
                languages = TrackPicker.lineLanguages(
                    inputs.line, tracks: inputs.subtitles, file: inputs.file, first: inputs.first, second: inputs.second)
                if landOnTab {
                    landOnTab = false
                    setRow(chosenRow)
                }
            }
            .onAppear {
                setRow(kind == .subtitles ? chosenRow : audioChosen.map(Row.audio))
                #if os(macOS)
                panelFocused = true
                #endif
                #if DEBUG
                if kind == .subtitles, let tag = DebugHooks.pickerLanguage, let l = languages.first(where: { $0.language == tag }) { show(l) }
                #endif
            }
    }

    // MARK: the data

    private var inputs: Inputs {
        Inputs(
            subtitles: model.playback?.subtitles ?? [], file: model.fileTextTracks, line: line,
            first: model.firstOn ? model.firstLanguage : nil, second: model.secondOn ? model.secondLanguage : nil)
    }

    private var w: StreamChoiceWords.Player { StreamChoice.words.player }
    /// The First line / Second line switch (plans/dual-subtitles.md 5.1): while a first line is on that is not
    /// drawn as pictures.
    private var lines: Bool { kind == .subtitles && model.firstOn && !model.pictureFirst }
    /// The line the picker sets: the tab while the switch shows, else the first.
    private var line: TrackPicker.Line { lines ? model.pickerLine : .first }
    private var second: Bool { line == .second }

    private var shown: [TrackPicker.Language] { languages.filter { TrackPicker.matches($0, query) } }
    private var chosen: String? {
        second
            ? TrackPicker.chosen(languages, subtitle: model.spokenOn ? nil : model.secondSubtitle, fileIndex: nil)
            : TrackPicker.chosen(languages, subtitle: model.subtitle, fileIndex: model.textTrack)
    }
    /// The tab's chosen row: the Spoken row when it is the choice, else the chosen language, else Off.
    private var chosenRow: Row { second && model.spokenOn ? .spoken : chosen.map(Row.language) ?? .off }
    /// The other line's language never opens: none of its tracks can be picked here.
    private var openLanguage: TrackPicker.Language? { open.flatMap { tag in languages.first { $0.language == tag && !$0.disabled } } }
    /// The spoken language's tracks in the list; nil when none is listed.
    private var spokenLanguage: TrackPicker.Language? {
        model.playback?.spokenLanguage.flatMap { tag in languages.first { !$0.disabled && TrackPicker.sameLanguage($0.language, tag) } }
    }
    private var spokenDisabled: Bool { TrackPicker.sameLanguage(model.playback?.spokenLanguage, model.firstLanguage) }
    /// Beside "Spoken language": its name; nil while the list is still out with no language known (the pending
    /// mark); "not known for this video" once it settles with none.
    private var spokenOwn: String? {
        if spokenDisabled { return w.onFirstLine }
        if let tag = model.playback?.spokenLanguage { return TrackPicker.englishName(tag) }
        return model.playback?.subtitlesPending == true ? nil : w.spokenNotKnown
    }
    private var audioChosen: Int? { model.audioTrack }

    private var audio: [TrackPicker.FileTrack] {
        model.audioTracks.indices.map { TrackPicker.FileTrack(index: $0, name: model.audioTracks[$0], language: model.audioTrackLanguages[safe: $0] ?? nil) }
    }

    private var shownAudio: [TrackPicker.FileTrack] {
        guard TrackPicker.audioSearch(audio.count), !query.isEmpty else { return audio }
        return audio.filter { TrackPicker.matches(TrackPicker.audioLanguage($0), query) }
    }

    // MARK: the forms

    @ViewBuilder private var content: some View {
        #if os(tvOS)
        HStack(alignment: .top, spacing: Theme.Space.l) {
            firstLevel.frame(width: 640)
            if kind == .subtitles, let language = openLanguage ?? languages.first(where: { $0.language == chosen && !$0.disabled }) {
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
            if lines { lineSwitch }
            if kind == .subtitles || TrackPicker.audioSearch(audio.count) {
                searchField
            }
            ScrollViewReader { proxy in
                ScrollView(.vertical) {
                    LazyVStack(alignment: .leading, spacing: 2) {
                        if kind == .subtitles {
                            offRow
                            if second, query.trimmingCharacters(in: .whitespaces).isEmpty { spokenRow }
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

    /// First line / Second line: a segmented control on iOS; on the Mac one keyboard stop the panel moves on with
    /// Left and Right; on tvOS two focusable buttons, a remote's Left and Right between them.
    @ViewBuilder private var lineSwitch: some View {
        #if os(iOS)
        Picker("Subtitle line", selection: Binding(get: { line }, set: switchLine)) {
            Text(w.firstLine).tag(TrackPicker.Line.first)
            Text(w.secondLine).tag(TrackPicker.Line.second)
        }
        .pickerStyle(.segmented)
        .controlSize(.large)
        .frame(minHeight: Theme.minTarget)
        #else
        HStack(spacing: 2) {
            ForEach([TrackPicker.Line.first, .second], id: \.self) { tab in
                Button { switchLine(tab) } label: {
                    Text(tab == .first ? w.firstLine : w.secondLine)
                        .textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout)
                        .frame(maxWidth: .infinity, minHeight: 32)
                        .background(Capsule().fill(.white.opacity(line == tab ? 0.24 : 0)))
                        .contentShape(Capsule())
                }
                .buttonStyle(FocusPlainStyle(shape: Capsule()))
                .accessibilityAddTraits(line == tab ? .isSelected : [])
            }
        }
        .padding(2)
        .background(Capsule().fill(.white.opacity(0.12)))
        #if os(macOS)
        .overlay { if lit(.line) { Capsule().strokeBorder(Theme.Palette.key, lineWidth: Theme.Focus.ring) } }
        #endif
        .id(Row.line)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Subtitle line")
        #endif
    }

    private var offRow: some View {
        let on = second ? model.secondSubtitle == nil && !model.spokenOn : model.subtitle == nil && model.textTrack == nil
        return PickerRow(focused: lit(.off), selected: on) {
            Color.clear.frame(width: 28, height: 28)
            Text("Off").textFont(Theme.type.body, relativeTo: .body)
            Spacer(minLength: 8)
        } action: { run(.off) }
        .focused($focus, equals: .off)
        .id(Row.off)
        .accessibilityLabel(on ? "Off, selected" : "Off")
    }

    /// The Second line tab's Spoken language row: whatever language the audio is, its flag and name beside it;
    /// its chevron (Right on the Mac) opens that language's tracks.
    private var spokenRow: some View {
        let on = model.spokenOn
        let own = spokenOwn
        return HStack(spacing: 4) {
            PickerRow(focused: lit(.spoken), selected: on) {
                if let tag = model.playback?.spokenLanguage { FlagMark(tag: tag) } else { Color.clear.frame(width: 24, height: 24) }
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 8) {
                        Text(w.spokenLanguage).textFont(Theme.type.body, relativeTo: .body).lineLimit(1).fixedSize()
                        if let own {
                            Text(own).textFont(Theme.type.callout, relativeTo: .callout).foregroundStyle(Theme.Palette.muted).lineLimit(1).fixedSize()
                        } else {
                            ProgressView().controlSize(.small).tint(Theme.Palette.muted)
                        }
                    }
                    Text(w.spokenLanguage).textFont(Theme.type.body, relativeTo: .body).lineLimit(1)
                }
                Spacer(minLength: 8)
            } action: { run(.spoken) }
            .disabled(spokenDisabled)
            .opacity(spokenDisabled ? 0.6 : 1)
            .focused($focus, equals: .spoken)
            .accessibilityLabel([w.spokenLanguage, own ?? "still searching", on ? "selected" : nil].compactMap { $0 }.joined(separator: ", "))
            #if !os(tvOS)
            if let l = spokenLanguage, !spokenDisabled {
                Button { show(l) } label: {
                    Glyph("chevron-right", size: 15).foregroundStyle(Theme.Palette.muted).frame(width: 36, height: 40)
                }
                .buttonStyle(FocusPlainStyle(shape: Circle()))
                .accessibilityHidden(true)
            }
            #endif
        }
        .id(Row.spoken)
        #if os(tvOS)
        .onChange(of: focus) { _, now in if now == .spoken, let l = spokenLanguage { open = l.language } }
        #endif
    }

    /// Flag, English name, own name in quiet text, Auto for the core's start pick, the count, the check; its
    /// chevron (Right on the Mac) opens the options. The other line's language reads "On the first line" (or
    /// second) in place of its own name and cannot be picked or opened.
    private func languageRow(_ l: TrackPicker.Language) -> some View {
        let on = chosen == l.language
        let auto = on && !second && model.subtitleIsAuto
        let other = l.disabled ? (second ? w.onFirstLine : w.onSecondLine) : nil
        let dim = l.disabled ? 0.5 : 1
        return HStack(spacing: 4) {
            PickerRow(focused: lit(.language(l.language)), selected: on) {
                // The other line's language: its flag, name and count dim, the reason stays readable.
                FlagMark(tag: l.language).opacity(dim)
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 8) { name(l).opacity(dim); own(l, other) }
                    name(l).opacity(dim)
                }
                if auto { AutoMark() }
                Spacer(minLength: 8)
                Text("\(l.count)").monospacedDigit().foregroundStyle(Theme.Palette.dim)
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .opacity(dim)
            } action: { run(TrackPicker.press(.language(l.language), languages: languages, open: openLanguage)) }
            .disabled(l.disabled)
            .focused($focus, equals: .language(l.language))
            .accessibilityLabel(TrackPicker.spoken(l, auto: auto, selected: on, other: other))
            .accessibilityAction(named: "Options") { if !l.disabled { show(l) } }
            #if !os(tvOS)
            if !l.disabled {
                Button { show(l) } label: {
                    Glyph("chevron-right", size: 15).foregroundStyle(Theme.Palette.muted).frame(width: 36, height: 40)
                }
                .buttonStyle(FocusPlainStyle(shape: Circle()))
                .focused($focus, equals: .opener(l.language))
                // One stop per language for a reader: the row carries Options as an action.
                .accessibilityHidden(true)
            }
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

    @ViewBuilder private func own(_ l: TrackPicker.Language, _ other: String?) -> some View {
        if let own = other ?? l.own {
            Text(own).textFont(Theme.type.callout, relativeTo: .callout).foregroundStyle(Theme.Palette.muted).lineLimit(1).fixedSize()
        }
    }

    /// One audio track: its flag, its name, its own name in quiet text as a subtitle language has, Default for
    /// the file's own default, the check.
    private func audioRow(_ t: TrackPicker.FileTrack) -> some View {
        let on = model.audioTrack == t.index
        // With one track there is nothing to tell apart.
        let isDefault = TrackPicker.marksDefault(index: t.index, defaultIndex: model.audioDefault, count: audio.count)
        let own = TrackPicker.audioOwnName(t)
        return PickerRow(focused: lit(.audio(t.index)), selected: on) {
            // No "?" for a track that names no language: a blank in the flag column, or no column at all.
            if TrackPicker.audioFlags(audio) {
                if let tag = TrackPicker.tag(t.language) { FlagMark(tag: TrackPicker.fileTag(tag)) } else { Color.clear.frame(width: 24, height: 24) }
            }
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 8) {
                    Text(t.name).textFont(Theme.type.body, relativeTo: .body).lineLimit(1).fixedSize()
                    if let own {
                        Text(own).textFont(Theme.type.callout, relativeTo: .callout).foregroundStyle(Theme.Palette.muted).lineLimit(1).fixedSize()
                    }
                }
                Text(t.name).textFont(Theme.type.body, relativeTo: .body).lineLimit(1)
            }
            if isDefault { AutoMark(text: "Default") }
            Spacer(minLength: 8)
        } action: { run(.audio(t.index)) }
        .focused($focus, equals: .audio(t.index))
        .id(Row.audio(t.index))
        .accessibilityLabel([t.name, own, isDefault ? "Default" : nil, on ? "selected" : nil].compactMap { $0 }.joined(separator: ", "))
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
            // The tuning foot scrolls with the options: a landscape phone cannot hold its rows fixed.
            ScrollView(.vertical) {
                VStack(alignment: .leading, spacing: 12) {
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
                            optionRow(
                                id: i, title: SubtitleMenu.title(t), detail: SubtitleMenu.detail(t, max: 40),
                                selected: second ? model.secondSubtitle == t : model.subtitle == t)
                        }
                    }
                    // The tuning foot: the line's timing while its track is on (the second's also when the Spoken
                    // row picked it); on the Second line tab also the profile's Layout and Only when paused, which
                    // redraw the lines at once.
                    if second {
                        if let url = model.secondSubtitle?.url, l.tracks.contains(where: { $0.url == url }) {
                            timing({ model.secondDelay }, track: url) { model.setSecondDelay($0, spoken: $1) }
                        }
                        layoutRow
                        Toggle(w.onlyWhenPaused, isOn: Binding(get: { model.secondOnPause }, set: { model.setSecondOnPause($0) }))
                            .textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout)
                            .frame(minHeight: Theme.minTarget)
                    } else if let key = model.timingKey, chosen == l.language {
                        timing({ model.subtitleDelay }, track: key) { model.setSubtitleDelay($0, spoken: $1) }
                    }
                }
            }
            .scrollIndicators(.hidden)
        }
    }

    /// A line's Timing row. `value` is read at each step, so a hold moves on from where the last step left it.
    @ViewBuilder private func timing(_ value: @escaping () -> Double, track: String, set: @escaping (Double, Bool) -> Void) -> some View {
        #if os(tvOS)
        // A remote: the three steps as buttons, as the old menu had them.
        HStack(spacing: Theme.Space.s) {
            Text("Timing \(PlayerLayout.delayText(value()))").monospacedDigit()
            Button { set(PlayerLayout.nudge(value(), by: -PlayerLayout.subtitleStep), true) } label: {
                Label("\u{2212}0.1 s", glyph: "minus")
            }
            Button { set(PlayerLayout.nudge(value(), by: PlayerLayout.subtitleStep), true) } label: {
                Label("+0.1 s", glyph: "plus")
            }
            if value() != 0 {
                Button { set(0, true) } label: { Label("Reset", glyph: "rotate-ccw") }
            }
        }
        .buttonStyle(PillButtonStyle())
        #else
        // Its keys only when it is focused on purpose (a click, Tab): the panel's rows keep the arrows.
        SubtitleTimingStepper(value: value(), track: track, takesFocus: false) { by, spoken in
            set(PlayerLayout.nudge(value(), by: by), spoken)
        } reset: {
            set(0, true)
        }
        #endif
    }

    /// Layout: Stacked, Split, One box, stepped as a value in words (plans/dual-subtitles.md 5.1).
    private var layoutRow: some View {
        let all = SubtitleLayout.allCases
        let at = all.firstIndex(of: model.subtitleLayout) ?? 0
        let step = { (by: Int) in if all.indices.contains(at + by) { model.setSubtitleLayout(all[at + by]) } }
        return HStack(spacing: 12) {
            Text(w.layout).textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout)
            Spacer(minLength: 16)
            layoutStep("minus", enabled: at > 0) { step(-1) }
            Text(w.layoutName(all[at])).textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout).frame(minWidth: 72)
            layoutStep("plus", enabled: at < all.count - 1) { step(1) }
        }
        // One element for a reader: its name, its value, and a swipe to step.
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(w.layout)
        .accessibilityValue(w.layoutName(all[at]))
        .accessibilityAdjustableAction { direction in step(direction == .increment ? 1 : -1) }
    }

    private func layoutStep(_ glyph: String, enabled: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Glyph(glyph, size: 16)
                .frame(width: 40, height: 40)
                .background(.white.opacity(0.12), in: Circle())
                .frame(width: Theme.minTarget, height: Theme.minTarget)
                .contentShape(Circle())
        }
        .buttonStyle(FocusPlainStyle(shape: Circle()))
        .disabled(!enabled)
        .opacity(enabled ? 1 : 0.4)
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
            ForEach(SubtitleNote.lines(notes), id: \.self) { line in
                Text(line).textFont(Theme.type.caption, relativeTo: .caption).foregroundStyle(Theme.Palette.muted).padding(10)
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
        case .off: if second { model.chooseSecondSubtitle(nil) } else { model.chooseSubtitle(nil) }
        case .turnOn(.file(let index)): model.selectText(index)
        case .turnOn(.track(let track)): if second { model.chooseSecondSubtitle(track) } else { model.chooseSubtitle(track) }
        case .audio(let index): model.selectAudio(index)
        case .line(let tab): switchLine(tab)
        case .spoken:
            // The spoken language's best track; with none here the profile keeps the choice for the next titles.
            // Never while it is the first line's language (a Mac's Return reaches a disabled row).
            guard !spokenDisabled else { break }
            if let best = spokenLanguage?.tracks.first { model.chooseSecondSubtitle(best, spoken: true) } else { model.keepSpoken() }
        case .none: break
        }
    }

    /// Another tab: its languages, landing on its chosen row, else Off (a remote's focus goes there; the Mac's
    /// keyboard stays on the switch).
    private func switchLine(_ tab: TrackPicker.Line) {
        guard tab != line else { return }
        model.pickerLine = tab
        open = nil
        #if os(macOS)
        setRow(.line)
        #else
        landOnTab = true
        #endif
    }

    /// A language's options, landing on the option playing when it is the chosen language, else its best.
    private func show(_ l: TrackPicker.Language) {
        open = l.language
        setRow(second
            ? TrackPicker.landing(l, subtitle: model.secondSubtitle, fileIndex: nil)
            : TrackPicker.landing(l, subtitle: model.subtitle, fileIndex: model.textTrack))
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
        let order = TrackPicker.order(
            audio: kind == .audio ? shownAudio : nil, open: openLanguage, shown: shown, lines: lines,
            spoken: second && query.trimmingCharacters(in: .whitespaces).isEmpty)
        let act = TrackPicker.key(
            key, focus: focus ?? current, order: order, languages: languages, open: openLanguage,
            spoken: spokenDisabled ? nil : spokenLanguage?.language)
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

/// A language's circle flag (circle-flags), or a neutral round mark with the language glyph or its two letters;
/// never an emoji.
struct FlagMark: View {
    let tag: String

    var body: some View {
        Group {
            if let name = TrackPicker.flag(tag) {
                Image(name).resizable().interpolation(.high)
            } else if TrackPicker.glyph(tag) {
                // Fixed, not Dynamic Type: it sits in the 24 pt circle.
                Image("languages").renderingMode(.template).resizable().scaledToFit()
                    .frame(width: 13, height: 13)
                    .foregroundStyle(Theme.Palette.text)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(Circle().fill(.white.opacity(0.18)))
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
