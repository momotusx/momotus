import SwiftUI

/// The next episode near the end of this one: its still, its name, a
/// countdown when it starts on its own, Play now and Cancel.
struct UpNextCard: View {
    let model: PlayerModel
    @Environment(\.metrics) private var metrics

    var body: some View {
        if let failure = model.nextFailure {
            Label(failure, systemImage: "exclamationmark.circle")
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(.white)
                .padding(Theme.Space.s)
                .frame(maxWidth: 360, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: Theme.Radius.m).fill(.black.opacity(0.75)))
        } else if let next = model.next {
            HStack(alignment: .top, spacing: Theme.Space.s) {
                if metrics.wide {
                    ArtImage(url: next.thumbnail, maxPixels: 640, fallback: next.title)
                        .frame(width: 192, height: 108)
                        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous))
                }
                VStack(alignment: .leading, spacing: 8) {
                    Text("NEXT EPISODE")
                        .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
                        .tracking(1.6)
                        .foregroundStyle(.white.opacity(0.7))
                    Text("S\(next.season):E\(next.episode) \(next.title)")
                        .font(.display(Theme.type.headline + 2))
                        .foregroundStyle(.white)
                        .lineLimit(2)
                    if model.startingNext {
                        Label("Finding a version", systemImage: "clock")
                            .font(.text(Theme.type.callout, relativeTo: .callout))
                            .foregroundStyle(.white.opacity(0.8))
                    } else if model.autoplayNext {
                        Text(model.ended ? "Starting" : "Starts in \(model.countdown)")
                            .font(.text(Theme.type.callout, relativeTo: .callout))
                            .monospacedDigit()
                            .foregroundStyle(.white.opacity(0.8))
                    }
                    HStack(spacing: 10) {
                        Button { Task { await model.playNext() } } label: { Label("Play now", systemImage: "forward.end.fill") }
                            .buttonStyle(PillButtonStyle(prominent: true))
                        Button("Cancel", action: model.cancelNext)
                            .buttonStyle(PillButtonStyle())
                    }
                    .disabled(model.startingNext)
                }
            }
            .padding(Theme.Space.s)
            .frame(maxWidth: metrics.wide ? 560 : 340, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous).fill(.black.opacity(0.78)))
            .overlay(RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous).strokeBorder(.white.opacity(0.12)))
            .accessibilityElement(children: .contain)
            .accessibilityLabel("Up next")
        }
    }
}
