import SwiftUI

/// Settings A1's pieces (design/DECISION.md, Settings): the header on its film's backdrop, the icon tiles, the
/// grouped list and its rows. The page puts them together (SettingsScreen).

extension EnvironmentValues {
    /// A row or menu entry under TV focus: its fill turns white, so its words and glyphs turn dark.
    @Entry var settingsLit = false
}

/// One rounded tile with a white glyph on a blue-to-green step.
struct IconTile: View {
    let tone: SettingsTone
    let glyph: String
    let size: CGFloat

    var body: some View {
        RoundedRectangle(cornerRadius: size / 4, style: .continuous)
            .fill(tone.fill)
            .frame(width: size, height: size)
            .overlay {
                Image(glyph).renderingMode(.template).resizable().scaledToFit()
                    .frame(width: size * 0.6, height: size * 0.6)
                    .foregroundStyle(.white)
            }
            .accessibilityHidden(true)
    }
}

/// A grouped list: solid `surface`, hairlines from the rows' titles on (they start past the tile).
struct SettingsGroup<Content: View>: View {
    let sizes: SettingsRules.Sizes
    @ViewBuilder let content: Content

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: sizes.radius, style: .continuous)
        VStack(spacing: 0) {
            Group(subviews: content) { rows in
                ForEach(rows) { row in
                    if row.id != rows.first?.id {
                        Rectangle().fill(SettingsRowLayout.hairline).frame(height: SettingsRowLayout.hairWidth)
                            .padding(.leading, SettingsRowLayout.titleInset(sizes))
                    }
                    row
                }
            }
        }
        .background(shape.fill(Theme.Palette.surface))
        .clipShape(shape)
    }
}

enum SettingsRowLayout {
    #if os(tvOS)
    static let side: CGFloat = 30
    static let vertical: CGFloat = 18
    static let spacing: CGFloat = 28
    static let hairWidth: CGFloat = 1
    static let hairline = Theme.Palette.line
    #elseif os(macOS)
    static let side: CGFloat = 18
    static let vertical: CGFloat = 10
    static let spacing: CGFloat = 16
    static let hairWidth: CGFloat = 1
    static let hairline = Theme.Palette.line
    #else
    static let side: CGFloat = 16
    static let vertical: CGFloat = 10
    static let spacing: CGFloat = 14
    static let hairWidth: CGFloat = 0.5
    static let hairline = Color.white.opacity(0.16)
    #endif

    /// Where a row's title starts: the hairlines begin there.
    static func titleInset(_ sizes: SettingsRules.Sizes) -> CGFloat { side + sizes.rowTile + spacing }
}

/// A row that leads somewhere: its tile, its title and line, a chevron at 40 percent.
struct A1Row: View {
    let tone: SettingsTone
    let glyph: String
    let title: String
    let detail: String
    let sizes: SettingsRules.Sizes
    @Environment(\.settingsLit) private var lit
    @Environment(\.metrics) private var metrics

    var body: some View {
        HStack(spacing: SettingsRowLayout.spacing) {
            IconTile(tone: tone, glyph: glyph, size: sizes.rowTile)
            VStack(alignment: .leading, spacing: tvForm ? 4 : 3) {
                Text(title)
                    .textFont(titleSize, weight: metrics.wide ? .semibold : .regular, relativeTo: .headline)
                    .foregroundStyle(lit ? Theme.Palette.onPrimary : Theme.Palette.text)
                Text(detail)
                    .textFont(detailSize, relativeTo: .callout)
                    .foregroundStyle(lit ? Color.black.opacity(0.64) : Theme.Palette.dim)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 0)
            Glyph("chevron-right", size: tvForm ? 30 : metrics.wide ? 16 : 18)
                .foregroundStyle(lit ? Color.black.opacity(0.4) : Color.white.opacity(0.4))
        }
        .multilineTextAlignment(.leading)
        .padding(.horizontal, SettingsRowLayout.side)
        .padding(.vertical, SettingsRowLayout.vertical)
        .frame(maxWidth: .infinity, minHeight: sizes.row, alignment: .leading)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
    }

    private var titleSize: CGFloat { tvForm ? Theme.type.body : metrics.wide ? 16 : 17 }
    private var detailSize: CGFloat { tvForm ? Theme.type.callout : metrics.wide ? 14 : 13 }
}

/// A Settings row or menu entry, or a first-run choice: a white fill under TV focus (its words turn dark), the
/// `hover` fill under the Mac's pointer, the key ring for the keyboard, the press veil on a touch screen.
/// `selected` is the menu's current section: `selected.fill` on TV, `surface` elsewhere; `rest` the fill at rest.
struct LitRowStyle: ButtonStyle {
    var radius: CGFloat
    var selected = false
    var rest = Color.clear
    var hover = Theme.Palette.raised

    func makeBody(configuration: Configuration) -> some View {
        Drawn(configuration: configuration, style: self)
    }

