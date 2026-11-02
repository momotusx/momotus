import SwiftUI

#if os(macOS)
/// The Mac's navigation, the owner's variant 4 (plans/owner-picks.md, 2026-10-25): no toolbar strip, the
/// places in one glass capsule floating at the window's bottom centre, over every page.
enum OrnamentLayout {
    /// Search is the glass button at the top right, so the ornament holds the other places, with no wordmark, and
    /// ends with the profile, whose menu opens Settings (plans/nav-profile.md).
    static let places: [AppTab] = AppTab.bar.filter { $0 != .search }
    /// The capsule's edge to its items, and an item's height.
    static let inset: CGFloat = 5
    static let itemHeight: CGFloat = 34
    /// The capsule's foot above the window's.
    static let gap: CGFloat = 22
    static var height: CGFloat { itemHeight + 2 * inset }
}

/// The capsule: each place as its icon and word, the current one on a white fill, then `profile` at its end (the
/// profile's menu, which opens Settings: plans/nav-profile.md). `strong` darkens the glass once something other
/// than Home's art lies behind it.
struct PlaceOrnament<Profile: View>: View {
    let selection: AppTab
    var strong = false
    let select: (AppTab) -> Void
    let profile: Profile
    @Environment(ProfileSession.self) private var profiles: ProfileSession?

    var body: some View {
        HStack(spacing: 2) {
            ForEach(OrnamentLayout.places.filter { profiles?.snapshot?.current?.kids != true || !$0.adultOnly }, id: \.self) { item in
                Button { select(item) } label: { Label(item.title, glyph: item.symbol, size: 16) }
                    .buttonStyle(OrnamentItemStyle(on: item == selection))
                    .accessibilityAddTraits(item == selection ? .isSelected : [])
            }
            profile
        }
        .padding(OrnamentLayout.inset)
        .fixedSize()
        .modifier(OrnamentGlass(shape: Capsule(), strong: strong))
        // A named group: VoiceOver reads "Places", then "Home, selected, button".
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Places")
        .animation(Theme.Motion.panel, value: strong)
    }
}

extension PlaceOrnament where Profile == EmptyView {
    init(selection: AppTab, strong: Bool = false, select: @escaping (AppTab) -> Void) {
        self.init(selection: selection, strong: strong, select: select, profile: EmptyView())
    }
}

/// The ornament's last item: the profile's avatar at a place's height, on the white fill while Settings shows, a faint
/// fill under the pointer and the key ring for keyboard focus, as the places have.
struct OrnamentProfile<Avatar: View>: View {
    let lit: Bool
    @ViewBuilder let avatar: Avatar
    @KeyboardFocused private var focused
    @State private var hovered = false

    var body: some View {
        avatar
            .padding(.horizontal, 8)
            .frame(height: OrnamentLayout.itemHeight)
            .background {
                if lit { Capsule().fill(Theme.Palette.primary) } else if hovered { Capsule().fill(.white.opacity(0.12)) }
            }
            .contentShape(Capsule())
            .modifier(FocusRing(shape: Capsule(), active: focused))
            .onHover { hovered = $0 }
    }
}

extension View {
    /// The ornament at the window's foot, as a bottom safe-area inset: a page's scrolling content runs under it
    /// and ends above it, so the last row is never hidden there. `shown` false (the player) leaves no room.
    /// `placed` hears where the ornament lands, in the window (PlaceOrnamentTests).
    func placeOrnament(
        _ selection: AppTab, strong: Bool, shown: Bool, placed: ((CGRect) -> Void)? = nil, select: @escaping (AppTab) -> Void
    ) -> some View {
        placeOrnament(selection, strong: strong, shown: shown, placed: placed, select: select) { EmptyView() }
    }

