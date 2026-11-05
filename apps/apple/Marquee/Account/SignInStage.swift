#if os(macOS)
import SwiftUI

/// The Mac's sign-in stage (variant 4, the owner's pick of 2026-11-04, design/exploration/desktop-signin), laid
/// out over the first run's still: the step at the lower left on the first run's inset, the phone's QR at the
/// lower right, where tv.apple.com puts Starring. A window too narrow for both puts the QR under the step; a step
/// taller than the window scrolls.
struct SignInStage<Block: View, Corner: View>: View {
    let block: Block
    let corner: Corner
    /// Where the step and the corner land, in the window (Tests/SignInStageTests.swift).
    var placed: ((_ block: CGRect, _ corner: CGRect) -> Void)?
    @State private var frames = (block: CGRect.zero, corner: CGRect.zero)

    init(placed: ((_ block: CGRect, _ corner: CGRect) -> Void)? = nil, @ViewBuilder block: () -> Block, @ViewBuilder corner: () -> Corner) {
        self.placed = placed
        self.block = block()
        self.corner = corner()
    }

    /// The step's widest, the corner's width, and the insets from the window's edges.
    static var blockWidth: CGFloat { 760 }
    static var cornerWidth: CGFloat { 420 }
    static var leading: CGFloat { FilmRules.inset }
    static var trailing: CGFloat { 56 }
    static var bottom: CGFloat { 84 }
    /// The corner sits beside the step from this window width (the step keeps 600 of its width), under it below.
    static var besideFrom: CGFloat { leading + 600 + Theme.Space.l + cornerWidth + trailing }

    var body: some View {
        GeometryReader { page in
            ScrollView(.vertical) {
                Group {
                    if page.size.width >= Self.besideFrom {
                        HStack(alignment: .bottom, spacing: Theme.Space.l) {
                            measuredBlock
                            Spacer(minLength: 0)
                            measuredCorner
                        }
                    } else {
                        VStack(alignment: .leading, spacing: Theme.Space.l) {
                            measuredBlock
                            measuredCorner
                        }
                    }
                }
                .padding(.leading, Self.leading)
                .padding(.trailing, Self.trailing)
                .padding(.bottom, Self.bottom)
                .padding(.top, Theme.Space.l)
                .frame(maxWidth: .infinity, minHeight: page.size.height, alignment: .bottomLeading)
            }
            .scrollIndicators(.hidden)
        }
    }

    private var measuredBlock: some View {
        block
            .frame(maxWidth: Self.blockWidth, alignment: .leading)
            .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { frames.block = $0; placed?(frames.block, frames.corner) }
    }

    private var measuredCorner: some View {
        // The corner sizes itself to `cornerWidth`: a step without one leaves no room for it.
        corner
            .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { frames.corner = $0; placed?(frames.block, frames.corner) }
    }
}
#endif
