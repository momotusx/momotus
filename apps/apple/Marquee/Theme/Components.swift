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
                        .display(Theme.type.headline)
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
/// keeps 3:1 contrast on any artwork and never relies on color alone.
struct FocusRing<S: InsettableShape>: ViewModifier {
    let shape: S
    let active: Bool

    func body(content: Content) -> some View {
        content.overlay {
            // tvOS and macOS only: iOS keeps no TV-style ring, and the system
            // draws its own indicator for keyboard users.
            if active, !touch {
                shape.inset(by: -(3 + Theme.Focus.ring))
                    .strokeBorder(Theme.Palette.accent, lineWidth: Theme.Focus.ring)
                    .shadow(color: Theme.Palette.halo, radius: Theme.Focus.halo / 2)
                    .allowsHitTesting(false)
            }
        }
    }
}

#if os(iOS)
let touch = true
#else
let touch = false
#endif

/// What a card shows of its state: focus rings its art; focus or hover
/// light the art's edge and turn its title accent.
struct CardState: Equatable {
    var focused = false
    var lit = false
}

extension EnvironmentValues {
    @Entry var card = CardState()
}

/// Shared interaction state for cards and pills: focus (tvOS, keyboard),
/// hover (macOS, iPad pointer) and press.
extension View {
    /// The pressed state on a touch screen: a grey veil at a fifth over the
    /// control's shape, as Android has it. It shows with Reduce Motion too,
    /// where the press scale does not. Focus and the pointer have their own
    /// states, so only iOS gets it.
    @ViewBuilder func pressVeil(_ shape: some Shape, pressed: Bool) -> some View {
        #if os(iOS)
        overlay { shape.fill(Color.gray.opacity(pressed ? 0.2 : 0)).allowsHitTesting(false) }
        #else
        self
        #endif
    }
}

private struct Interactive<S: InsettableShape>: View {
    let configuration: ButtonStyleConfiguration
    let shape: S
    let focusScale: CGFloat
    let content: AnyView
    /// A card: its art carries the ring (`cardArt`) and the card lifts.
    var card = false

    @Environment(\.isFocused) private var focused
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var hovered = false

    var body: some View {
        content
            .environment(\.card, CardState(focused: focused, lit: focused || hovered))
            .pressVeil(shape, pressed: configuration.isPressed)
            .modifier(FocusRing(shape: shape, active: focused && !card))
            .brightness(hovered && !focused && !card ? 0.04 : 0)
            .scaleEffect(reduceMotion ? 1 : scale, anchor: card ? .bottom : .center)
            .offset(y: card && !reduceMotion && (focused || hovered) ? -6 : 0)
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
        return hovered && !card ? 1.03 : 1
    }
}

/// Poster, still and settings cards.
struct CardButtonStyle: ButtonStyle {
    /// A corner radius rings the whole card; nil leaves the ring to its art.
    var radius: CGFloat?
    var scale = Theme.Focus.scaleCard

    func makeBody(configuration: Configuration) -> some View {
        Interactive(
            configuration: configuration,
            shape: RoundedRectangle(cornerRadius: radius ?? Theme.Radius.m, style: .continuous),
            focusScale: scale,
            content: AnyView(configuration.label),
            card: radius == nil
        )
    }
}

extension View {
    /// A card's art: rounded, a faint edge that turns white while the card is
    /// lit, and the focus ring.
    func cardArt(radius: CGFloat = Theme.Radius.m) -> some View {
        modifier(CardArt(radius: radius))
    }
}

private struct CardArt: ViewModifier {
    let radius: CGFloat
    @Environment(\.card) private var card

    func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
        content
            .clipShape(shape)
            .overlay { shape.strokeBorder(.white.opacity(card.lit ? 0.5 : 0.06), lineWidth: 1).allowsHitTesting(false) }
            .modifier(FocusRing(shape: shape, active: card.focused))
    }
}

/// A card's name: 700, accent while the card is lit. Accent text sits on
/// `bg` only, so a name on a glass card (`onGlass`) stays white.
struct CardTitle: View {
    let text: String
    var size = Theme.type.callout
    var onGlass = false
    var lines = 1
    @Environment(\.card) private var card

