// Generated from design/tokens.json by apps/apple/scripts/gen-theme.ts. Do not edit.
import SwiftUI

enum Theme {
    /// Colors resolve per appearance: light and dark from the token file.
    enum Palette {
        static let bg = Color(light: RGBA(0.9608, 0.9373, 0.9020, 1), dark: RGBA(0.0588, 0.0510, 0.0431, 1))
        static let surface = Color(light: RGBA(0.9216, 0.8902, 0.8431, 1), dark: RGBA(0.1059, 0.0941, 0.0824, 1))
        static let raised = Color(light: RGBA(0.8784, 0.8353, 0.7765, 1), dark: RGBA(0.1647, 0.1451, 0.1216, 1))
        static let line = Color(light: RGBA(0.8118, 0.7608, 0.6902, 1), dark: RGBA(0.2392, 0.2118, 0.1804, 1))
        static let control = Color(light: RGBA(0.5490, 0.4902, 0.4157, 1), dark: RGBA(0.4627, 0.4157, 0.3569, 1))
        static let text = Color(light: RGBA(0.1098, 0.0863, 0.0667, 1), dark: RGBA(0.9647, 0.9373, 0.9020, 1))
        static let muted = Color(light: RGBA(0.3725, 0.3255, 0.2706, 1), dark: RGBA(0.7216, 0.6706, 0.6078, 1))
        static let accent = Color(light: RGBA(0.6118, 0.3451, 0.0000, 1), dark: RGBA(1.0000, 0.7098, 0.2784, 1))
        static let onAccent = Color(light: RGBA(1.0000, 0.9725, 0.9333, 1), dark: RGBA(0.1020, 0.0706, 0.0235, 1))
        static let halo = Color(light: RGBA(0.1098, 0.0863, 0.0667, 0.82), dark: RGBA(0.0471, 0.0353, 0.0235, 0.62))
        static let ok = Color(light: RGBA(0.1804, 0.4902, 0.2706, 1), dark: RGBA(0.4824, 0.8471, 0.5608, 1))
        static let warn = Color(light: RGBA(0.6118, 0.3451, 0.0000, 1), dark: RGBA(1.0000, 0.7098, 0.2784, 1))
        static let bad = Color(light: RGBA(0.7020, 0.1490, 0.1176, 1), dark: RGBA(1.0000, 0.4784, 0.4196, 1))
        static let scrim = Color(light: RGBA(0.9608, 0.9373, 0.9020, 0.78), dark: RGBA(0.0588, 0.0510, 0.0431, 0.72))
    }

    struct TypeScale: Sendable {
        let hero: CGFloat
        let title1: CGFloat
        let title2: CGFloat
        let title3: CGFloat
        let headline: CGFloat
        let body: CGFloat
        let callout: CGFloat
        let caption: CGFloat
    }

    static let typeTV = TypeScale(hero: 76, title1: 57, title2: 48, title3: 38, headline: 31, body: 29, callout: 25, caption: 23)
    static let typePhone = TypeScale(hero: 34, title1: 28, title2: 22, title3: 20, headline: 17, body: 17, callout: 15, caption: 12)

    struct Insets: Sendable {
        let top: CGFloat
        let bottom: CGFloat
        let side: CGFloat
    }

    static let safeAreaTV = Insets(top: 60, bottom: 60, side: 80)
    static let safeAreaPhone = Insets(top: 12, bottom: 12, side: 16)

    #if os(tvOS)
    static let type = typeTV
    static let safeArea = safeAreaTV
    #else
    static let type = typePhone
    static let safeArea = safeAreaPhone
    #endif

    enum Space {
        static let xs: CGFloat = 8
        static let s: CGFloat = 16
        static let m: CGFloat = 24
        static let l: CGFloat = 40
        static let xl: CGFloat = 64
        static let xxl: CGFloat = 96
    }

    enum Radius {
        static let s: CGFloat = 8
        static let m: CGFloat = 14
        static let l: CGFloat = 22
        static let pill: CGFloat = 999
    }

    enum Focus {
        static let ring: CGFloat = 4
        /// Dark theme: a soft halo this wide. Light theme: a crisp outer line this wide.
        static let haloDark: CGFloat = 14
        static let haloLight: CGFloat = 3
        static let scaleCard: CGFloat = 1.05
        static let scaleTile: CGFloat = 1.025
    }

    enum Motion {
        static let focus = Animation.timingCurve(0.2, 0.8, 0.2, 1, duration: 0.18)
        static let panel = Animation.timingCurve(0.2, 0.8, 0.2, 1, duration: 0.32)
    }

    enum Poster {
        static let tv = CGSize(width: 240, height: 360)
        static let landscape = CGSize(width: 384, height: 216)
    }

    enum Family {
        static let display = "Fraunces"
        static let text = "Inter"
        static let mono = "IBM Plex Mono"
    }
}
