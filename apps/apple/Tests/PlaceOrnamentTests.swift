import AppKit
import SwiftUI
import Testing

// The Mac's ornament (the owner's variant 4, plans/owner-picks.md 2026-10-25), laid out for real: the five
// places with no wordmark, inside the narrowest window, centred at any width, and a page's last row ending
// above it.

@MainActor private final class Frames {
    var ornament = CGRect.zero
    var lastRow = CGRect.zero
}

@MainActor @Test func theOrnamentHoldsThePlacesButSettingsAndNoWordmark() {
    // Settings opens from the profile's menu at the top right (plans/nav-profile.md).
    #expect(OrnamentLayout.places == [.home, .discover, .library, .calendar])
    #expect(OrnamentLayout.height == 44)
}

@MainActor @Test func theOrnamentFitsTheNarrowestWindow() {
    // With the profile at its end, as the shell draws it (a stand-in of the avatar item's size).
    let profile = Color.clear.frame(width: OrnamentLayout.itemHeight + 8, height: OrnamentLayout.itemHeight)
    let size = NSHostingView(rootView: PlaceOrnament(selection: .home, select: { _ in }, profile: profile)).fittingSize
    #expect(abs(size.height - OrnamentLayout.height) < 1, "the ornament is \(size)")
    // 16 pt clear of each side of the narrowest window the app allows.
    #expect(size.width + 32 <= BackdropStops.macMinWidth, "the ornament is \(size.width) wide")
}

/// A page of rows scrolled to its end under the shell's ornament, in a `width` by 700 window.
@MainActor private func scrolledToTheEnd(width: CGFloat) -> Frames {
    let frames = Frames()
    let page = ScrollView {
        VStack(spacing: 20) {
            ForEach(0..<12, id: \.self) { i in
                Color.gray.frame(height: 120)
                    .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { if i == 11 { frames.lastRow = $0 } }
            }
        }
        .frame(maxWidth: .infinity)
    }
    .defaultScrollAnchor(.bottom)
    .placeOrnament(.library, strong: true, shown: true, placed: { frames.ornament = $0 }) { _ in }
    let host = NSHostingView(rootView: page.frame(width: width, height: 700))
    host.frame = CGRect(x: 0, y: 0, width: width, height: 700)
    host.layoutSubtreeIfNeeded()
    RunLoop.main.run(until: Date().addingTimeInterval(0.3))
    return frames
}

@MainActor @Test func aPagesLastRowEndsAboveTheOrnamentAtAnyWidth() {
    for width in [BackdropStops.macMinWidth, 1100, 1440] {
        let frames = scrolledToTheEnd(width: width)
        #expect(frames.lastRow.height == 120, "the rows were not laid out at \(width): \(frames.lastRow)")
        #expect(frames.ornament.height > 0, "the ornament was not laid out at \(width)")
        #expect(frames.lastRow.maxY <= frames.ornament.minY, "at \(width) the last row \(frames.lastRow) runs under the ornament \(frames.ornament)")
    }
}

@MainActor @Test func theOrnamentStaysCentredAtTheFootAsTheWindowResizes() {
    let frames = Frames()
    let host = NSHostingView(rootView: Color.clear.placeOrnament(.home, strong: false, shown: true, placed: { frames.ornament = $0 }) { _ in })
    for width in [BackdropStops.macMinWidth, 1000, 1600] {
        host.frame = CGRect(x: 0, y: 0, width: width, height: 800)
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.2))
        #expect(abs(frames.ornament.midX - width / 2) < 1, "at \(width) the ornament's centre is \(frames.ornament.midX)")
        #expect(abs(frames.ornament.maxY - (800 - OrnamentLayout.gap)) < 1, "at \(width) the ornament's foot is \(frames.ornament.maxY)")
    }
}

@MainActor @Test func thePlayerHasNoOrnament() {
    let frames = Frames()
    let host = NSHostingView(rootView: Color.clear.placeOrnament(.home, strong: false, shown: false, placed: { frames.ornament = $0 }) { _ in })
    host.frame = CGRect(x: 0, y: 0, width: 1000, height: 800)
    host.layoutSubtreeIfNeeded()
    RunLoop.main.run(until: Date().addingTimeInterval(0.2))
    #expect(frames.ornament == .zero)
}
