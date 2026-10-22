// Generated from design/tokens.json by apps/apple/scripts/gen-theme.ts. Do not edit.
import SwiftUI

enum Theme {
    /// The one palette: Marquee is dark only.
    enum Palette {
        #if os(iOS)
        static let bg = Color(RGBA(0.0000, 0.0000, 0.0000, 1))
        #elseif os(macOS)
        static let bg = Color(RGBA(0.1216, 0.1216, 0.1216, 1))
        #else
        static let bg = Color(RGBA(0.1098, 0.1098, 0.1176, 1))
        #endif
        #if os(iOS)
        static let surface = Color(RGBA(0.1098, 0.1098, 0.1176, 1))
        #else
        static let surface = Color(RGBA(0.1725, 0.1725, 0.1804, 1))
        #endif
        #if os(iOS)
        static let raised = Color(RGBA(0.1725, 0.1725, 0.1804, 1))
        #else
        static let raised = Color(RGBA(0.2275, 0.2275, 0.2353, 1))
        #endif
        static let glass = Color(RGBA(0.1490, 0.1490, 0.1569, 0.6))
        static let glassStrong = Color(RGBA(0.1176, 0.1176, 0.1255, 0.9))
        static let edge = Color(RGBA(1.0000, 1.0000, 1.0000, 0.1))
        static let line = Color(RGBA(1.0000, 1.0000, 1.0000, 0.1))
        static let lineStrong = Color(RGBA(1.0000, 1.0000, 1.0000, 0.25))
        static let control = Color(RGBA(0.4235, 0.4235, 0.4392, 1))
        static let text = Color(RGBA(1.0000, 1.0000, 1.0000, 0.92))
        static let muted = Color(RGBA(1.0000, 1.0000, 1.0000, 0.7))
        #if os(iOS)
        static let dim = Color(RGBA(0.9216, 0.9216, 0.9608, 0.6))
        #else
        static let dim = Color(RGBA(1.0000, 1.0000, 1.0000, 0.64))
        #endif
        static let accent = Color(RGBA(1.0000, 1.0000, 1.0000, 1))
        static let onAccent = Color(RGBA(0.0000, 0.0000, 0.0000, 0.88))
        #if os(iOS)
        static let primary = Color(RGBA(0.9608, 0.9608, 0.9608, 1))
        #else
        static let primary = Color(RGBA(1.0000, 1.0000, 1.0000, 0.92))
        #endif
        static let onPrimary = Color(RGBA(0.0000, 0.0000, 0.0000, 0.88))
        static let star = Color(RGBA(1.0000, 1.0000, 1.0000, 0.92))
        static let halo = Color(RGBA(0.0000, 0.0000, 0.0000, 0.6))
        static let ok = Color(RGBA(1.0000, 1.0000, 1.0000, 0.92))
        static let warn = Color(RGBA(1.0000, 1.0000, 1.0000, 0.7))
        static let bad = Color(RGBA(1.0000, 1.0000, 1.0000, 0.92))
        static let scrim = Color(RGBA(0.0000, 0.0000, 0.0000, 0.45))
        static let avatarEmber = Color(RGBA(0.3882, 0.3882, 0.4000, 1))
        static let avatarAmber = Color(RGBA(0.3529, 0.3529, 0.3686, 1))
        static let avatarMoss = Color(RGBA(0.3294, 0.3294, 0.3373, 1))
        static let avatarTeal = Color(RGBA(0.2824, 0.2824, 0.2902, 1))
        static let avatarSky = Color(RGBA(0.2980, 0.2980, 0.3137, 1))
        static let avatarPlum = Color(RGBA(0.2667, 0.2667, 0.2745, 1))
        static let avatarRose = Color(RGBA(0.2275, 0.2275, 0.2353, 1))
        static let avatarGraphite = Color(RGBA(0.2118, 0.2118, 0.2196, 1))
        static let key = Color(RGBA(0.0392, 0.5176, 1.0000, 1))
        static let keyFill = Color(RGBA(0.0000, 0.4431, 0.8902, 1))
        static let onKey = Color(RGBA(1.0000, 1.0000, 1.0000, 1))
        static let switchOn = Color(RGBA(0.1882, 0.8196, 0.3451, 1))
        static let bgPhone = Color(RGBA(0.0000, 0.0000, 0.0000, 1))
        static let surfacePhone = Color(RGBA(0.1098, 0.1098, 0.1176, 1))
        static let raisedPhone = Color(RGBA(0.1725, 0.1725, 0.1804, 1))
        static let dimPhone = Color(RGBA(0.9216, 0.9216, 0.9608, 0.6))
        static let primaryPhone = Color(RGBA(0.9608, 0.9608, 0.9608, 1))
        static let bgDesktop = Color(RGBA(0.1216, 0.1216, 0.1216, 1))
        static let barDesktop = Color(RGBA(0.0000, 0.0000, 0.0000, 1))
        static let bandDesktop = Color(RGBA(0.1686, 0.1686, 0.1686, 1))
        /// A selected segment or chip: one grey step up (`selected.fill`).
        static let selectedFill = Color(RGBA(0.3882, 0.3882, 0.4000, 1))
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
    static let safeAreaPhone = Insets(top: 12, bottom: 12, side: 20)

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
        static let l: CGFloat = 20
        static let hero: CGFloat = 20
        static let pill: CGFloat = 999
    }

    enum Focus {
        static let ring: CGFloat = 4
        /// A soft dark halo this wide around the ring.
        static let halo: CGFloat = 30
        static let scaleCard: CGFloat = 1.06
        static let scaleTile: CGFloat = 1.03
    }

    enum Motion {
        static let focus = Animation.timingCurve(0.22, 1, 0.36, 1, duration: 0.18)
        static let panel = Animation.timingCurve(0.22, 1, 0.36, 1, duration: 0.26)
    }

    enum Poster {
        static let tv = CGSize(width: 217, height: 326)
        static let landscape = CGSize(width: 410, height: 231)
    }

    /// The phone's tiles (`tile.phone`), read by `Metrics.compact`.
    enum Tile {
        static let poster = CGSize(width: 99, height: 149)
        static let landscape = CGSize(width: 239, height: 134)
        static let gap: CGFloat = 12
    }

    /// The Mac's shelves (`tile.desktop`, tv.apple.com's), for `Metrics.regular`.
    enum TileDesktop {
        static let poster = CGSize(width: 153, height: 228)
        static let landscape = CGSize(width: 256, height: 144)
        static let gap: CGFloat = 20
        static let inset: CGFloat = 40
    }

    /// Font weights, 100 to 900.
    enum Weight {
        static let display: CGFloat = 700
        static let heading: CGFloat = 700
        static let label: CGFloat = 600
        static let body: CGFloat = 400
    }

    /// Letter spacing in em: multiply by the font size.
    enum Tracking {
        static let display: CGFloat = -0.022
        static let heading: CGFloat = -0.019
        static let label: CGFloat = -0.011
    }

    enum Family {
        static let display = "Inter"
        static let text = "Inter"
        static let mono = "IBM Plex Mono"
    }
}
