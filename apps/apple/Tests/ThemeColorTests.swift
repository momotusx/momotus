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
// the bare scrim. Each spot is its layout's least dark case: the label's top trailing corner in
// points, the hero at its floor, and on the Mac the narrowest window (BackdropStops.macMinWidth).
private struct Spot {
    let name: String
    let page: String
    /// What covers the white art at the label, bottom first: the page color (or black), and how much.
    let layers: [(page: Bool, alpha: Double)]
    /// A round button has its word under the circle, on the bare scrim; a pill has none.
    var word = true
}

/// Home's wide hero as HeroView draws it, at a label ending `end` pt from the leading edge with its
/// top `top` pt above the foot of a `width` by `height` hero: the vignette and the 80 degree wash in
/// black, then the foot in the page color.
private func wideHome(width: Double, height: Double, end: Double, top: Double) -> [(page: Bool, alpha: Double)] {
    let y = height - top
    return [
        (false, BackdropStops.vignetteAlpha(x: end / width, y: y / height)),
        (false, BackdropStops.alpha(BackdropStops.heroSide, at: BackdropStops.washT(x: end, y: y, width: width, height: height))),
        (true, BackdropStops.alpha(BackdropStops.heroFoot, at: top / height)),
    ]
}

/// The wide title page as TitleScreen draws it: the side ramp, then the foot from the top, both in
/// the page color, at a label ending `end` pt in with its top `top` pt below the hero's top.
private func titlePage(width: Double, height: Double, end: Double, top: Double) -> [(page: Bool, alpha: Double)] {
    [(true, BackdropStops.alpha(BackdropStops.titleSide, at: end / width)),
     (true, BackdropStops.alpha(BackdropStops.titleFoot(wide: true), at: (height - top) / height))]
}

private let narrowestMac = Double(BackdropStops.macMinWidth)

private let spots = [
    // Home's Details on the Mac: the 40 pt pill's 15 pt label (19 tall) ends 271 pt in (the 40 pt
    // margin, Play 115, 12, Details 134 less its 30), its top 104.5 above the foot (75, 10.5, 19),
    // in the hero's 400 pt floor.
    Spot(name: "home hero, Mac", page: "bgDesktop", layers: wideHome(width: narrowestMac, height: 400, end: 271, top: 104.5), word: false),
    // tvOS draws the same hero at 1920 by 600: Details ends at about a third of the width.
    Spot(name: "home hero, TV", page: "bg", layers: wideHome(width: 1920, height: 600, end: 634, top: 130), word: false),
    // The iPhone's 48 pt glass Details, its top 104 pt above the foot of the 680 pt hero.
    Spot(name: "home hero, iPhone", page: "bgPhone", layers: [(true, BackdropStops.alpha(BackdropStops.heroPhoneFoot(height: 680), at: 104.0 / 680))], word: false),
    // A landscape phone shortens the hero to most of its height, never under 280 (HomeScreen.heroHeight);
    // Details keeps its place, 104 pt above the foot.
    Spot(name: "home hero, iPhone landscape", page: "bgPhone", layers: [(true, BackdropStops.alpha(BackdropStops.heroPhoneFoot(height: 280), at: 104.0 / 280))], word: false),
    // The title's round row, at the far edge of its 580 pt column (620 pt in), the hero at its 420 floor.
    Spot(name: "title, Mac", page: "bgDesktop", layers: titlePage(width: narrowestMac, height: 420, end: 620, top: 96)),
    // Start over beside Resume, above the version line and the round row: the widest Resume row
    // ends 548 pt in (measured in 1b).
    Spot(name: "title, Mac, Start over", page: "bgDesktop", layers: titlePage(width: narrowestMac, height: 420, end: 548, top: 190), word: false),
    Spot(name: "title, TV", page: "bg", layers: titlePage(width: 1920, height: 640, end: 653, top: 134)),
]

@Test(arguments: spots.map(\.name)) func aSecondaryOverWhiteArtKeepsItsLabel(_ name: String) {
    let s = spots.first { $0.name == name }!
    let page = tone(palette[s.page]!)
    let under = s.layers.reduce(Tone(r: 255, g: 255, b: 255, a: 1)) { art, layer in
        over(layer.page ? Tone(r: page.r, g: page.g, b: page.b, a: layer.alpha) : Tone(r: 0, g: 0, b: 0, a: layer.alpha), art)
    }
    // A round button can hold a state (In Library, Watched); a pill does not.
    for fill in s.word ? [SecondaryFillLevel.rest, SecondaryFillLevel.held] : [SecondaryFillLevel.rest] {
        let ratio = contrast(Tone(r: 255, g: 255, b: 255, a: 1), on: Tone(r: 255, g: 255, b: 255, a: fill), page: under)
        print(#"glass-measure {"spot":"\#(name)","fill":\#(fill),"ratio":\#((ratio * 100).rounded() / 100)}"#)
        #expect(ratio >= 4.5, "label on \(fill), \(name)")
    }
    if s.word {
        #expect(contrast(Tone(r: 255, g: 255, b: 255, a: 0.7), on: Tone(r: 0, g: 0, b: 0, a: 0), page: under) >= 4.5, "word, \(name)")
    }
}
