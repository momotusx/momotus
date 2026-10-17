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
    /// A color that follows the light or dark appearance.
    init(light: RGBA, dark: RGBA) {
        #if os(macOS)
        self.init(nsColor: NSColor(name: nil) { appearance in
            let c = appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua ? dark : light
            return NSColor(srgbRed: c.r, green: c.g, blue: c.b, alpha: c.a)
        })
        #else
        self.init(uiColor: UIColor { traits in
            let c = traits.userInterfaceStyle == .dark ? dark : light
            return UIColor(red: c.r, green: c.g, blue: c.b, alpha: c.a)
        })
        #endif
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
    /// Fraunces: titles and the one editorial line per screen.
    static func display(_ size: CGFloat, italic: Bool = false, relativeTo style: Font.TextStyle = .title) -> Font {
        .custom(italic ? "Fraunces-Italic" : "Fraunces-SemiBold", size: size, relativeTo: style)
    }

    /// Inter: everything the user reads to decide.
    static func text(_ size: CGFloat, weight: Font.Weight = .regular, relativeTo style: Font.TextStyle = .body) -> Font {
        .custom(Theme.Family.text, size: size, relativeTo: style).weight(weight)
    }

    /// IBM Plex Mono: release names and technical lines.
    static func mono(_ size: CGFloat) -> Font {
        .custom("IBMPlexMono-Regular", size: size, relativeTo: .caption)
    }
}

/// Layout numbers that depend on the device class, set once at the root.
struct Metrics: Sendable {
    let poster: CGSize
    let gutter: CGFloat
    let rowSpacing: CGFloat
    let wide: Bool

    static let tv = Metrics(poster: Theme.Poster.tv, gutter: Theme.safeArea.side, rowSpacing: Theme.Space.l, wide: true)
    static let regular = Metrics(poster: CGSize(width: 160, height: 240), gutter: Theme.Space.l, rowSpacing: Theme.Space.l, wide: true)
    static let compact = Metrics(poster: CGSize(width: 116, height: 174), gutter: 20, rowSpacing: Theme.Space.m + 4, wide: false)
}

extension EnvironmentValues {
    @Entry var metrics = Metrics.regular
}
