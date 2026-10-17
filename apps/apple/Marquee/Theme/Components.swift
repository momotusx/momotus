import SwiftUI

/// Remote artwork with a calm placeholder and a short fade in.
struct ArtImage: View {
    let url: String?
    var mode: ContentMode = .fill

    var body: some View {
        AsyncImage(url: url.flatMap(URL.init(string:)), transaction: Transaction(animation: .easeOut(duration: 0.25))) { phase in
            if let image = phase.image {
                image.resizable().aspectRatio(contentMode: mode)
            } else {
                Theme.Palette.surface
            }
        }
    }
}

/// Accent ring with a dark halo, the focus mark from the design system. It
/// keeps 3:1 contrast on any artwork and never relies on color alone.
struct FocusRing<S: InsettableShape>: ViewModifier {
    let shape: S
    let active: Bool

    func body(content: Content) -> some View {
        content.overlay {
            if active {
                shape.inset(by: -(Theme.Focus.ring + 3))
                    .strokeBorder(Theme.Palette.accent, lineWidth: Theme.Focus.ring)
                    .shadow(color: Theme.Palette.halo, radius: Theme.Focus.halo / 2)
                    .allowsHitTesting(false)
            }
        }
    }
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
                        if !prominent { Capsule().strokeBorder(Theme.Palette.line, lineWidth: 1) }
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

private struct PillLabelStyle: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 8) {
            configuration.icon.imageScale(.medium)
            configuration.title
        }
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

/// One-line explanation for an empty or failed state, with an optional fix.
struct StateLine: View {
    let symbol: String
    let text: String
    var action: (title: String, run: () -> Void)?

    var body: some View {
        HStack(spacing: Theme.Space.s) {
            Label(text, systemImage: symbol)
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.muted)
                .lineLimit(2)
            if let action {
                Button(action.title, action: action.run).buttonStyle(PillButtonStyle())
            }
        }
    }
}
