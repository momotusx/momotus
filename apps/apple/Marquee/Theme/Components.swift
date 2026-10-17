import SwiftUI

/// Remote artwork with a calm placeholder; it fades in unless it was cached.
struct ArtImage: View {
    let url: String?
    var mode: ContentMode = .fill
    var maxPixels = ArtSize.poster
    /// Shown in the display face when there is no art or it cannot load.
    var fallback: String?
    @State private var art: DecodedArt?
    @State private var missing = false

    var body: some View {
        // The art is an overlay so its natural size never reaches layout: a
        // filled image is wider than its frame, and letting that through made
        // the title page grow without end.
        Theme.Palette.surface
            .overlay {
                if let art {
                    Image(decorative: art.image, scale: 1)
                        .resizable()
                        .aspectRatio(contentMode: mode)
                        .transition(.opacity)
                } else if missing, let fallback {
                    Text(fallback)
                        .font(.display(Theme.type.headline))
                        .foregroundStyle(Theme.Palette.text.opacity(0.85))
                        .multilineTextAlignment(.center)
                        .padding(Theme.Space.xs)
                }
            }
            .clipped()
            .task(id: url) {
                missing = false
                let start = ContinuousClock.now
                let loaded = await ArtLoader.shared.image(url, maxPixels: maxPixels)
                withAnimation(reveal(since: start)) {
                    art = loaded
                    missing = loaded == nil
                }
            }
    }
}

extension ArtLoader {
    /// Art for a core URL string, or nil for a missing or bad URL.
    func image(_ url: String?, maxPixels: Int) async -> DecodedArt? {
        guard let url = url.flatMap(URL.init(string:)) else { return nil }
        return await image(url, maxPixels: maxPixels)
    }
}

/// A short fade for art that had to load; none for memory hits.
func reveal(since start: ContinuousClock.Instant) -> Animation? {
    ContinuousClock.now - start > .milliseconds(50) ? .easeOut(duration: 0.25) : nil
}

/// Accent ring with a dark halo, the focus mark from the design system. It
/// keeps 3:1 contrast on any artwork and never relies on color alone. The
/// halo is soft on the dark theme and a crisp outer line on the light one.
struct FocusRing<S: InsettableShape>: ViewModifier {
    let shape: S
    let active: Bool
    @Environment(\.colorScheme) private var scheme

    func body(content: Content) -> some View {
        content.overlay {
            // tvOS and macOS only: iOS keeps no TV-style ring, and the system
            // draws its own indicator for keyboard users.
            if active, !Self.touch {
                let gap: CGFloat = 3
                ZStack {
                    if scheme == .dark {
                        ring(gap).shadow(color: Theme.Palette.halo, radius: Theme.Focus.haloDark / 2)
                    } else {
                        shape.inset(by: -(gap + Theme.Focus.ring + Theme.Focus.haloLight))
                            .strokeBorder(Theme.Palette.halo, lineWidth: Theme.Focus.haloLight)
                        ring(gap)
                    }
                }
                .allowsHitTesting(false)
            }
        }
    }

    private func ring(_ gap: CGFloat) -> some View {
        shape.inset(by: -(gap + Theme.Focus.ring)).strokeBorder(Theme.Palette.accent, lineWidth: Theme.Focus.ring)
    }

    #if os(iOS)
    private static var touch: Bool { true }
    #else
    private static var touch: Bool { false }
    #endif
}

/// Shared interaction state for cards and pills: focus (tvOS, keyboard),
/// hover (macOS, iPad pointer) and press.
private struct Interactive<S: InsettableShape>: View {
    let configuration: ButtonStyleConfiguration
    let shape: S
    let focusScale: CGFloat
    let content: AnyView

    @Environment(\.isFocused) private var focused
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var hovered = false

    var body: some View {
        content
            .modifier(FocusRing(shape: shape, active: focused))
            .brightness(hovered && !focused ? 0.04 : 0)
            .scaleEffect(reduceMotion ? 1 : scale)
            .animation(Theme.Motion.focus, value: focused)
            .animation(Theme.Motion.focus, value: hovered)
            .animation(Theme.Motion.focus, value: configuration.isPressed)
            #if !os(tvOS)
            .onHover { hovered = $0 }
            #endif
    }

    private var scale: CGFloat {
        if configuration.isPressed { return 0.97 }
        if focused { return focusScale }
        return hovered ? 1.03 : 1
    }
}

/// Poster and episode cards.
struct CardButtonStyle: ButtonStyle {
    var radius: CGFloat = Theme.Radius.s

