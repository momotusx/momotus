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