    var body: some View {
        Text(text)
            .font(.text(size, weight: .bold, relativeTo: .callout))
            .tracking(size * Theme.Tracking.label)
            .foregroundStyle(card.lit && !onGlass ? Theme.Palette.accent : Theme.Palette.text)
            .lineLimit(lines)
            .multilineTextAlignment(.leading)
    }
}

/// Capsule buttons. `prominent` is the one primary action on a screen: the
/// white pill. The rest are a faint sheen with a `control` edge.
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
                    .labelFont(Theme.type.headline)
                    .labelStyle(PillLabelStyle())
                    .foregroundStyle(prominent ? Theme.Palette.onPrimary : Theme.Palette.text)
                    .padding(.horizontal, Self.horizontal)
                    .padding(.vertical, Self.vertical)
                    .background {
                        if prominent {
                            Capsule().fill(Theme.Palette.primary).shadow(color: .black.opacity(0.28), radius: 8, y: 3)
                        } else {
                            Capsule().fill(Theme.controlFill)
                        }
                    }
                    .overlay {
                        if !prominent { Capsule().strokeBorder(Theme.Palette.control, lineWidth: edgeWidth) }
                    }
                    .contentShape(Capsule())
                    .opacity(enabled ? 1 : 0.55)
            )
        )
    }

    #if os(tvOS)
    static let horizontal: CGFloat = 32
    static let vertical: CGFloat = 18
    #else
    static let horizontal: CGFloat = 20
    static let vertical: CGFloat = 13
    #endif
}

/// The width of a control's edge: 2 pt on TV, 1.5 pt elsewhere.
#if os(tvOS)
let edgeWidth: CGFloat = 2
#else
let edgeWidth: CGFloat = 1.5
#endif

/// A round button with its word under the circle (Details, Trailer,
/// Versions). `on` marks a state that holds, like In Library: a lighter fill
/// and a bright edge, never the ring (the ring means focus).
struct RoundButtonStyle: ButtonStyle {
    var on = false

    func makeBody(configuration: Configuration) -> some View {
        configuration.label.labelStyle(RoundLabelStyle(on: on, pressed: configuration.isPressed))
    }
}

private struct RoundLabelStyle: LabelStyle {
    let on: Bool
    let pressed: Bool

    func makeBody(configuration: Configuration) -> some View {
        RoundLabel(icon: configuration.icon, title: configuration.title, on: on, pressed: pressed)
    }
}

private struct RoundLabel<Icon: View, Title: View>: View {
    let icon: Icon
    let title: Title
    let on: Bool
    let pressed: Bool
    @Environment(\.isFocused) private var focused
    @Environment(\.isEnabled) private var enabled
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var hovered = false

    var body: some View {
        VStack(spacing: Self.gap) {
            icon
                .foregroundStyle(Theme.Palette.text)
                .frame(width: Self.size, height: Self.size)
                .background(Circle().fill(on ? AnyShapeStyle(Color.white.opacity(0.22)) : AnyShapeStyle(Theme.controlFill)))
                .overlay(Circle().strokeBorder(edge, lineWidth: edgeWidth))
                .pressVeil(Circle(), pressed: pressed)
                .modifier(FocusRing(shape: Circle(), active: focused))
                .scaleEffect(reduceMotion ? 1 : pressed ? 0.95 : focused ? Theme.Focus.scaleCard : 1)
            title
                .font(.text(Self.word, weight: Font.Weight(token: Theme.Weight.label), relativeTo: .caption))
                .foregroundStyle(focused || hovered ? Theme.Palette.text : Theme.Palette.muted)
                .lineLimit(2)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
        }
        .opacity(enabled ? 1 : 0.55)
        .contentShape(Rectangle())
        .animation(Theme.Motion.focus, value: focused)
        .animation(Theme.Motion.focus, value: hovered)
        .animation(Theme.Motion.focus, value: pressed)
        #if !os(tvOS)
        .onHover { hovered = $0 }
        #endif
    }

    private var edge: Color {
        if on { return .white.opacity(0.55) }
        return hovered ? .white.opacity(0.44) : Theme.Palette.control
    }

