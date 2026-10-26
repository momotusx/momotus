import SwiftUI

/// The wide hero's metrics, shared by Home's hero and the title page's: tv.apple.com's sizes on the
/// Mac, the tvOS scale on TV.
enum HeroLayout {
    #if os(tvOS)
    static let gap: CGFloat = 12
    static let logoWidth: CGFloat = 560
    static let logoHeight: CGFloat = 140
    static let genre = Theme.type.callout
    static let meta = Theme.type.caption
    static let reasonSize = Theme.type.caption
    static let synopsisWidth: CGFloat = 820
    static let foot: CGFloat = 75
    static let icon = Theme.type.body + 1
    #else
    static let gap: CGFloat = 9
    static let logoWidth: CGFloat = 400
    static let logoHeight: CGFloat = 81
    static let genre: CGFloat = 15
    static let meta: CGFloat = 13
    static let reasonSize: CGFloat = 13
    static let synopsisWidth = BackdropStops.heroSynopsisWidth
    static let foot: CGFloat = 75
    #if os(macOS)
    // The desktop frames' 16 px icon in the 40 pt pill; the iPhone keeps its 18.
    static let icon: CGFloat = 16
    #else
    static let icon = Theme.type.body + 1
    #endif
    #endif
}

/// The hero's Play words. Wide (the Mac, the iPad, TV) the pill is as wide as the widest label the core gives the
/// hero, so Details never moves whatever label lands; the iPhone's pill is a fixed 178 and shrinks the words instead.
struct HeroPlayLabel: View {
    let text: String
    let wide: Bool
    /// The widest labels the hero's Play can show: a resume, a start, a series watched to its end.
    static let widest = ["Resume S12:E10", "Play S12:E10", "Play again"]

    var body: some View {
        Group {
            if wide {
                ZStack {
                    ForEach(Self.widest, id: \.self) { word in
                        Label(word, glyph: "play-fill", size: HeroLayout.icon).hidden().accessibilityHidden(true)
                    }
                    Label(text, glyph: "play-fill", size: HeroLayout.icon)
                }
            } else {
                Label(text, glyph: "play-fill", size: HeroLayout.icon).frame(maxWidth: .infinity)
            }
        }
        // A large text size shrinks the words rather than cut them.
        .lineLimit(1)
        .minimumScaleFactor(0.7)
    }
}
