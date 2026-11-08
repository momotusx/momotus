import Foundation
import SwiftUI
import Testing

// First run V1, Full Bleed (the owner's pick of 2026-10-30, design/exploration/onboarding-v2, design/DECISION.md):
// the phone's ramp anchored to the step's block, the wide wash and column, every lead word at 4.5:1 over a white
// still, the step count and Back, and the still asked once.

/// The contrast of white words at `text` opacity over a white still under black at `cover` (WCAG 2.2).
private func contrast(_ text: Double, over cover: Double) -> Double {
    func lin(_ v: Double) -> Double { v <= 0.04045 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4) }
    let back = 1 - cover
    let words = text + (1 - text) * back
    return (lin(words) + 0.05) / (lin(back) + 0.05)
}

/// The lead line's white, `Theme.Palette.muted`.
private let muted = 0.7

@Test func thePhoneRampIsClearAboveTheQuestionAndBlackUnderIt() {
    // A 420 pt block (the doors at the foot) under the ramp's 170 over the question.
    let height = FilmRules.above + 420
    let stops = FilmRules.ramp(height: height)
    func at(_ y: CGFloat) -> Double { BackdropStops.alpha(stops, at: Double(y / height)) }
    #expect(at(0) == 0, "the still shows above")
    #expect(abs(at(FilmRules.above) - 0.82) < 0.001, "82 percent at the question's top line")
    #expect(at(FilmRules.above + FilmRules.solid) == 1)
    #expect(at(height) == 1, "black down to the foot, no band of still under the choices")
    // A block shorter than the solid reach (never drawn so, but the stops stay in order).
    let short = FilmRules.ramp(height: FilmRules.above + 40)
    #expect(zip(short, short.dropFirst()).allSatisfy { $0.at <= $1.at })
    #expect(short.last?.at == 1)
}

@Test func theLeadKeeps4_5To1OnThePhoneRamp() {
    // From the question's top line down, where the lead and the choices' lines sit.
    let height = FilmRules.above + 300
    let stops = FilmRules.ramp(height: height)
    for y in stride(from: FilmRules.above, through: height, by: 10) {
        let cover = BackdropStops.alpha(stops, at: Double(y / height))
        #expect(contrast(muted, over: cover) >= 4.5, "at \(y): \(contrast(muted, over: cover))")
    }
}

@Test func theWideColumnSitsInTheWash() {
    #expect(FilmRules.column(tv: false) == 580)
    #expect(FilmRules.column(tv: true) == 840)
    // The column's trailing edge, from under the header to the foot: the Mac and iPad 80 in (a 1440 by 900
    // window, iPad landscape, the Mac sheet's floor), the TV's canvas inside its 80 side.
    let screens: [(width: Double, height: Double, tv: Bool)] = [(1440, 900, false), (1024, 768, false), (760, 640, false), (1920, 1080, true)]
    for screen in screens {
        let stops = FilmRules.side(width: screen.width, height: screen.height, tv: screen.tv)
        #expect(zip(stops, stops.dropFirst()).allSatisfy { $0.at <= $1.at } && stops.last!.at <= 1)
        let edge = Double(FilmRules.inset + FilmRules.column(tv: screen.tv))
        for y in stride(from: 104.0, through: screen.height, by: 10) {
            let side = BackdropStops.alpha(stops, at: BackdropStops.washT(x: edge, y: y, width: screen.width, height: screen.height))
            let foot = BackdropStops.alpha(FilmRules.foot, at: (screen.height - y) / screen.height)
            let cover = BackdropStops.covered(side, foot)
            #expect(contrast(muted, over: cover) >= 4.5, "\(screen) at \(y): \(contrast(muted, over: cover))")
        }
    }
    // A wide window keeps the gallery's stops: 86 at half the line, none at 78 percent.
    let wide = FilmRules.side(width: 1440, height: 900, tv: false)
    #expect(zip(wide.map(\.at), [0, 0.5, 0.78]).allSatisfy { abs($0 - $1) < 1e-9 })
}

