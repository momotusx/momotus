import SwiftUI

// The player bar's strips and its quick shift (plans/player-bar.md section 5; design/exploration/player-controls/
// round3.html, variant 1 "Toolbar strip"). tvOS keeps its full pickers: none of this is built there.

#if !os(tvOS)
private var bar: StreamChoiceWords.Player.Bar { StreamChoice.words.player.bar }

/// The 16 percent white of the bar's pills and chips; a checked chip takes the selected fill and a light inner edge.
private let pillFill = Color.white.opacity(0.16)

/// A strip's chip: a pill of text (and a mark), at least a target tall; `checked` marks the one on.
struct StripChip<Mark: View>: View {
    let title: String
    var checked = false
    var ghost = false
    @ViewBuilder var mark: Mark
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                mark
                Text(title).textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout).lineLimit(1).fixedSize()
            }
            .padding(.horizontal, 14)
            .frame(minWidth: Theme.minTarget, minHeight: Theme.minTarget)
            .background {
                Capsule().fill(checked ? Theme.Palette.selectedFill : ghost ? .clear : pillFill)
                if checked { Capsule().strokeBorder(.white.opacity(0.7), lineWidth: 2) }
            }
            .contentShape(Capsule())
        }
        .buttonStyle(FocusPlainStyle(shape: Capsule()))
        .accessibilityAddTraits(checked ? .isSelected : [])
    }
}

extension StripChip where Mark == EmptyView {
    init(title: String, checked: Bool = false, ghost: Bool = false, action: @escaping () -> Void) {
        self.init(title: title, checked: checked, ghost: ghost, mark: { EmptyView() }, action: action)
    }
}

/// A strip's label ("Subtitles", "Audio") and the thin line between its parts.
@MainActor private func stripLabel(_ glyph: String, _ title: String) -> some View {
    HStack(spacing: 8) {
        Glyph(glyph, size: 18)
        Text(title).textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout).fixedSize()
    }
    .foregroundStyle(Theme.Palette.muted)
    .padding(.horizontal, 6)
    .accessibilityHidden(true)
}

@MainActor private var divider: some View {
    Rectangle().fill(.white.opacity(0.24)).frame(width: 1, height: 26).accessibilityHidden(true)
}

/// One strip row: its parts in a glass capsule, scrolling sideways when the bar is narrower than it.
private struct StripRow<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        let row = HStack(spacing: 8) { content }.padding(6)
        ViewThatFits(in: .horizontal) {
            row.fixedSize()
            ScrollView(.horizontal) { row }.scrollIndicators(.hidden)
        }
        .glass(Capsule(), strong: true)
    }
}