    #if os(tvOS)
    private static var size: CGFloat { 72 }
    private static var word: CGFloat { Theme.type.caption }
    private static var gap: CGFloat { 10 }
    #elseif os(macOS)
    private static var size: CGFloat { 48 }
    private static var word: CGFloat { 13 }
    private static var gap: CGFloat { 8 }
    #else
    private static var size: CGFloat { 44 }
    private static var word: CGFloat { 12 }
    private static var gap: CGFloat { 6 }
    #endif
}

extension View {
    /// A text field's box: the surface fill and edge, the edge turning accent
    /// while focused, and on TV, where a field is a focus stop like a button,
    /// the shared focus ring.
    func fieldChrome(focused: Bool) -> some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous)
        return background(shape.fill(Theme.Palette.surface))
            .overlay(shape.strokeBorder(focused ? Theme.Palette.accent : Theme.Palette.control, lineWidth: focused ? 2 : 1))
            .modifier(FocusRing(shape: shape, active: focused))
            .animation(Theme.Motion.focus, value: focused)
    }
}

/// TV: words with no action of their own (a movie's facts, About, the hidden
/// versions) take focus, ringed, so the page scrolls to them: a remote
/// scrolls only by moving focus. Elsewhere they stay plain.
struct ReadableOnTV: ViewModifier {
    @FocusState private var focused: Bool

    func body(content: Content) -> some View {
        #if os(tvOS)
        content
            .focusable()
            .focused($focused)
            .modifier(FocusRing(shape: RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous), active: focused))
        #else
        content
        #endif
    }
}

/// A control drawn by its own label (a settings row, a door, a key, a
/// transport glyph, an OSD menu): the shared focus ring, halo and lift on TV
/// and with a Mac keyboard, the press veil on a touch screen. Anything a
/// remote can focus uses this or another style here, never `.plain`, which
/// draws no focus mark on tvOS.
struct FocusPlainStyle<S: InsettableShape>: ButtonStyle {
    let shape: S
    var scale = Theme.Focus.scaleTile

    func makeBody(configuration: Configuration) -> some View {
        Interactive(
            configuration: configuration, shape: shape, focusScale: scale,
            content: AnyView(configuration.label.contentShape(shape)))
    }
}

extension ButtonStyle where Self == FocusPlainStyle<RoundedRectangle> {
    /// A row or a door: the ring follows the card's corners.
    static var row: Self { FocusPlainStyle(shape: RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous)) }
}

extension ButtonStyle where Self == FocusPlainStyle<Circle> {
    /// A round control (keypad key, transport, the player's Back).
    static var round: Self { FocusPlainStyle(shape: Circle(), scale: Theme.Focus.scaleCard) }
}

extension ButtonStyle where Self == FocusPlainStyle<Capsule> {
    /// Words as a link ("More", "Browse first", "Get a key").
    static var textLink: Self { FocusPlainStyle(shape: Capsule()) }
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
                    .labelFont(Theme.type.headline)
                    .labelStyle(PillLabelStyle())
                    .foregroundStyle(enabled ? Theme.Palette.bad : Theme.Palette.muted)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 11)
                    .contentShape(Capsule())
            )
        )
    }
}

struct PillLabelStyle: LabelStyle {
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
                    Capsule().fill(Theme.Palette.accent).frame(width: geo.size.width * min(max(value, 0), 1))
                }
        }
        .frame(height: 4)
        .accessibilityLabel("Progress")
        .accessibilityValue(Text(value, format: .percent.precision(.fractionLength(0))))
    }
}

/// A short explanation for an empty or failed state, with an optional fix
/// in the primary style. It wraps rather than cut a reason off, and the fix
/// moves under the reason when the two do not fit side by side.
struct StateLine: View {
    let symbol: String
    let text: String
    var action: (title: String, run: () -> Void)?
    /// False inside a page that has its own primary (a failed row on a working Home).
    var primary = true

    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: Theme.Space.s) { reason; fix }
            VStack(alignment: .leading, spacing: Theme.Space.s) { reason; fix }
        }
    }

    private var reason: some View {
        Label(text, glyph: symbol)
            .font(.text(Theme.type.callout, relativeTo: .callout))
            .foregroundStyle(Theme.Palette.muted)
            .fixedSize(horizontal: false, vertical: true)
    }

    @ViewBuilder private var fix: some View {
        if let action {
            Button(action.title, action: action.run).buttonStyle(PillButtonStyle(prominent: primary))
        }
    }
}
