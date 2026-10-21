import SwiftUI

/// "IMDb 8.0  RT 87%" on a small strong-glass chip at a poster's top right.
/// A score never wraps and is never cut: on a phone's narrow poster the two
/// stack; elsewhere they sit side by side, and stack only when the pair is
/// wider than the poster.
struct RatingChip: View {
    let ratings: [String]
    @Environment(\.metrics) private var metrics

    var body: some View {
        Group {
            if metrics.wide {
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: metrics.chip * 0.5) { scores }
                    stacked
                }
            } else {
                stacked
            }
        }
        .foregroundStyle(Theme.Palette.text)
        .padding(.horizontal, metrics.chip * 0.55)
        .padding(.vertical, metrics.chip * 0.3)
        .frame(minHeight: metrics.chip * 1.8)
        .glass(RoundedRectangle(cornerRadius: metrics.chip * 0.9, style: .continuous), strong: true)
        .accessibilityHidden(true)
    }

    private var stacked: some View {
        VStack(alignment: .trailing, spacing: metrics.chip * 0.1) { scores }
    }

    @ViewBuilder private var scores: some View {
        ForEach(ratings, id: \.self) { score in
            Text(score)
                .font(.text(metrics.chip, weight: .bold, relativeTo: .caption2))
                .monospacedDigit()
                .lineLimit(1)
                .fixedSize()
        }
    }
}
