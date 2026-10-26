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

@MainActor @Test func wordsTallerThanTheHeroGrowItUnderTheTitle() {
    // A landscape phone at a large text size: a 300 pt column in a 280 pt hero.
    let frames = Frames()
    let hero = Color.clear.frame(width: 350, height: 300)
        .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { frames.words = $0 }
        .heroFrame(height: 280, alignment: .bottom) {
            Text("Home").font(.system(size: 34, weight: .bold))
                .padding(.leading, 16).padding(.top, 2)
                .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { frames.title = $0 }
        }
    let host = NSHostingView(rootView: hero.frame(width: 390).fixedSize(horizontal: false, vertical: true))
    host.frame = CGRect(x: 0, y: 0, width: 390, height: 600)
    host.layoutSubtreeIfNeeded()
    RunLoop.main.run(until: Date().addingTimeInterval(0.2))
    #expect(frames.words.height == 300, "the words were not laid out: \(frames.words)")
    #expect(frames.title.maxY <= frames.words.minY, "the title \(frames.title) is on the words \(frames.words)")
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
    // Home's Details and synopsis on the narrowest Mac window in the hero's 400 pt floor, and a wide 1440 hero.
    let narrow = CGSize(width: BackdropStops.macMinWidth, height: 400)
    for (size, point) in [(narrow, CGPoint(x: 271, y: 400 - 104.5)), (narrow, CGPoint(x: 520, y: 400 - 208)),
                          (CGSize(width: 1440, height: 810), CGPoint(x: 271, y: 810 - 104.5)),
                          (CGSize(width: 1440, height: 810), CGPoint(x: 900, y: 200))] {
        let box = BackdropStops.rampBox(size, held: true)
        let wash = BackdropStops.alpha(BackdropStops.heroSide, at: BackdropStops.washT(x: point.x, y: point.y, size: size, box: box))
        let vignette = BackdropStops.vignetteAlpha(x: point.x / size.width, y: point.y / size.height)
        #expect(abs(drawnAlpha(BackdropStops.wash(size, box: box), size: size, at: point) - wash) < 0.02, "wash at \(point) in \(size)")
        #expect(abs(drawnAlpha(BackdropStops.vignetteGradient, size: size, at: point) - vignette) < 0.02, "vignette at \(point) in \(size)")
    }
}

@MainActor @Test func heroScrimsDrawWhatTheGlassCheckReads() {
    // The view HeroView lays under its words: wide, the three ramps in the frame's box; the phone,
    // its foot held in points (black pages, so the layers' alphas compose).
    let narrow = CGSize(width: BackdropStops.macMinWidth, height: 400)
    let box = BackdropStops.rampBox(narrow, held: true)
    for point in [CGPoint(x: 271, y: 400 - 104.5), CGPoint(x: 520, y: 400 - 208)] {
        let parts = [BackdropStops.vignetteAlpha(x: point.x / narrow.width, y: point.y / narrow.height),
                     BackdropStops.alpha(BackdropStops.heroSide, at: BackdropStops.washT(x: point.x, y: point.y, size: narrow, box: box)),
                     BackdropStops.alpha(BackdropStops.heroFoot(narrow, box: box), at: (narrow.height - point.y) / narrow.height)]
        let expected = parts.reduce(0) { BackdropStops.covered($0, $1) }
        let drawn = drawnAlpha(HeroScrims(wide: true, page: .black), size: narrow, at: point)
        #expect(abs(drawn - expected) < 0.03, "wide scrims at \(point): \(drawn), the check \(expected)")
    }
    let landscape = CGSize(width: 844, height: 323)
    let upright = BackdropStops.alpha(BackdropStops.heroPhoneFoot, at: 104.0 / BackdropStops.heroPhoneHeight)
    let drawn = drawnAlpha(HeroScrims(wide: false, page: .black), size: landscape, at: CGPoint(x: 422, y: 323 - 104.5))
    #expect(abs(drawn - upright) < 0.02, "phone scrims in landscape: \(drawn), upright \(upright)")
}

@MainActor @Test func theCardRampIsDrawnWhereTheGlassCheckReadsIt() {
    // The card spots' heights above the foot (ThemeColorTests): the tag, name, overview and caption lines.
    for (size, heights) in [(CGSize(width: 256, height: 141), [98.0, 83, 63, 27]), (CGSize(width: 239, height: 170), [122.0, 107, 85])] {
        for h in heights {
            let drawn = drawnAlpha(BackdropStops.cardRampGradient, size: size, at: CGPoint(x: size.width / 2, y: size.height - h - 0.5))
            let checked = BackdropStops.alpha(BackdropStops.cardRamp, at: h / size.height)
            #expect(abs(drawn - checked) < 0.02, "the ramp at \(h) in \(size) draws \(drawn), the check reads \(checked)")
        }
    }
}

@MainActor @Test func theWideHerosPlayIsAsWideAsItsWidestLabelWhateverLands() {
    // The lead, after review r4: on the Mac (and any wide hero) Details never moves when Play's words change.
    func width(_ text: String, wide: Bool) -> CGFloat {
        let pill = Button {} label: { HeroPlayLabel(text: text, wide: wide) }.buttonStyle(PillButtonStyle(prominent: true))
        let host = NSHostingView(rootView: pill.fixedSize())
        host.layoutSubtreeIfNeeded()
        return host.fittingSize.width
    }
    let widths = ["Play", "Resume S2:E3", "Play again", "Play S1:E1"].map { width($0, wide: true) }
    #expect(Set(widths).count == 1, "wide pills \(widths)")
    for word in HeroPlayLabel.widest { #expect(width(word, wide: true) == widths[0], "\(word) is wider") }
    // The words are measured for real: unheld, a short label makes a narrower pill.
    #expect(width("Play", wide: false) < width("Resume S12:E10", wide: false))
    // Review r5, R5-3: a page's label longer than the constants widens every page's Play to it.
    let long = "Resume S1:E1122"
    #expect(width(long, wide: true) > widths[0])
    func held(_ text: String) -> CGFloat {
        let pill = Button {} label: { HeroPlayLabel(text: text, wide: true, pages: [long, "Play S1:E1"]) }
            .buttonStyle(PillButtonStyle(prominent: true))
        let host = NSHostingView(rootView: pill.fixedSize())
        host.layoutSubtreeIfNeeded()
        return host.fittingSize.width
    }
    #expect(Set(["Play", "Play S1:E1", long].map(held)) == [width(long, wide: true)])
}
