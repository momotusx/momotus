import CoreText
import SwiftUI

/// One color channel set, 0...1, as written by the token generator.
struct RGBA: Sendable {
    let r, g, b, a: Double
    init(_ r: Double, _ g: Double, _ b: Double, _ a: Double) {
        (self.r, self.g, self.b, self.a) = (r, g, b, a)
    }
}

extension Color {
    init(_ c: RGBA) {
        self.init(.sRGB, red: c.r, green: c.g, blue: c.b, opacity: c.a)
    }
}

extension Theme {
    /// Secondary controls (web `.btn`): a faint white sheen over the page,
    /// edged with `control`.
    static let controlFill = LinearGradient(
        colors: [.white.opacity(0.12), .white.opacity(0.065)], startPoint: .topLeading, endPoint: .bottomTrailing)
}

extension View {
    /// Glass: the token tint over a blur of what lies behind, a 1 pt cool
    /// edge with a brighter top, and for floating pieces a soft shadow.
    func glass<S: InsettableShape>(_ shape: S, strong: Bool = false, shadow: Bool = false) -> some View {
        background {
            ZStack {
                shape.fill(.ultraThinMaterial)
                shape.fill(strong ? Theme.Palette.glassStrong : Theme.Palette.glass)
            }
            .shadow(color: .black.opacity(shadow ? 0.24 : 0), radius: 24, y: 18)
        }
        .overlay {
            shape.strokeBorder(
                LinearGradient(
                    stops: [.init(color: .white.opacity(0.2), location: 0), .init(color: Theme.Palette.edge, location: 0.3)],
                    startPoint: .top, endPoint: .bottom),
                lineWidth: 1)
            .allowsHitTesting(false)
        }
    }

    /// Display and heading type: Inter 800, tracked tight (display from
    /// title1 up, heading below).
    func display(_ size: CGFloat, relativeTo style: Font.TextStyle = .title) -> some View {
        font(.display(size, relativeTo: style))
            .tracking(size * (size >= Theme.type.title1 ? Theme.Tracking.display : Theme.Tracking.heading))
    }

    /// Labels on controls: the label weight, tracked a little tight.
    func labelFont(_ size: CGFloat, relativeTo style: Font.TextStyle = .headline) -> some View {
        font(.text(size, weight: Font.Weight(token: Theme.Weight.label), relativeTo: style))
            .tracking(size * Theme.Tracking.label)
    }
}

extension Font.Weight {
    /// The nearest named weight at or below a token weight (650 sets as semibold).
    init(token: CGFloat) {
        let named: [(CGFloat, Font.Weight)] = [(900, .black), (800, .heavy), (700, .bold), (600, .semibold), (500, .medium)]
        self = named.first { token >= $0.0 }?.1 ?? .regular
    }
}

enum Fonts {
    /// Registers the bundled OFL fonts (Resources/Fonts) for this process.
    static func register() {
        guard let dir = Bundle.main.url(forResource: "Fonts", withExtension: nil),
              let files = FileManager.default.enumerator(at: dir, includingPropertiesForKeys: nil)
        else { return }
        for case let url as URL in files where url.pathExtension == "ttf" {
            CTFontManagerRegisterFontsForURL(url as CFURL, .process, nil)
        }
    }
}

extension Font {
    /// Inter at the display weight: titles and headings. Use `View.display`
    /// for its tracking too.
    @MainActor static func display(_ size: CGFloat, relativeTo style: Font.TextStyle = .title) -> Font {
        .custom(Theme.Family.display, size: size, relativeTo: style).weight(Font.Weight(token: Theme.Weight.display).legible)
    }

    /// Inter: everything the user reads to decide.
    @MainActor static func text(_ size: CGFloat, weight: Font.Weight = .regular, relativeTo style: Font.TextStyle = .body) -> Font {
        .custom(Theme.Family.text, size: size, relativeTo: style).weight(weight.legible)
    }

    /// IBM Plex Mono: release names and technical lines.
    @MainActor static func mono(_ size: CGFloat) -> Font {
        .custom(Font.Weight.boldText ? "IBMPlexMono-Medium" : "IBMPlexMono-Regular", size: size, relativeTo: .caption)
    }
}

extension Text {
    /// Eyebrows, kickers and field labels: capitals on screen, the plain words
    /// for VoiceOver, which can spell out a word set in capitals.
    init(caps words: String) {
        self = Text(words.uppercased()).accessibilityLabel(words)
    }
}

extension Font.Weight {
    /// Bold Text (Settings, Accessibility, Display & Text Size): SwiftUI bolds
    /// the system font for it, never a custom one, so the bundled faces step
    /// up a weight themselves.
    @MainActor static var boldText: Bool {
        #if os(iOS) || os(tvOS)
        UIAccessibility.isBoldTextEnabled
        #else
        false
        #endif
    }

    /// This weight, one step heavier under Bold Text.
    @MainActor var legible: Font.Weight {
        guard Font.Weight.boldText else { return self }
        switch self {
        case .ultraLight, .thin, .light: return .regular
        case .regular, .medium: return .semibold
        case .semibold: return .bold
        case .bold: return .heavy
        default: return .black
        }
    }
}

/// Layout numbers that depend on the device class, set once at the root.
struct Metrics: Sendable {
    let poster: CGSize
    let gutter: CGFloat
    let rowSpacing: CGFloat
    let wide: Bool
    /// Under card art: the name, then the year or the core's caption.
    let cardName: CGFloat
    let cardInfo: CGFloat
    /// Text in a poster's rating chip.
    let chip: CGFloat

    static let tv = Metrics(
        poster: Theme.Poster.tv, gutter: Theme.safeArea.side, rowSpacing: Theme.Space.l, wide: true,
        cardName: Theme.type.caption, cardInfo: Theme.type.caption - 2, chip: 20)
    static let regular = Metrics(
        poster: CGSize(width: 160, height: 240), gutter: Theme.Space.l, rowSpacing: Theme.Space.l, wide: true,
        cardName: 15, cardInfo: 13, chip: 12)
    static let compact = Metrics(
        poster: CGSize(width: 112, height: 168), gutter: 16, rowSpacing: Theme.Space.m, wide: false,
        cardName: 13, cardInfo: 12, chip: 11)
}

extension EnvironmentValues {
    @Entry var metrics = Metrics.regular
}

/// The player asks for the whole window (the navigation hides) while it is up.
@MainActor @Observable
final class Immersion {
    var active = false
}