@Test func theHeadersWordsKeep4_5To1() {
    // The header's words are white (`Theme.Palette.text`, 92 percent), at their right where no side wash reaches.
    let text = 0.92
    // The phone: from the screen's top to the 48 pt header's foot under any status bar (none in landscape, up
    // to 62 with the Dynamic Island).
    for safeTop in [0.0, 20, 47, 54, 59, 62] {
        for y in stride(from: 0, through: safeTop + 48, by: 4) {
            let cover = BackdropStops.alpha(FilmRules.phoneTop, at: y / Double(FilmRules.phoneTopReach))
            #expect(contrast(text, over: cover) >= 4.5, "safe top \(safeTop) at \(y): \(contrast(text, over: cover))")
        }
    }
    // Wide: the Mac's 104 pt header at the sheet's floor, iPad's under its 24 status bar, the TV's in its 60 safe top.
    for (height, foot) in [(640.0, 104.0), (768, 24 + 104), (1080, 60 + 64)] {
        for y in stride(from: 0, through: foot, by: 4) {
            let cover = BackdropStops.alpha(FilmRules.top, at: y / height)
            #expect(contrast(text, over: cover) >= 4.5, "\(height) tall at \(y): \(contrast(text, over: cover))")
        }
    }
}

@Test func theMacFirstRunFillsTheWindow() throws {
    // The sheet takes the window's size (a 1440 by 900 window: the still runs edge to edge), never under its floor.
    #expect(FilmRules.macSheet(window: CGSize(width: 1440, height: 900)) == CGSize(width: 1440, height: 900))
    #expect(FilmRules.macSheet(window: CGSize(width: 720, height: 500)) == FilmRules.sheetFloor)
    // Both places that open first run ask for it.
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    for page in ["Home/HomeScreen.swift", "Discover/DiscoverScreen.swift"] {
        let source = try String(contentsOf: app.appending(path: page), encoding: .utf8)
        #expect(source.contains("FullScreen(isPresented: $onboarding, fillsWindow: true)"), "\(page) opens first run in a small sheet")
    }
}

@Test func theHeaderCountsFourStepsAndBackShowsFromTheSecond() {
    #expect(FilmRules.count(.doors) == 1)
    #expect(FilmRules.count(.defaults) == 2)
    #expect(FilmRules.count(.question) == 3)
    #expect(FilmRules.count(.subtitles) == 4)
    #expect(FilmRules.count(.other) == nil)
    #expect(!FilmRules.hasBack(FilmRules.count(.doors)))
    #expect(FilmRules.hasBack(FilmRules.count(.defaults)))
    #expect(!FilmRules.hasBack(nil))
}

private actor Asks {
    var count = 0
    func add() -> Int {
        count += 1
        return count
    }
}

@MainActor @Test func theStillIsKeptOnceItCameAndNoneOrAFailedAskIsAskedAgain() async {
    let asks = Asks()
    let still = StillOnce()
    let answer = FirstRunStill(background: "https://art.example/b.jpg", logo: nil, name: "B")
    // The core not ready yet: the ask fails, nothing is kept.
    #expect(await still.get { _ = await asks.add(); throw CancellationError() } == nil)
    #expect(await still.get { _ = await asks.add(); return answer } == answer)
    // Kept for every framed step after: no third ask.
    #expect(await still.get { _ = await asks.add(); return nil } == answer)
    #expect(await asks.count == 2)
    // The core's "none" (a phone's first requests past its budget) is not kept: the next step asks again and the
    // still comes late instead of never (batch 15, E images).
    let none = StillOnce()
    #expect(await none.get { _ = await asks.add(); return nil } == nil)
    #expect(await none.get { _ = await asks.add(); return answer } == answer)
    #expect(await none.get { _ = await asks.add(); return nil } == answer)
    #expect(await asks.count == 4)
}

@Test func theStillDecodesAndNullIsNone() throws {
    let still = try JSONDecoder().decode(FirstRunStill?.self, from: Data(#"{"background":"https://art.example/b.jpg","logo":null,"name":"Big Buck Bunny"}"#.utf8))
    #expect(still == FirstRunStill(background: "https://art.example/b.jpg", logo: nil, name: "Big Buck Bunny"))
    #expect(try JSONDecoder().decode(FirstRunStill?.self, from: Data("null".utf8)) == nil)
}