/// The quick shift (D2, D11, D12): Earlier, the value and Later in one capsule beside Subtitles. A tap moves 0.25 s; a
/// hold repeats by `HoldRule.quick`; the value resets to 0. On the Mac each part takes the keyboard with Full
/// Keyboard Access (Space and Return held); VoiceOver adjusts the value. `chevrons`: Earlier and Later as arrows
/// (D16 b, D17 a), their names unchanged.
struct QuickShift: View {
    let value: Double
    /// The track the value is for: a new one ends a hold under way.
    let track: String
    var chevrons = false
    /// Its words' size before Dynamic Type (the iPhone's is smaller), as `BarFit` measures them.
    var textSize = Theme.type.callout
    /// A step by the signed amount; `spoken` false for VoiceOver's own adjust, which it reads itself.
    let move: (Double, _ spoken: Bool) -> Void
    /// Pressed or let go: the controls stay up while a hold runs, and their timer starts again after it.
    var holding: (Bool) -> Void = { _ in }
    @State private var hold: TimingHold?
    @State private var pressing = 0
    @GestureState private var touchingEarlier = false
    @GestureState private var touchingLater = false
    @FocusState private var focus: Int?
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        HStack(spacing: 0) {
            side(-1)
            valueView
            side(1)
        }
        .background(Capsule().fill(pillFill))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(bar.names.timing)
        .onAppear { hold = TimingHold(rule: .quick) { move($0, true) } }
        .onDisappear { stop(single: false) }
        .onChange(of: track) { stop(single: false) }
        .onChange(of: scenePhase) { _, phase in if phase != .active { stop(single: false) } }
        .onChange(of: focus) { old, _ in if old == pressing, pressing != 0 { stop(single: false) } }
        // A touch the system took back (a banner, Control Center) resets the gesture with no end.
        .onChange(of: touchingEarlier) { _, now in if !now { touchEnded(-1) } }
        .onChange(of: touchingLater) { _, now in if !now { touchEnded(1) } }
    }

    private var valueView: some View {
        Text(PlayerLayout.delayText(value))
            .textFont(textSize, weight: .semibold, relativeTo: .callout)
            .monospacedDigit()
            .frame(minWidth: BarFit.shiftValue, minHeight: Theme.minTarget)
            .contentShape(Capsule())
            .onTapGesture(perform: reset)
            .focusable(interactions: .activate)
            .focusEffectDisabled()
            .focused($focus, equals: 0)
            .modifier(FocusRing(shape: Capsule(), active: focus == 0))
            .onKeyPress(keys: [.space, .return], phases: .down) { press in
                guard PlayerLayout.stepperKey(press.modifiers) else { return .ignored }
                reset()
                return .handled
            }
            .accessibilityElement()
            .accessibilityLabel(PlayerLayout.shiftValueName(value))
            .accessibilityAddTraits(.isButton)
            .accessibilityAction(.default, reset)
            .accessibilityAdjustableAction { direction in
                move(direction == .increment ? PlayerLayout.quickStep : -PlayerLayout.quickStep, false)
            }
    }

    /// Nothing happens at 0.
    private func reset() {
        guard value != 0 else { return }
        move(-value, true)
    }

    /// Earlier (-1) or Later (1): a press and hold by the shift's rule, not the system's repeat.
    private func side(_ direction: Int) -> some View {
        Group {
            if chevrons {
                Glyph(direction < 0 ? "chevron-left" : "chevron-right", size: 18)
            } else {
                Text(direction < 0 ? bar.earlier : bar.later)
                    .textFont(textSize, weight: .semibold, relativeTo: .callout)
                    .fixedSize()
                    .padding(.horizontal, BarFit.shiftPad)
            }
        }
        .frame(minWidth: Theme.minTarget, minHeight: Theme.minTarget)
        .background(Capsule().fill(.white.opacity(pressing == direction ? 0.16 : 0)))
        .contentShape(Capsule())
        .gesture(
            DragGesture(minimumDistance: 0)
                .updating(direction < 0 ? $touchingEarlier : $touchingLater) { _, state, _ in state = true }
                .onChanged { _ in if pressing != direction { start(direction) } }
                .onEnded { _ in if pressing == direction { stop(single: true) } }
        )
        .focusable(interactions: .activate)
        .focusEffectDisabled()
        .focused($focus, equals: direction)
        .modifier(FocusRing(shape: Capsule(), active: focus == direction))
        .onKeyPress(keys: [.space, .return], phases: [.down, .repeat, .up]) { press in
            guard PlayerLayout.stepperTakes(down: press.phase == .down, modifiers: press.modifiers) else { return .ignored }
            switch press.phase {
            case .down: start(direction)
            case .up: if pressing == direction { stop(single: true) }
            default: break
            }
            return .handled
        }
        .accessibilityElement()
        .accessibilityLabel(direction < 0 ? bar.names.earlier : bar.names.later)
        .accessibilityAddTraits(.isButton)
        .accessibilityAction { move(Double(direction) * PlayerLayout.quickStep, true) }
    }

    private func start(_ direction: Int) {
        pressing = direction
        holding(true)
        hold?.start(direction)
    }

    private func touchEnded(_ direction: Int) {
        Task { @MainActor in if pressing == direction { stop(single: false) } }
    }

    private func stop(single: Bool) {
        let was = pressing
        pressing = 0
        hold?.stop(single: single)
        if was != 0 { holding(false) }
    }
}

