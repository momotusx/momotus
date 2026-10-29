import SwiftUI

#if !os(tvOS)
/// A title's trailer full screen (plans/trailer-inline.md 7): YouTube's own player (the title page's `player`),
/// taking no input, under our controls, so nothing leaves for YouTube. The picture at 16:9, the position bar under
/// it, then the trailer's name with Pause or Play, Sound and Close. Its sound is its own, never the session's. A
/// trailer YouTube will not play here gives way to the next one, then to a quiet line; its end, or the app going to
/// the background, closes it. The Mac: Space pauses and plays, Left and Right seek 10 s, Escape closes.
struct FullTrailer: View {
    let trailers: [Trailer]
    let player: TrailerPlayer
    let close: () -> Void
    @State private var state = TrailerState()
    @State private var index = 0
    @State private var failed = false
    /// On at open (the viewer asked for it), except in a walk build, which plays every trailer silent.
    @State private var loud = !TrailerPlayer.walk
    /// Where a drag on the bar will land, until it is let go.
    @State private var held: Double?
    @State private var dragging = false
    /// The wait shows only past 300 ms, so a quick start never flashes it.
    @State private var waiting = false
    @FocusState private var focus: Control?
    @Environment(\.scenePhase) private var scenePhase

    private enum Control: Hashable { case page, toggle, close }
    private static let seekStep: Double = 10
    private static let failedLine = "This trailer can't play here."

    var body: some View {
        VStack(spacing: 16) {
            ZStack {
                Color.black
                if state.loading { TrailerSurface(player: player) }
                if failed {
                    Text(Self.failedLine)
                        .textFont(Theme.type.callout, relativeTo: .callout)
                        .foregroundStyle(Theme.Palette.muted)
                        .multilineTextAlignment(.center)
                        .padding(24)
                } else if waiting {
                    ProgressView().controlSize(.large).tint(.white).accessibilityLabel("Loading trailer")
                }
            }
            .aspectRatio(16 / 9, contentMode: .fit)
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            bar
            HStack(spacing: 8) {
                Text(trailers[safe: index]?.title ?? "")
                    .textFont(Theme.type.headline, weight: .bold, relativeTo: .headline)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                Button(action: player.toggle) {
                    Label(state.paused ? "Play" : "Pause", glyph: state.paused ? "play-fill" : "pause-fill")
                }
                .buttonStyle(PillButtonStyle())
                .disabled(failed)
                .focused($focus, equals: .toggle)
                Button {
                    loud.toggle()
                    player.loud(loud)
                } label: {
                    Label("Sound", glyph: loud ? "volume-2" : "volume-x")
                }
                .buttonStyle(PillButtonStyle())
                .disabled(failed)
                .accessibilityAddTraits(.isToggle)
                .accessibilityValue(loud ? "On" : "Off")
                Button(action: close) { Label("Close", glyph: "x") }
                    .buttonStyle(PillButtonStyle())
                    .focused($focus, equals: .close)
                    .keyboardShortcut(.cancelAction)
            }
        }
        .padding(16)
        .background(Theme.Palette.bg)
        // The page itself holds the keyboard's focus, so Space and the arrows work with Full Keyboard Access off.
        .focusable()
        .focusEffectDisabled()
        .focused($focus, equals: .page)
        .defaultFocus($focus, .page)
        // Once a press: a held key never flickers the trailer or runs it to its end.
        .onKeyPress(.space, phases: .down) { _ in
            player.toggle()
            return .handled
        }
        .onKeyPress(.leftArrow, phases: .down) { _ in seek(by: -Self.seekStep) }
        .onKeyPress(.rightArrow, phases: .down) { _ in seek(by: Self.seekStep) }
        .accessibilityAddTraits(.isModal)
        .task(id: index) { start() }
        .task(id: TrailerWait(buffering: state.buffering, failed: failed)) {
            waiting = false
            guard state.buffering, !failed else { return }
            try? await Task.sleep(for: .milliseconds(300))
            if !Task.isCancelled { waiting = true }
        }
        .onChange(of: failed) { _, now in
            guard now else { return }
            focus = .close
            AccessibilityNotification.Announcement(Self.failedLine).post()
        }
        .onChange(of: scenePhase) { _, phase in if phase == .background { close() } }
        .onDisappear { player.stop() }
    }

    /// Elapsed, the bar, remaining: the film player's Slider, named "Position".
    private var bar: some View {
        let position = held ?? state.seconds
        return HStack(spacing: 12) {
            Text(timecode(position)).monospacedDigit().accessibilityHidden(true)
            // A drag previews and seeks once let go; VoiceOver's adjust and the arrow keys seek at once.
            Slider(
                value: Binding(get: { held ?? state.seconds }, set: { value in
                    let move = trailerSliderMove(value, dragging: dragging)
                    held = move.held
                    if let to = move.seek { seek(to: to) }
                }),
                in: 0...max(1, state.length - 2)
            ) { editing in
                dragging = editing
                guard !editing, let to = held else { return }
                held = nil
                seek(to: to)
            }
            .disabled(failed || state.length <= 0)
            .accessibilityLabel("Position")
            .accessibilityValue("\(timecode(position)) of \(timecode(state.length))")
            Text("-" + timecode(max(0, state.length - position))).monospacedDigit().accessibilityHidden(true)
        }
        .textFont(Theme.type.callout, relativeTo: .callout)
        .foregroundStyle(Theme.Palette.muted)
    }

    private func start() {
        guard let trailer = trailers[safe: index] else {
            failed = true
            return
        }
        failed = false
        // One player at a time: Home's lets go first.
        TrailerPlayer.shared.release()
        player.play(trailer.youtubeId, into: state, cap: 0, full: true, loud: loud) { why in
            // Refused (embedding off, gone): the next trailer, then the line. Never ready (YouTube unreachable): the
            // line at once, not another 15 s per trailer.
            if why != "error", why != "timeout", why != "lost" {
                close()
            } else if why == "error", index + 1 < trailers.count {
                index += 1
            } else {
                failed = true
            }
        }
    }

    private func seek(by: Double) -> KeyPress.Result {
        guard state.length > 0 else { return .ignored }
        seek(to: state.seconds + by)
        return .handled
    }

    private func seek(to: Double) {
        let to = trailerSeekTarget(to, state.length, by: 0)
        state.seconds = to
        player.seek(to)
    }

    private func timecode(_ seconds: Double) -> String {
        clock(ms: Int64(max(0, seconds.isFinite ? seconds : 0) * 1000))
    }
}

private struct TrailerWait: Equatable {
    let buffering: Bool
    let failed: Bool
}
#endif
