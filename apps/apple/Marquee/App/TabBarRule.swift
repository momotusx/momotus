import SwiftUI

/// The iPhone bar shows icons only on a narrow screen or at large text, the
/// rule the Android bar uses (width under 360, font scale over 1.3). xxxLarge
/// is the first size past 1.3: its body text is 23 pt against the default 17.
func tabBarIconsOnly(width: CGFloat, type: DynamicTypeSize) -> Bool {
    width < 360 || type >= .xxxLarge
}

/// The iPhone capsule (apple-tv.md, Frame and navigation): 62 pt tall, its foot 21 pt above the
/// screen's bottom edge, whatever the home indicator's inset.
enum TabBarMetrics {
    static let height: CGFloat = 62
    static let foot: CGFloat = 21

    /// How far the capsule's foot sits from the safe area's bottom: below it (an offset down) on a
    /// screen with a home indicator, above it (padding) on one without.
    static func drop(bottomInset: CGFloat) -> CGFloat { bottomInset - foot }

    /// The bottom margin every scroll view in the places keeps for the bar: the capsule's reach
    /// above the safe area's bottom. None under the player: the bar is away, and the margin had
    /// grown the actions' scroll row and lifted the scrubber off the foot (the iPhone walk, round 3).
    static func scrollMargin(immersive: Bool, bottomInset: CGFloat) -> CGFloat {
        immersive ? 0 : max(0, height + foot - bottomInset)
    }
}