    func placeOrnament<Profile: View>(
        _ selection: AppTab, strong: Bool, shown: Bool, placed: ((CGRect) -> Void)? = nil, select: @escaping (AppTab) -> Void,
        @ViewBuilder profile: () -> Profile
    ) -> some View {
        let end = profile()
        return safeAreaInset(edge: .bottom, spacing: 0) {
            if shown {
                PlaceOrnament(selection: selection, strong: strong, select: select, profile: end)
                    .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { placed?($0) }
                    .padding(.bottom, OrnamentLayout.gap)
                    .frame(maxWidth: .infinity)
            }
        }
    }
}

/// The ornament's glass and the top right buttons': solid under Reduce Transparency.
struct OrnamentGlass<S: InsettableShape>: ViewModifier {
    let shape: S
    var strong = false
    @Environment(\.accessibilityReduceTransparency) private var solid

    func body(content: Content) -> some View {
        if solid {
            content
                .background(shape.fill(Theme.Palette.raised))
                .overlay(shape.strokeBorder(Theme.Palette.edge, lineWidth: 1).allowsHitTesting(false))
        } else {
            content.glass(shape, strong: strong, shadow: true)
        }
    }
}

/// A place in the ornament: 14/600 beside its 16 pt icon; the current one dark on white, the others white,
/// brighter under the pointer; the key ring for keyboard focus.
private struct OrnamentItemStyle: ButtonStyle {
    let on: Bool

    func makeBody(configuration: Configuration) -> some View {
        Item(configuration: configuration, on: on)
    }

    private struct Item: View {
        let configuration: ButtonStyleConfiguration
        let on: Bool
        @KeyboardFocused private var focused
        @State private var hovered = false

        var body: some View {
            configuration.label
                .labelStyle(ItemLabel())
                .textFont(14, weight: .semibold, relativeTo: .callout)
                .foregroundStyle(on ? Theme.Palette.onPrimary : hovered ? Theme.Palette.text : Theme.Palette.muted)
                .padding(.horizontal, 15)
                .frame(height: OrnamentLayout.itemHeight)
                .background { if on { Capsule().fill(Theme.Palette.primary) } }
                .contentShape(Capsule())
                .pressVeil(Capsule(), pressed: configuration.isPressed)
                .modifier(FocusRing(shape: Capsule(), active: focused))
                .onHover { hovered = $0 }
        }
    }

    private struct ItemLabel: LabelStyle {
        func makeBody(configuration: Configuration) -> some View {
            HStack(spacing: 7) {
                configuration.icon
                configuration.title
            }
        }
    }
}

/// The window's top once content scrolls under it (variant 4's soft edge): a blur fading out downward over a
/// page-coloured wash, so the traffic lights and the top buttons read over rows; the wash alone under Reduce
/// Transparency.
struct ScrollTopEdge: View {
    @Environment(\.accessibilityReduceTransparency) private var solid

    var body: some View {
        let fade = LinearGradient(stops: [.init(color: .black, location: 0.45), .init(color: .clear, location: 1)], startPoint: .top, endPoint: .bottom)
        ZStack {
            if !solid { Rectangle().fill(.ultraThinMaterial).mask(fade) }
            LinearGradient(colors: [Theme.Palette.bg.opacity(0.78), Theme.Palette.bg.opacity(0)], startPoint: .top, endPoint: .bottom)
        }
        .frame(height: Self.height)
        .frame(maxWidth: .infinity)
        .ignoresSafeArea(edges: .top)
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }

    static let height: CGFloat = 78
}

/// The top right's round glass button (Search): the glyph alone, 36 pt.
struct GlassCircleStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        Circle36(configuration: configuration)
    }

    private struct Circle36: View {
        let configuration: ButtonStyleConfiguration
        @KeyboardFocused private var focused

        var body: some View {
            configuration.label
                .labelStyle(.iconOnly)
                .foregroundStyle(Theme.Palette.text)
                .frame(width: 36, height: 36)
                .modifier(OrnamentGlass(shape: Circle()))
                .contentShape(Circle())
                .pressVeil(Circle(), pressed: configuration.isPressed)
                .modifier(FocusRing(shape: Circle(), active: focused))
        }
    }
}
#endif