    func makeBody(configuration: Configuration) -> some View {
        Interactive(
            configuration: configuration,
            shape: RoundedRectangle(cornerRadius: radius, style: .continuous),
            focusScale: Theme.Focus.scaleCard,
            content: AnyView(configuration.label)
        )
    }
}

/// Capsule buttons. `prominent` is the one primary action on a screen.
struct PillButtonStyle: ButtonStyle {
    var prominent = false
    @Environment(\.isEnabled) private var enabled

    func makeBody(configuration: Configuration) -> some View {
        Interactive(
            configuration: configuration,
            shape: Capsule(),
            focusScale: Theme.Focus.scaleTile,
            content: AnyView(
                configuration.label
                    .font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                    .labelStyle(PillLabelStyle())
                    .foregroundStyle(prominent ? Theme.Palette.onAccent : enabled ? Theme.Palette.text : Theme.Palette.muted)
                    .padding(.horizontal, Self.horizontal)
                    .padding(.vertical, Self.vertical)
                    .background {
                        Capsule().fill(prominent ? AnyShapeStyle(Theme.Palette.accent) : AnyShapeStyle(Theme.Palette.raised.opacity(0.72)))
                    }
                    .overlay {
                        if !prominent { Capsule().strokeBorder(Theme.Palette.control, lineWidth: 1) }
                    }
                    .contentShape(Capsule())
            )
        )
    }

    #if os(tvOS)
    private static let horizontal: CGFloat = 32
    private static let vertical: CGFloat = 18
    #else
    private static let horizontal: CGFloat = 20
    private static let vertical: CGFloat = 11
    #endif
}

/// A destructive action as words in the error token: no fill, never the
/// accent, so it cannot pass for the primary. Same height as a pill.
struct DestructiveTextStyle: ButtonStyle {
    @Environment(\.isEnabled) private var enabled

    func makeBody(configuration: Configuration) -> some View {
        Interactive(
            configuration: configuration,
            shape: Capsule(),
            focusScale: Theme.Focus.scaleTile,
            content: AnyView(
                configuration.label
                    .font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                    .labelStyle(PillLabelStyle())
                    .foregroundStyle(enabled ? Theme.Palette.bad : Theme.Palette.muted)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 11)
                    .contentShape(Capsule())
            )
        )
    }
}

private struct PillLabelStyle: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 8) {
            configuration.icon.imageScale(.medium)
            configuration.title
        }
    }
}

/// Lays children out left to right, wrapping onto new lines when they do not fit.
struct FlowRow: Layout {
    var spacing: CGFloat = 12

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let frames = place(subviews, width: proposal.width ?? .infinity)
        return CGSize(width: frames.map(\.maxX).max() ?? 0, height: frames.map(\.maxY).max() ?? 0)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for (subview, frame) in zip(subviews, place(subviews, width: bounds.width)) {
            subview.place(at: CGPoint(x: bounds.minX + frame.minX, y: bounds.minY + frame.minY), proposal: ProposedViewSize(frame.size))
        }
    }

    private func place(_ subviews: Subviews, width: CGFloat) -> [CGRect] {
        var frames: [CGRect] = []
        var x: CGFloat = 0, y: CGFloat = 0, line: CGFloat = 0
        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x > 0, x + size.width > width {
                (x, y, line) = (0, y + line + spacing, 0)
            }
            frames.append(CGRect(origin: CGPoint(x: x, y: y), size: size))
            x += size.width + spacing
            line = max(line, size.height)
        }
        return frames
    }
}

extension View {
    /// Lets the user copy a code or release name; tvOS has no text selection.
    func selectableText() -> some View {
        #if os(tvOS)
        self
        #else
        textSelection(.enabled)
        #endif
    }
}

/// Thin progress bar for partly watched items.
struct ProgressLine: View {
    let value: Double

    var body: some View {
        GeometryReader { geo in
            Capsule().fill(Theme.Palette.text.opacity(0.25))
                .overlay(alignment: .leading) {
                    Capsule().fill(Theme.Palette.text).frame(width: geo.size.width * min(max(value, 0), 1))
                }
        }
        .frame(height: 4)
        .accessibilityLabel("Progress")
        .accessibilityValue(Text(value, format: .percent.precision(.fractionLength(0))))
    }
}

/// A short explanation for an empty or failed state, with an optional fix.
/// It wraps rather than cut a reason off.
struct StateLine: View {
    let symbol: String
    let text: String
    var action: (title: String, run: () -> Void)?

    var body: some View {
        HStack(spacing: Theme.Space.s) {
            Label(text, glyph: symbol)
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
            if let action {
                Button(action.title, action: action.run).buttonStyle(PillButtonStyle())
            }
        }
    }
}
