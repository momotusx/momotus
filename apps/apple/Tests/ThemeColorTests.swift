import Foundation
import Testing

// Colors the theme draws, from design/tokens.json: a chip's words on the track and on the selected
// segment (Browse.swift ChipStyle), the control edge at 3:1 on the page, the Mac's keyboard ring and
// its header, the per-form palettes, and the glass check.

private struct Tone { var r, g, b, a: Double }

private func tone(_ css: String) -> Tone {
    if css.hasPrefix("#") {
        let v = Int(css.dropFirst(), radix: 16)!
        return Tone(r: Double(v >> 16 & 255), g: Double(v >> 8 & 255), b: Double(v & 255), a: 1)
    }
    let n = css.split(whereSeparator: { !"0123456789.".contains($0) }).compactMap { Double($0) }
    return Tone(r: n[0], g: n[1], b: n[2], a: n.count > 3 ? n[3] : 1)
}

private func over(_ top: Tone, _ under: Tone) -> Tone {
    Tone(r: top.r * top.a + under.r * (1 - top.a), g: top.g * top.a + under.g * (1 - top.a), b: top.b * top.a + under.b * (1 - top.a), a: 1)
}

private func luminance(_ c: Tone) -> Double {
    func lin(_ v: Double) -> Double { v / 255 <= 0.04045 ? v / 255 / 12.92 : pow((v / 255 + 0.055) / 1.055, 2.4) }
    return 0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

private func contrast(_ text: Tone, on fill: Tone, page: Tone) -> Double {
    let back = over(fill, page)
    let (a, b) = (luminance(over(text, back)) + 0.05, luminance(back) + 0.05)
    return max(a, b) / min(a, b)
}

private let palette: [String: String] = {
    let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../../../design/tokens.json")
    let json = try! JSONSerialization.jsonObject(with: Data(contentsOf: url)) as! [String: Any]
    return (json["color"] as! [String: Any])["dark"] as! [String: String]
}()

private let selectedFill: Tone = {
    let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../../../design/tokens.json")
    let json = try! JSONSerialization.jsonObject(with: Data(contentsOf: url)) as! [String: Any]
    return tone((json["selected"] as! [String: String])["fill"]!)
}()

// Every page a chip row sits on: TV and the base, the iPhone set, the Mac page.
@Test(arguments: ["bg", "surface", "bgPhone", "surfacePhone", "bgDesktop"]) func aChipsWordsReadOnTheTrackAndTheSelectedSegment(_ page: String) {
    let under = tone(palette[page]!)
    for fill in [ChipFill.rest, ChipFill.hover] {
        let track = Tone(r: ChipFill.track.r, g: ChipFill.track.g, b: ChipFill.track.b, a: fill)
        #expect(contrast(tone(palette["muted"]!), on: track, page: under) >= 4.5, "muted on the track at \(fill)")
    }
    #expect(contrast(tone(palette["text"]!), on: selectedFill, page: under) >= 4.5, "text on the selected segment")
}

@Test func theControlEdgeKeeps3To1OnThePage() {
    #expect(contrast(tone(palette["control"]!), on: tone(palette["bg"]!), page: tone(palette["bg"]!)) >= 3)
}

@Test func theMacHeaderIsBlackAndItsWordsRead() {
    // tv.apple.com's 52 pt black bar on every page: the tabs at 64 and 92 percent, the field's edge.
    let bar = tone(palette["barDesktop"]!)
    #expect(palette["barDesktop"] == "#000000")
    #expect(contrast(tone(palette["dim"]!), on: bar, page: bar) >= 4.5)
    #expect(contrast(tone(palette["text"]!), on: bar, page: bar) >= 4.5)
}

@Test func theMacKeyboardRingIsTheKeyBlue() {
    // FocusRing strokes FocusRule.ring; the key token is Apple's system blue.
    #expect(FocusRule.ring == .key)
    #expect(palette["key"] == "#0A84FF")
    #expect(contrast(tone(palette["key"]!), on: tone(palette["surface"]!), page: tone(palette["bg"]!)) >= 3)
}

// Per form (D10): the iPhone set (black page, #1C1C1E groups, iOS's secondary grey, the #F5F5F5
// pill) and the Mac's #1F1F1F page, as gen-theme.ts puts them under the base names.
@Test func theIPhonePaletteKeepsItsPairs() {
    let page = tone(palette["bgPhone"]!), group = tone(palette["surfacePhone"]!)
    #expect(contrast(tone(palette["dimPhone"]!), on: page, page: page) >= 4.5)
    #expect(contrast(tone(palette["dimPhone"]!), on: group, page: page) >= 4.5)
    #expect(contrast(tone(palette["text"]!), on: group, page: page) >= 4.5)
    #expect(contrast(tone(palette["onPrimary"]!), on: tone(palette["primaryPhone"]!), page: page) >= 4.5)
    #expect(contrast(tone(palette["key"]!), on: page, page: page) >= 4.5)
}

@Test func theMacPageKeepsItsPairs() {
    let page = tone(palette["bgDesktop"]!)
    #expect(contrast(tone(palette["dim"]!), on: page, page: page) >= 4.5)
    #expect(contrast(tone(palette["dim"]!), on: tone(palette["bandDesktop"]!), page: page) >= 4.5)
    #expect(contrast(tone(palette["key"]!), on: page, page: page) >= 3)
}

// The glass check (plans/design-v3-rollout.md, section 6): white art under the backdrop's scrims at
// each secondary over art, then its fill at rest and held (SecondaryFillLevel). The pure white label
// the code draws keeps 4.5:1 on either, and so does the word under a round button (70 percent) on
// the bare scrim. The spots are the layouts' least dark cases (HeroView, TitleScreen): the label's
// top edge in points from the hero's foot, the hero's height at its floor, and the row's far edge as
// a share of the width.
private struct Spot {
    let name: String
    let page: String
    let side: [BackdropStops.Stop]?
    let x: Double
    let foot: [BackdropStops.Stop]
    /// The foot gradient starts at the bottom (the hero) or at the top (the title page).
    let fromTop: Bool
    let top: Double
    let height: Double
    /// A round button has its word under the circle, on the bare scrim; a pill has none.
    var word = true
}

private let spots = [
    // The Mac hero's 400 pt floor holds below a 1111 pt window (HomeScreen.heroHeight).
    Spot(name: "home hero, Mac", page: "bgDesktop", side: BackdropStops.heroSide, x: 0.36, foot: BackdropStops.heroFoot, fromTop: false, top: 112, height: 400),
    Spot(name: "home hero, TV", page: "bg", side: BackdropStops.heroSide, x: 0.24, foot: BackdropStops.heroFoot, fromTop: false, top: 151, height: 600),
    Spot(name: "home hero, iPhone", page: "bgPhone", side: nil, x: 0, foot: BackdropStops.heroPhoneFoot, fromTop: false, top: 85, height: 520),
    Spot(name: "title, Mac", page: "bgDesktop", side: BackdropStops.titleSide, x: 0.69, foot: BackdropStops.titleFoot(wide: true), fromTop: true, top: 96, height: 420),
    // Start over beside Resume, above the version line and the round row: the widest Resume row
    // ends near .6 of a 913 pt window, where the title hero is at its 420 pt floor.
    Spot(name: "title, Mac, Start over", page: "bgDesktop", side: BackdropStops.titleSide, x: 0.6, foot: BackdropStops.titleFoot(wide: true), fromTop: true, top: 190, height: 420, word: false),
    Spot(name: "title, TV", page: "bg", side: BackdropStops.titleSide, x: 0.34, foot: BackdropStops.titleFoot(wide: true), fromTop: true, top: 134, height: 640),
]

@Test(arguments: spots.map(\.name)) func aSecondaryOverWhiteArtKeepsItsLabel(_ name: String) {
    let s = spots.first { $0.name == name }!
    let t = s.fromTop ? (s.height - s.top) / s.height : s.top / s.height
    let foot = BackdropStops.alpha(s.foot, at: t)
    let cover = s.side.map { BackdropStops.covered(foot, BackdropStops.alpha($0, at: s.x)) } ?? foot
    let page = tone(palette[s.page]!)
    let under = over(Tone(r: page.r, g: page.g, b: page.b, a: cover), Tone(r: 255, g: 255, b: 255, a: 1))
    // A round button can hold a state (In Library, Watched); a pill does not.
    for fill in s.word ? [SecondaryFillLevel.rest, SecondaryFillLevel.held] : [SecondaryFillLevel.rest] {
        #expect(contrast(Tone(r: 255, g: 255, b: 255, a: 1), on: Tone(r: 255, g: 255, b: 255, a: fill), page: under) >= 4.5, "label on \(fill), \(name)")
    }
    if s.word {
        #expect(contrast(Tone(r: 255, g: 255, b: 255, a: 0.7), on: Tone(r: 0, g: 0, b: 0, a: 0), page: under) >= 4.5, "word, \(name)")
    }
}
