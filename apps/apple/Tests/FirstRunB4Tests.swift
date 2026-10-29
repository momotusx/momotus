import AppKit
import Foundation
import SwiftUI
import Testing

// First run B4 (the owner's pick of 2026-10-28, design/DECISION.md, First run): the 2.39:1 frame as wide as the
// height left allows, never under 40 percent of the screen, the frame and its bar in the middle under the header,
// the question's scrim over a white still, the step count and Back, and the still asked once.

@Test func theFrameIsEdgeToEdgeWhileTheHeightLeftAllowsIt() {
    // A 1440 by 900 desktop under a 100 header with a 149 bar: the frame spans the width (the gallery's 1440x602).
    let full = FilmRules.frame(width: 1440, room: 900 - 100, bar: 149, edge: 0, floor: 900 * FilmRules.floor)
    #expect(full.width == 1440)
    #expect(abs(full.width / full.height - 2.39) < 0.001)
    // A wide, short window: the height left sets the frame, at 2.39:1 and centered by its layout.
    let short = FilmRules.frame(width: 1920, room: 700, bar: 160, edge: 24, floor: 0)
    #expect(abs(short.height - (700 - 160 - 48)) < 0.001)
    #expect(abs(short.width - short.height * 2.39) < 0.001)
    // Never under 40 percent of the screen: a tall bar (200 percent text) leaves the page to scroll instead.
    let floor = 800 * FilmRules.floor
    let tight = FilmRules.frame(width: 1440, room: 700, bar: 600, edge: 24, floor: floor)
    #expect(tight.height == floor)
}

@MainActor private final class Frames {
    var frame = CGRect.zero
    var bar = CGRect.zero
    var total = CGRect.zero
}

@MainActor private func laidOut(width: CGFloat, room: CGFloat, bar: CGFloat, floor: CGFloat = 0, edge: CGFloat = 24) -> Frames {
    let frames = Frames()
    let page = FilmLayout(room: room, floor: floor, edge: edge) {
        Color.black.onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { frames.frame = $0 }
        Color.gray.frame(height: bar).onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { frames.bar = $0 }
    }
    .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { frames.total = $0 }
    let host = NSHostingView(rootView: page.frame(width: width).fixedSize(horizontal: false, vertical: true))
    host.frame = CGRect(x: 0, y: 0, width: width, height: max(room, 2000))
    host.layoutSubtreeIfNeeded()
    RunLoop.main.run(until: Date().addingTimeInterval(0.1))
    return frames
}

@MainActor @Test func theFrameAndItsBarSitInTheMiddleUnderTheHeader() {
    // A tall window (a portrait phone, a TV with room): what is left goes above and below alike.
    let f = laidOut(width: 390, room: 700, bar: 240)
    #expect(f.frame.width == 390)
    #expect(abs(f.frame.width / f.frame.height - 2.39) < 0.01)
    #expect(f.bar.minY == f.frame.maxY, "the bar starts under the frame")
    let above = f.frame.minY - f.total.minY, below = f.total.maxY - f.bar.maxY
    #expect(abs(above - below) < 1, "above \(above), below \(below)")
    #expect(above > 24)
}

@MainActor @Test func aWideShortWindowCentersTheFrameAcross() {
    let f = laidOut(width: 1440, room: 600, bar: 150)
    #expect(f.frame.height < 1440 / 2.39)
    #expect(abs(f.frame.midX - f.total.midX) < 0.5, "frame \(f.frame) in \(f.total)")
    #expect(f.bar.width == 1440)
}

@MainActor @Test func aBarTallerThanTheRoomScrollsTheFrameAtItsFloor() {
    let f = laidOut(width: 1440, room: 500, bar: 600, floor: 320)
    #expect(abs(f.frame.height - 320) < 0.5)
    #expect(f.total.height > 500)
    #expect(abs(f.frame.minY - f.total.minY - 24) < 0.5)
}

@Test func theQuestionsWhiteWordsKeep4_5To1OverAWhiteStill() {
    // The player's 60 percent scrim behind each line: white words over white art under it.
    func lin(_ v: Double) -> Double { v <= 0.04045 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4) }
    let back = 1 - FilmRules.scrim
    #expect((1.05) / (lin(back) + 0.05) >= 4.5)
}

@Test func theMacFirstRunFillsTheWindow() throws {
    // The sheet takes the window's size (a 1440 by 900 window: the letterbox runs edge to edge), never under its floor.
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

@MainActor @Test func theStillIsAskedOnceAndAFailedAskIsAskedAgain() async {
    let asks = Asks()
    let still = StillOnce()
    let answer = FirstRunStill(background: "https://art.example/b.jpg", logo: nil, name: "B")
    // The core not ready yet: the ask fails, nothing is kept.
    #expect(await still.get { _ = await asks.add(); throw CancellationError() } == nil)
    #expect(await still.get { _ = await asks.add(); return answer } == answer)
    // Kept for every framed step after: no third ask.
    #expect(await still.get { _ = await asks.add(); return nil } == answer)
    #expect(await asks.count == 2)
    // The core's "none" (offline, past its budget) is kept too: the frame stays black.
    let none = StillOnce()
    #expect(await none.get { _ = await asks.add(); return nil } == nil)
    #expect(await none.get { _ = await asks.add(); return answer } == nil)
    #expect(await asks.count == 3)
}

@Test func theStillDecodesAndNullIsNone() throws {
    let still = try JSONDecoder().decode(FirstRunStill?.self, from: Data(#"{"background":"https://art.example/b.jpg","logo":null,"name":"Big Buck Bunny"}"#.utf8))
    #expect(still == FirstRunStill(background: "https://art.example/b.jpg", logo: nil, name: "Big Buck Bunny"))
    #expect(try JSONDecoder().decode(FirstRunStill?.self, from: Data("null".utf8)) == nil)
}
