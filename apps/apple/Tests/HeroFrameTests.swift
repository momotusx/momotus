import AppKit
import SwiftUI
import Testing

// The hero's box, laid out and drawn for real: the iPhone's large title at the hero's top (not on
// the logo, phase 2 review A, P1-1), and the wash and vignette drawn where the glass check reads them.

@MainActor private final class Frames {
    var title = CGRect.zero
    var words = CGRect.zero
}

@MainActor @Test func theLargeTitleSitsAtTheHeroTopNotOnTheWords() {
    let frames = Frames()
    // The iPhone's column (logo to buttons, about 262 pt) at the foot of its 633 pt hero.
    let hero = Color.clear.frame(width: 350, height: 262)
        .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { frames.words = $0 }
        .heroFrame(height: 633, alignment: .bottom) {
            Text("Home").font(.system(size: 34, weight: .bold))
                .padding(.leading, 16).padding(.top, 2)
                .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { frames.title = $0 }
        }
    let host = NSHostingView(rootView: hero.frame(width: 390))
    host.frame = CGRect(x: 0, y: 0, width: 390, height: 633)
    host.layoutSubtreeIfNeeded()
    RunLoop.main.run(until: Date().addingTimeInterval(0.2))
    #expect(frames.words.height == 262, "the words were not laid out: \(frames.words)")
    #expect(frames.title.minY < 10, "the title sits at \(frames.title)")
    #expect(frames.title.maxY < frames.words.minY, "the title \(frames.title) is on the words \(frames.words)")
}

/// The alpha a view draws at a point, rendered at 1x in a `size` box.
@MainActor private func drawnAlpha(_ view: some View, size: CGSize, at point: CGPoint) -> Double {
    let renderer = ImageRenderer(content: view.frame(width: size.width, height: size.height))
    renderer.scale = 1
    guard let image = renderer.cgImage, let data = image.dataProvider?.data, let bytes = CFDataGetBytePtr(data) else { return -1 }
    let offset = Int(point.y) * image.bytesPerRow + Int(point.x) * image.bitsPerPixel / 8
    return Double(bytes[offset + 3]) / 255
}

@MainActor @Test func thePhoneFootHoldsInPointsFromTheFoot() {
    // Details, 104 pt above the foot, gets the portrait hero's darkness in a landscape hero too.
    let upright = BackdropStops.alpha(BackdropStops.heroPhoneFoot, at: 104.0 / BackdropStops.heroPhoneHeight)
    for size in [CGSize(width: 390, height: 680), CGSize(width: 844, height: 323), CGSize(width: 667, height: 280)] {
        let drawn = drawnAlpha(BackdropStops.phoneFoot(size, .black), size: size, at: CGPoint(x: size.width / 2, y: size.height - 104.5))
        #expect(abs(drawn - upright) < 0.02, "the foot at 104 pt in \(size) draws \(drawn), upright \(upright)")
    }
}

@MainActor @Test func theWashAndTheVignetteAreDrawnWhereTheGlassCheckReadsThem() {
    // Home's Details on the narrowest Mac window, in the hero's 400 pt floor, and a wide 1440 hero.
    for (size, point) in [(CGSize(width: BackdropStops.macMinWidth, height: 400), CGPoint(x: 271, y: 400 - 104.5)),
                          (CGSize(width: 1440, height: 810), CGPoint(x: 271, y: 810 - 104.5)),
                          (CGSize(width: 1440, height: 810), CGPoint(x: 900, y: 200))] {
        let wash = BackdropStops.alpha(BackdropStops.heroSide, at: BackdropStops.washT(x: point.x, y: point.y, width: size.width, height: size.height))
        let vignette = BackdropStops.vignetteAlpha(x: point.x / size.width, y: point.y / size.height)
        #expect(abs(drawnAlpha(BackdropStops.wash(size), size: size, at: point) - wash) < 0.02, "wash at \(point) in \(size)")
        #expect(abs(drawnAlpha(BackdropStops.vignetteGradient, size: size, at: point) - vignette) < 0.02, "vignette at \(point) in \(size)")
    }
}
