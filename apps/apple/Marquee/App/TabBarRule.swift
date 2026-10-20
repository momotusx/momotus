import SwiftUI

/// The iPhone bar shows icons only on a narrow screen or at large text, the
/// rule the Android bar uses (width under 360, font scale over 1.3). xxxLarge
/// is the first size past 1.3: its body text is 23 pt against the default 17.
func tabBarIconsOnly(width: CGFloat, type: DynamicTypeSize) -> Bool {
    width < 360 || type >= .xxxLarge
}

/// The iPhone bar's height with its gap below, for the room a pushed page keeps at its end.
enum TabBarMetrics {
    static let height: CGFloat = 66

    /// The bottom margin every scroll view in the places keeps for the bar.
    /// None under the player: the bar is away, and the margin had grown the
    /// actions' scroll row by 66 pt and lifted the scrubber off the foot
    /// (the iPhone walk, round 3).
    static func scrollMargin(immersive: Bool) -> CGFloat {
        immersive ? 0 : height
    }
}