/// The subtitle strip (section 5): right-aligned over the bar's right end. The label, the language chips (D5),
/// More languages, the shift while a timeable subtitle is on, Style and Close; Style adds a row above it with
/// Size, Position and Background, each written to the profile on its press.
struct SubtitleStrip: View {
    let model: PlayerModel
    let phone: Bool
    /// An upright phone: Position does nothing there (D7) and says so.
    let upright: Bool
    let chevrons: Bool
    let moreLanguages: () -> Void
    let close: () -> Void
    var holding: (Bool) -> Void = { _ in }
    @FocusState private var focus: String?

    var body: some View {
        VStack(alignment: .trailing, spacing: 10) {
            if model.styleOpen { StripRow { styleRow } }
            StripRow { mainRow }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(bar.names.subtitles)
        .onAppear { if KeyboardNavigation.on { focus = checked ?? "off" } }
    }

    private var languages: [TrackPicker.Language] {
        TrackPicker.lineLanguages(
            .first, tracks: model.playback?.subtitles ?? [], file: model.fileTextTracks, first: nil,
            second: model.secondOn ? model.secondLanguage : nil)
    }

    /// The language on, nil for Off.
    private var checked: String? { TrackPicker.chosen(languages, subtitle: model.subtitle, fileIndex: model.textTrack) }

    @ViewBuilder private var mainRow: some View {
        stripLabel("captions", bar.names.subtitles)
        let all = languages
        let on = checked
        HStack(spacing: 8) {
            StripChip(title: "Off", checked: on == nil) { model.chooseSubtitle(nil) }
                .focused($focus, equals: "off")
            ForEach(BarRules.chips(all, on: on, preferred: model.subtitleLanguages, cap: BarRules.chipCap(phone: phone))) { l in
                StripChip(title: l.name, checked: on == l.language) {
                    FlagMark(tag: l.language)
                } action: {
                    switch TrackPicker.press(.language(l.language), languages: all, open: nil) {
                    case .turnOn(.file(let index)): model.selectText(index)
                    case .turnOn(.track(let track)): model.chooseSubtitle(track)
                    default: break
                    }
                }
                .focused($focus, equals: l.language)
            }
            if all.isEmpty, model.playback?.subtitlesPending == true || model.playback?.phase != "ready" {
                Text("Searching").textFont(Theme.type.callout, relativeTo: .callout).foregroundStyle(Theme.Palette.muted).fixedSize()
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(bar.names.subtitleLanguage)
        StripChip(title: bar.moreLanguages, ghost: true, mark: { EmptyView() }, action: moreLanguages)
        if let key = model.timingKey {
            divider
            QuickShift(value: model.subtitleDelay, track: key, chevrons: chevrons, textSize: phone ? 14 : Theme.type.callout,
                       move: { by, spoken in model.shiftSubtitles(by: by, spoken: spoken) }, holding: holding)
        }
        divider
        // Nothing in Style applies to a subtitle drawn as pictures.
        if !model.pictureFirst {
            StripChip(title: bar.style, checked: model.styleOpen) {
                Glyph("text-align-start", size: 16)
            } action: {
                withAnimation(Theme.Motion.panel) { model.styleOpen.toggle() }
            }
            .accessibilityAddTraits(.isButton)
            .accessibilityValue(model.styleOpen ? "Expanded" : "Collapsed")
        }
        closeButton(close)
    }

    @ViewBuilder private var styleRow: some View {
        let size = model.subtitleSize
        stepper(bar.size, value: BarRules.sizeText(size), down: ("minus", bar.names.smaller), up: ("plus", bar.names.larger),
                lower: BarRules.sizeStep(size, by: -1).map { s in { model.setSubtitleSize(s) } },
                higher: BarRules.sizeStep(size, by: 1).map { s in { model.setSubtitleSize(s) } })
        divider
        let position = model.subtitlePosition
        stepper(bar.position, value: BarRules.positionText(position), down: ("chevron-down", bar.lower), up: ("chevron-up", bar.higher),
                lower: BarRules.positionStep(position, by: -1).map { p in { model.setSubtitlePosition(p) } },
                higher: BarRules.positionStep(position, by: 1).map { p in { model.setSubtitlePosition(p) } })
        if upright { note(bar.sidewaysOnly) }
        divider
        Text(bar.background).textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout).foregroundStyle(Theme.Palette.muted).fixedSize()
            .accessibilityHidden(true)
        // The viewer's caption settings decide it (D8): the buttons wait, with the reason.
        let insists = CaptionStyle.systemInsists
        HStack(spacing: 4) {
            ForEach(SubtitleBackground.allCases, id: \.self) { b in
                StripChip(title: bar.backgroundName(b), checked: !insists && model.subtitleBackground == b) { model.setSubtitleBackground(b) }
                    .disabled(insists)
            }
        }
        .opacity(insists ? 0.5 : 1)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(bar.background)
        if insists { note(bar.setByDevice) }
    }

    private func note(_ text: String) -> some View {
        Text(text).textFont(Theme.type.caption, relativeTo: .caption).foregroundStyle(Theme.Palette.muted).fixedSize()
    }

    /// Size or Position: its name, a step down, its value, a step up; one adjustable element for a reader.
    private func stepper(
        _ name: String, value: String, down: (String, String), up: (String, String), lower: (() -> Void)?, higher: (() -> Void)?
    ) -> some View {
        HStack(spacing: 4) {
            Text(name).textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout).foregroundStyle(Theme.Palette.muted)
                .fixedSize().padding(.trailing, 4)
            stepButton(down, lower)
            Text(value).textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout).monospacedDigit()
                .frame(minWidth: 64).fixedSize()
            stepButton(up, higher)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(name)
        .accessibilityValue(value)
        .accessibilityAdjustableAction { direction in (direction == .increment ? higher : lower)?() }
    }

    private func stepButton(_ glyph: (name: String, label: String), _ action: (() -> Void)?) -> some View {
        Button { action?() } label: {
            Glyph(glyph.name, size: 18)
                .frame(width: Theme.minTarget, height: Theme.minTarget)
                .background(Circle().fill(pillFill))
                .contentShape(Circle())
        }
        .buttonStyle(FocusPlainStyle(shape: Circle()))
        .disabled(action == nil)
        .opacity(action == nil ? 0.4 : 1)
        .accessibilityLabel(glyph.label)
    }
}

/// The audio strip (section 5): left-aligned over the bar's left end, one chip per track; with one track or none
/// listed, that one and Choose a version.
struct AudioStrip: View {
    let model: PlayerModel
    let chooseVersion: (() -> Void)?
    let close: () -> Void
    @FocusState private var focus: Int?

    var body: some View {
        StripRow {
            stripLabel("audio-lines", bar.names.audio)
            HStack(spacing: 8) {
                ForEach(model.audioTracks.indices, id: \.self) { index in
                    StripChip(title: model.audioTracks[index], checked: model.audioTrack == index) {
                        Glyph("audio-lines", size: 16)
                    } action: { model.selectAudio(index) }
                    .focused($focus, equals: index)
                }
            }
            .accessibilityElement(children: .contain)
            .accessibilityLabel(bar.names.audioTrack)
            if model.audioTracks.count <= 1, let chooseVersion {
                StripChip(title: PlayerLayout.failureLabel(.versions, offer: nil), ghost: true, mark: { EmptyView() }, action: chooseVersion)
            }
            closeButton(close)
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(bar.names.audio)
        .onAppear { if KeyboardNavigation.on { focus = model.audioTrack ?? 0 } }
    }
}

@MainActor private func closeButton(_ close: @escaping () -> Void) -> some View {
    Button(action: close) {
        Glyph("x", size: 18).frame(width: Theme.minTarget, height: Theme.minTarget).contentShape(Circle())
    }
    .buttonStyle(FocusPlainStyle(shape: Circle()))
    .accessibilityLabel(bar.close)
}
#endif
