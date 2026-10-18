import SwiftUI

/// The next episode near the end of this one: its still, its name, a
/// countdown when it starts on its own, Play now and Cancel.
struct UpNextCard: View {
    let model: PlayerModel
    /// A short window: the still goes, so the card takes less height.
    var short = false
    @Environment(\.metrics) private var metrics
    /// Play now takes focus when the card appears, so a remote can press it at once.
    @FocusState private var playFocused: Bool

    var body: some View {
        if let next = model.next {
            // The buttons sit under the still and the words, so neither squeezes the other.
            VStack(alignment: .leading, spacing: 12) {
                HStack(alignment: .top, spacing: Theme.Space.s) {
                    if !short {
                        ArtImage(url: next.thumbnail, maxPixels: 640, fallback: next.title)
                            .frame(width: metrics.wide ? 192 : 120, height: metrics.wide ? 108 : 68)
                            .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous))
                    }
                    VStack(alignment: .leading, spacing: 6) {
                        Text("NEXT EPISODE")
                            .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
                            .tracking(1.6)
                            .foregroundStyle(.white.opacity(0.7))
                        Text("S\(next.season):E\(next.episode) \(next.title)")
                            .display(Theme.type.headline + 2)
                            .foregroundStyle(.white)
                            .lineLimit(2)
                            .accessibilityLabel("\(spokenEpisode(season: next.season, episode: next.episode)), \(next.title)")
                        if model.startingNext {
                            Label("Finding a version", glyph: "clock")
                                .font(.text(Theme.type.callout, relativeTo: .callout))
                                .foregroundStyle(.white.opacity(0.8))
                        } else if model.autoplayNext {
                            Text(startsIn(seconds: model.countdown))
                                .font(.text(Theme.type.callout, relativeTo: .callout))
                                .monospacedDigit()
                                .foregroundStyle(.white.opacity(0.8))
                        }
                    }
                }
                // Side by side when they fit, else stacked with Play now first.
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 10) { buttons }
                    VStack(alignment: .leading, spacing: 10) { buttons }
                }
                .disabled(model.startingNext)
            }
            .padding(Theme.Space.s)
            .frame(maxWidth: metrics.wide ? 560 : 380, alignment: .leading)
            .glass(RoundedRectangle(cornerRadius: Theme.Radius.l, style: .continuous), strong: true, shadow: true)
            .accessibilityElement(children: .contain)
            .accessibilityLabel("Up next")
            .onAppear { playFocused = true }
        }
    }

    @ViewBuilder private var buttons: some View {
        Button { Task { await model.playNext() } } label: { Label("Play now", glyph: "skip-forward") }
            .buttonStyle(PillButtonStyle(prominent: true))
            .focused($playFocused)
        Button("Cancel", action: model.cancelNext)
            .buttonStyle(PillButtonStyle())
    }
}

/// Over the last frame when an item ends, or when the next episode has no
/// version: the way on, never a black screen with one line. Series offer
/// Next episode first; movies offer Back to the title first.
struct EndCard: View {
    let model: PlayerModel
    let back: () -> Void
    let chooseVersion: () -> Void
    @FocusState private var focused: Bool

    var body: some View {
        ZStack {
            Color.black.opacity(0.72).ignoresSafeArea()
            VStack(spacing: Theme.Space.s) {
                content
            }
            .multilineTextAlignment(.center)
            .foregroundStyle(.white)
            .padding(Theme.Space.l)
            .frame(maxWidth: 620)
        }
        .onAppear { focused = true }
    }

    private var title: String { model.route.target.title }

    @ViewBuilder private var content: some View {
        if let next = model.next, let failure = model.nextFailure {
            Glyph("circle-alert", size: 34).accessibilityHidden(true)
            heading("S\(next.season):E\(next.episode) \(next.title)")
            Text(failure)
                .font(.text(Theme.type.body))
                .foregroundStyle(.white.opacity(0.8))
            actions {
                Button("Choose a version", action: chooseVersion).buttonStyle(PillButtonStyle(prominent: true)).focused($focused)
                Button("Back", action: back).buttonStyle(PillButtonStyle())
            }
        } else if let next = model.next, model.startingNext {
            ProgressView().tint(.white).controlSize(.large)
            heading("Starting S\(next.season):E\(next.episode) \(next.title)")
            actions {
                Button("Back to \(title)", action: back).buttonStyle(PillButtonStyle()).focused($focused)
            }
        } else if let next = model.next {
            kicker("Next episode")
            heading("S\(next.season):E\(next.episode) \(next.title)")
            actions {
                Button { Task { await model.playNext() } } label: { Label("Next episode", glyph: "skip-forward") }
                    .buttonStyle(PillButtonStyle(prominent: true))
                    .focused($focused)
                Button { model.playAgain() } label: { Label("Play again", glyph: "rotate-ccw") }
                    .buttonStyle(PillButtonStyle())
                Button("Back to \(title)", action: back).buttonStyle(PillButtonStyle())
            }
        } else {
            kicker("Finished")
            heading(title)
            actions {
                Button("Back to \(title)", action: back).buttonStyle(PillButtonStyle(prominent: true)).focused($focused)
                Button { model.playAgain() } label: { Label("Play again", glyph: "rotate-ccw") }
                    .buttonStyle(PillButtonStyle())
            }
        }
    }

    private func kicker(_ text: String) -> some View {
        Text(caps: text)
            .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
            .tracking(1.6)
            .foregroundStyle(.white.opacity(0.7))
    }

    private func heading(_ text: String) -> some View {
        Text(text).display(Theme.type.title2).lineLimit(3)
    }

    /// Side by side when they fit, else stacked with the first on top.
    private func actions<Buttons: View>(@ViewBuilder _ buttons: () -> Buttons) -> some View {
        let buttons = buttons()
        return ViewThatFits(in: .horizontal) {
            HStack(spacing: 12) { buttons }
            VStack(spacing: 10) { buttons }
        }
        .padding(.top, Theme.Space.xs)
    }
}
