#if !os(tvOS)
import SwiftUI

/// The Login sheet's two columns on wide forms (round2.html combo A, D10: the Mac and iPad at regular width, first run
/// and Settings' Sign in alike): the ways in on the left, the phone's live QR on the right, 81 pt apart. A sheet too
/// narrow for both (the Mac's smallest window) puts the QR under the ways.
struct SignInStage<Block: View, Corner: View>: View {
    let block: Block
    let corner: Corner
    /// Where the ways and the QR land, in the window (Tests/SignInStageTests.swift).
    var placed: ((_ block: CGRect, _ corner: CGRect) -> Void)?
    @State private var frames = (block: CGRect.zero, corner: CGRect.zero)

    init(placed: ((_ block: CGRect, _ corner: CGRect) -> Void)? = nil, @ViewBuilder block: () -> Block, @ViewBuilder corner: () -> Corner) {
        self.placed = placed
        self.block = block()
        self.corner = corner()
    }

    /// The QR column's width, the gap on each side of its line, and the ways' narrowest beside it.
    static var cornerWidth: CGFloat { 260 }
    static var gap: CGFloat { 40 }
    static var blockMin: CGFloat { 300 }
    /// The columns sit side by side from this width, the QR under the ways below it.
    static var besideFrom: CGFloat { blockMin + gap * 2 + 1 + cornerWidth }

    var body: some View {
        Columns(besideFrom: Self.besideFrom, gap: Self.gap) {
            block
                .frame(maxWidth: .infinity, alignment: .leading)
                .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { frames.block = $0; placed?(frames.block, frames.corner) }
            corner
                .frame(width: Self.cornerWidth, alignment: .leading)
                .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { frames.corner = $0; placed?(frames.block, frames.corner) }
        }
    }
}

/// Two views beside each other (the second at its own width, a 1 pt line between, `gap` each side of it) or, below
/// `besideFrom`, the second under the first.
private struct Columns: Layout {
    let besideFrom: CGFloat
    let gap: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? besideFrom
        let (first, second) = sizes(width, subviews)
        return beside(width)
            ? CGSize(width: width, height: max(first.height, second.height))
            : CGSize(width: width, height: first.height + gap + second.height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        guard subviews.count == 2 else { return }
        let (first, second) = sizes(bounds.width, subviews)
        if beside(bounds.width) {
            subviews[0].place(at: bounds.origin, proposal: ProposedViewSize(width: first.width, height: nil))
            subviews[1].place(at: CGPoint(x: bounds.maxX - second.width, y: bounds.minY), proposal: ProposedViewSize(second))
        } else {
            subviews[0].place(at: bounds.origin, proposal: ProposedViewSize(width: bounds.width, height: nil))
            subviews[1].place(at: CGPoint(x: bounds.minX, y: bounds.minY + first.height + gap), proposal: ProposedViewSize(second))
        }
    }

    private func beside(_ width: CGFloat) -> Bool { width >= besideFrom }

    private func sizes(_ width: CGFloat, _ subviews: Subviews) -> (CGSize, CGSize) {
        guard subviews.count == 2 else { return (.zero, .zero) }
        let second = subviews[1].sizeThatFits(.unspecified)
        let firstWidth = beside(width) ? width - second.width - gap * 2 - 1 : width
        let first = subviews[0].sizeThatFits(ProposedViewSize(width: firstWidth, height: nil))
        return (CGSize(width: firstWidth, height: first.height), second)
    }
}
#endif
