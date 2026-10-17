// Generated from design/tokens.json by apps/apple/scripts/gen-theme.ts. Do not edit.
import SwiftUI

enum Theme {
    /// Colors resolve per appearance: light and dark from the token file.
    enum Palette {
        static let bg = Color(light: RGBA(0.0235, 0.0353, 0.0549, 1), dark: RGBA(0.0235, 0.0353, 0.0549, 1))
        static let surface = Color(light: RGBA(0.0471, 0.0667, 0.0941, 1), dark: RGBA(0.0471, 0.0667, 0.0941, 1))
        static let raised = Color(light: RGBA(0.0784, 0.1059, 0.1412, 1), dark: RGBA(0.0784, 0.1059, 0.1412, 1))
        static let glass = Color(light: RGBA(0.0510, 0.0706, 0.0980, 0.58), dark: RGBA(0.0510, 0.0706, 0.0980, 0.58))
        static let glassStrong = Color(light: RGBA(0.0353, 0.0510, 0.0745, 0.84), dark: RGBA(0.0353, 0.0510, 0.0745, 0.84))
        static let edge = Color(light: RGBA(0.8431, 0.9098, 1.0000, 0.13), dark: RGBA(0.8431, 0.9098, 1.0000, 0.13))
        static let line = Color(light: RGBA(1.0000, 1.0000, 1.0000, 0.08), dark: RGBA(1.0000, 1.0000, 1.0000, 0.08))
        static let lineStrong = Color(light: RGBA(1.0000, 1.0000, 1.0000, 0.16), dark: RGBA(1.0000, 1.0000, 1.0000, 0.16))
        static let control = Color(light: RGBA(0.3843, 0.4157, 0.4510, 1), dark: RGBA(0.3843, 0.4157, 0.4510, 1))
        static let text = Color(light: RGBA(1.0000, 1.0000, 1.0000, 1), dark: RGBA(1.0000, 1.0000, 1.0000, 1))
        static let muted = Color(light: RGBA(0.8627, 0.8667, 0.8667, 1), dark: RGBA(0.8627, 0.8667, 0.8667, 1))
        static let dim = Color(light: RGBA(0.6667, 0.6706, 0.6784, 1), dark: RGBA(0.6667, 0.6706, 0.6784, 1))
        static let accent = Color(light: RGBA(0.8980, 0.2157, 0.1647, 1), dark: RGBA(0.8980, 0.2157, 0.1647, 1))
        static let onAccent = Color(light: RGBA(1.0000, 1.0000, 1.0000, 1), dark: RGBA(1.0000, 1.0000, 1.0000, 1))
        static let primary = Color(light: RGBA(1.0000, 1.0000, 1.0000, 1), dark: RGBA(1.0000, 1.0000, 1.0000, 1))
        static let onPrimary = Color(light: RGBA(0.0392, 0.0392, 0.0471, 1), dark: RGBA(0.0392, 0.0392, 0.0471, 1))
        static let star = Color(light: RGBA(0.9608, 0.7725, 0.0941, 1), dark: RGBA(0.9608, 0.7725, 0.0941, 1))
        static let halo = Color(light: RGBA(0.0000, 0.0000, 0.0000, 0.62), dark: RGBA(0.0000, 0.0000, 0.0000, 0.62))
        static let ok = Color(light: RGBA(0.4824, 0.8471, 0.5608, 1), dark: RGBA(0.4824, 0.8471, 0.5608, 1))
        static let warn = Color(light: RGBA(0.9608, 0.7725, 0.0941, 1), dark: RGBA(0.9608, 0.7725, 0.0941, 1))
        static let bad = Color(light: RGBA(1.0000, 0.4196, 0.3686, 1), dark: RGBA(1.0000, 0.4196, 0.3686, 1))
        static let scrim = Color(light: RGBA(0.0235, 0.0353, 0.0549, 0.72), dark: RGBA(0.0235, 0.0353, 0.0549, 0.72))
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
        static let m: CGFloat = 12
        static let card: CGFloat = 14
        static let l: CGFloat = 18
        static let hero: CGFloat = 22
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
        static let focus = Animation.timingCurve(0.22, 1, 0.36, 1, duration: 0.18)
        static let panel = Animation.timingCurve(0.22, 1, 0.36, 1, duration: 0.26)
    }

    enum Poster {
        static let tv = CGSize(width: 240, height: 360)
        static let landscape = CGSize(width: 384, height: 216)
    }

    enum Family {
        static let display = "Inter"
        static let text = "Inter"
        static let mono = "IBM Plex Mono"
    }
}