    private struct Drawn: View {
        let configuration: ButtonStyleConfiguration
        let style: LitRowStyle
        @Environment(\.isFocused) private var focused
        @KeyboardFocused private var keyboard
        @Environment(\.accessibilityReduceMotion) private var reduceMotion
        @State private var hovered = false

        var body: some View {
            let shape = RoundedRectangle(cornerRadius: style.radius, style: .continuous)
            #if os(tvOS)
            configuration.label
                .environment(\.settingsLit, focused)
                .background(shape.fill(focused ? Theme.Palette.primary : style.selected ? Theme.Palette.selectedFill : style.rest))
                .clipShape(shape)
                .shadow(color: .black.opacity(focused ? 0.6 : 0), radius: 30, y: 30)
                .scaleEffect(focused && !reduceMotion ? Theme.Focus.scaleTile : 1)
                .animation(Theme.Motion.focus, value: focused)
            #else
            configuration.label
                .background(shape.fill(style.selected ? Theme.Palette.surface : hovered ? style.hover : style.rest))
                .clipShape(shape)
                .pressVeil(shape, pressed: configuration.isPressed)
                .modifier(FocusRing(shape: shape, active: keyboard))
                .onHover { hovered = $0 }
            #endif
        }
    }
}

/// The profile block in the header: on TV focus its avatar turns white and rises; the Mac's keyboard rings it.
struct ProfileBlockStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        Drawn(configuration: configuration)
    }

    private struct Drawn: View {
        let configuration: ButtonStyleConfiguration
        @Environment(\.isFocused) private var focused
        @KeyboardFocused private var keyboard

        var body: some View {
            #if os(tvOS)
            configuration.label.environment(\.settingsLit, focused)
            #else
            let shape = RoundedRectangle(cornerRadius: Theme.Radius.l, style: .continuous)
            configuration.label
                .pressVeil(shape, pressed: configuration.isPressed)
                .modifier(FocusRing(shape: shape, active: keyboard))
            #endif
        }
    }
}

/// The header's art: the film's backdrop under its ramps in the page color. Nothing shows until the picture
/// lands (the header stays on `bg`, never a gradient in its place); it fades in by opacity alone, at once under
/// Reduce Motion.
struct SettingsArt: View {
    let url: String?
    let wide: Bool
    @State private var art: DecodedArt?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        GeometryReader { box in
            ZStack {
                if let art {
                    FilledArt(art: art, size: box.size, y: 0.3).transition(.opacity)
                    ramps
                }
            }
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
        .task(id: url) {
            let loaded = await ArtLoader.shared.image(url, maxPixels: ArtSize.backdrop)
            withAnimation(reduceMotion ? nil : Theme.Motion.panel) { art = loaded }
        }
    }

    @ViewBuilder private var ramps: some View {
        let page = Theme.Palette.bg
        if wide {
            LinearGradient(stops: BackdropStops.gradient(BackdropStops.settingsSide, page), startPoint: .leading, endPoint: .trailing)
            LinearGradient(stops: BackdropStops.gradient(BackdropStops.settingsFoot, page), startPoint: .bottom, endPoint: .top)
        } else {
            LinearGradient(stops: BackdropStops.gradient(BackdropStops.settingsPhone, page), startPoint: .bottom, endPoint: .top)
        }
    }
}

/// Art filling a box, cut at its sides or its top and foot: `y` is the share of what is cut that goes from the
/// top (0.3 keeps the upper middle of a backdrop, where faces sit).
struct FilledArt: View {
    let art: DecodedArt
    let size: CGSize
    let y: CGFloat

    var body: some View {
        let aspect = art.image.height > 0 ? CGFloat(art.image.width) / CGFloat(art.image.height) : 1
        let width = max(size.width, size.height * aspect)
        let height = width / aspect
        Image(decorative: art.image, scale: 1)
            .resizable()
            .frame(width: width, height: height)
            .offset(x: (size.width - width) / 2, y: (size.height - height) * y)
            .frame(width: size.width, height: size.height, alignment: .topLeading)
            .clipped()
    }
}

/// The film's own logo at the header's right, decorative; none when the title has none or it does not load.
struct SettingsLogo: View {
    let url: String
    let size: CGSize
    @State private var art: DecodedArt?

    var body: some View {
        ZStack(alignment: .bottomTrailing) {
            if let art {
                Image(decorative: art.image, scale: 1).resizable().scaledToFit().transition(.opacity)
            }
        }
        .frame(maxWidth: size.width, maxHeight: size.height, alignment: .bottomTrailing)
        .frame(width: size.width, height: size.height, alignment: .bottomTrailing)
        .accessibilityHidden(true)
        .task(id: url) {
            let loaded = await ArtLoader.shared.image(url, maxPixels: ArtSize.logo)
            withAnimation(Theme.Motion.panel) { art = loaded }
        }
    }
}

/// The tvOS build: Settings takes the TV's sizes and focus.
#if os(tvOS)
let tvForm = true
#else
let tvForm = false
#endif
