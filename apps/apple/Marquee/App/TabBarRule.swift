import SwiftUI

/// The iPhone bar shows icons only on a narrow screen or at large text, the
/// rule the Android bar uses (width under 360, font scale over 1.3). xxxLarge
/// is the first size past 1.3: its body text is 23 pt against the default 17.
func tabBarIconsOnly(width: CGFloat, type: DynamicTypeSize) -> Bool {
    width < 360 || type >= .xxxLarge
}
