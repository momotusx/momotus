import SwiftUI

extension View {
    /// The hero's box: its words held to the foot of `height`, then `top` (the iPhone's large
    /// title) on the stretched box's top edge. `top` hangs on the box, never on the words' column,
    /// where it would sit on the logo (HeroFrameTests).
    func heroFrame<Top: View>(height: CGFloat, alignment: Alignment, @ViewBuilder top: () -> Top) -> some View {
        frame(maxWidth: .infinity, minHeight: height, alignment: alignment)
            .overlay(alignment: .topLeading, content: top)
    }
}
