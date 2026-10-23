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
// the bare scrim. Plain text on the art (the hero's genre, meta, synopsis and reason) keeps 4.5:1 in
// its own color. Each spot is its layout's least dark case: the top trailing corner in points, the
// hero at its floor, and on the Mac the narrowest window (BackdropStops.macMinWidth).
private struct Spot {
    let name: String
    let page: String
    /// What covers the white art at the label, bottom first: the page color (or black), and how much.
    let layers: [(page: Bool, alpha: Double)]
    /// A round button has its word under the circle, on the bare scrim; a pill has none.
    var word = true
    /// Plain white text at this alpha straight on the scrim, instead of a secondary.
    var text: Double? = nil
}

/// Home's wide hero as HeroScrims draws it, at a label ending `end` pt from the leading edge with its
/// top `top` pt above the foot of a `width` by `height` hero: the vignette and the 80 degree wash in
/// black, then the foot in the page color, the last two laid out in the desktop frame's box off TV.
private func wideHome(width: Double, height: Double, end: Double, top: Double, held: Bool = true, wash: [BackdropStops.Stop] = BackdropStops.heroSide) -> [(page: Bool, alpha: Double)] {
    let size = CGSize(width: width, height: height)
    let box = BackdropStops.rampBox(size, held: held)
    let y = height - top
    return [
        (false, BackdropStops.vignetteAlpha(x: end / width, y: y / height)),
        (false, BackdropStops.alpha(wash, at: BackdropStops.washT(x: end, y: y, size: size, box: box))),
        (true, BackdropStops.alpha(BackdropStops.heroFoot(size, box: box), at: top / height)),
    ]
}

/// The wide title page as TitleScreen draws it: the side ramp, then the foot from the top, both in
/// the page color, at a label ending `end` pt in with its top `top` pt below the hero's top.
private func titlePage(width: Double, height: Double, end: Double, top: Double) -> [(page: Bool, alpha: Double)] {
    [(true, BackdropStops.alpha(BackdropStops.titleSide(width: width), at: end / width)),
     (true, BackdropStops.alpha(BackdropStops.titleFoot(wide: true), at: (height - top) / height))]
}

private let narrowestMac = Double(BackdropStops.macMinWidth)

/// Where the Mac title hero's parts sit, in points above the foot: tops no lower than TitleHeroTests
/// measures them on the real hero at 720 and 1440, so each spot reads a part at or above its real
/// place (less cover there). The column ends 40 + `heroSynopsisWidth` in.
enum TitleSpots {
    /// The action row at 720 wraps into two rows of 40, 12 apart, over the 75 pt foot.
    static let lowerRow: CGFloat = 75 + 10.5
    static let upperRow: CGFloat = 75 + 40 + 12 + 10.5
    static let synopsis: CGFloat = 246
    static let chips: CGFloat = 324
    static let meta: CGFloat = 356
    /// The hero at 720 grown by `heroFrame` to hold the column, logo at the top (445 measured).
    static let grown: CGFloat = 445
    static var end: Double { 40 + Double(BackdropStops.heroSynopsisWidth) }
}

/// The title page's hero (iOS and macOS): Home's ramps with the title's deeper wash.
private func titleHero(width: Double, height: Double, end: Double, top: CGFloat) -> [(page: Bool, alpha: Double)] {
    wideHome(width: width, height: height, end: end, top: Double(top), wash: BackdropStops.titleWash)
}

private let spots = [
    // Home's Details on the Mac: the 40 pt pill's 15 pt label (19 tall) ends 271 pt in (the 40 pt
    // margin, Play 115, 12, Details 134 less its 30), its top 104.5 above the foot (75, 10.5, 19),
    // in the hero's 400 pt floor.
    Spot(name: "home hero, Mac", page: "bgDesktop", layers: wideHome(width: narrowestMac, height: 400, end: 271, top: 104.5), word: false),
    // tvOS draws the same hero at 1920 by 600: Details ends at about a third of the width.
    Spot(name: "home hero, TV", page: "bg", layers: wideHome(width: 1920, height: 600, end: 634, top: 130, held: false), word: false),
    // The Mac hero's words, bottom up from the foot (75), the 40 pt pills and 16 to the reason: a
    // two-line reason (13 pt, 16 a line) to 163, the synopsis (15 pt, two lines of 18, its column
    // capped at heroSynopsisWidth from the 40 pt margin) to 208, the meta line (13) to 233, the genre
    // line (15/600) to 260. Each ends at its widest likely run: the reason and the meta line 300
    // wide, the genre line 260.
    Spot(name: "home hero, Mac, reason", page: "bgDesktop", layers: wideHome(width: narrowestMac, height: 400, end: 340, top: 163), word: false, text: 0.92),
    Spot(name: "home hero, Mac, synopsis", page: "bgDesktop", layers: wideHome(width: narrowestMac, height: 400, end: 40 + BackdropStops.heroSynopsisWidth, top: 208), word: false, text: 0.7),
    Spot(name: "home hero, Mac, meta", page: "bgDesktop", layers: wideHome(width: narrowestMac, height: 400, end: 340, top: 233), word: false, text: 0.64),
    Spot(name: "home hero, Mac, genre", page: "bgDesktop", layers: wideHome(width: narrowestMac, height: 400, end: 300, top: 260), word: false, text: 0.92),
    // The iPhone's 48 pt glass Details, its top 104 pt above the foot of the 680 pt hero.
    Spot(name: "home hero, iPhone", page: "bgPhone", layers: [(true, BackdropStops.alpha(BackdropStops.heroPhoneFoot(height: 680), at: 104.0 / 680))], word: false),
    // A landscape phone shortens the hero to most of its height, never under 280 (HomeScreen.heroHeight);
    // Details keeps its place, 104 pt above the foot.
    Spot(name: "home hero, iPhone landscape", page: "bgPhone", layers: [(true, BackdropStops.alpha(BackdropStops.heroPhoneFoot(height: 280), at: 104.0 / 280))], word: false),
    // The phone hero's text over art, bottom up in the 680 pt hero. Home: the foot (56), the 48 pt
    // buttons, 16 to a two-line reason (13 pt, 16 a line) and 6 more, the ratings (16) and 4, then the
    // genre line (15/600, 20 tall) stacked over them: its top about 198. The title page, as the iPhone
    // frame draws it: the genre line's top about 222, "31 min left" (13 pt, 70 percent white) about 134.
    Spot(name: "home hero, iPhone, genre stacked", page: "bgPhone", layers: [(true, BackdropStops.alpha(BackdropStops.heroPhoneFoot(height: 680), at: 198.0 / 680))], word: false, text: 0.92),
    Spot(name: "title, iPhone, genre", page: "bgPhone", layers: [(true, BackdropStops.alpha(BackdropStops.heroPhoneFoot(height: 680), at: 222.0 / 680))], word: false, text: 0.92),
    Spot(name: "title, iPhone, resume sub", page: "bgPhone", layers: [(true, BackdropStops.alpha(BackdropStops.heroPhoneFoot(height: 680), at: 134.0 / 680))], word: false, text: 0.7),
    // The Mac title hero (TitleSpots), at the narrowest window grown to hold its words and at 1440:
    // the pills of both action rows (a white label on the 16 percent fill), the synopsis (70 percent),
    // the chips (white on the fill) and the meta row (64 percent), each ending 520 in.
    Spot(name: "title, Mac, lower row", page: "bgDesktop", layers: titleHero(width: narrowestMac, height: TitleSpots.grown, end: TitleSpots.end, top: TitleSpots.lowerRow), word: false),
    Spot(name: "title, Mac, upper row", page: "bgDesktop", layers: titleHero(width: narrowestMac, height: TitleSpots.grown, end: TitleSpots.end, top: TitleSpots.upperRow), word: false),
    Spot(name: "title, Mac, synopsis", page: "bgDesktop", layers: titleHero(width: narrowestMac, height: TitleSpots.grown, end: TitleSpots.end, top: TitleSpots.synopsis), word: false, text: 0.7),
    Spot(name: "title, Mac, chips", page: "bgDesktop", layers: titleHero(width: narrowestMac, height: TitleSpots.grown, end: TitleSpots.end, top: TitleSpots.chips), word: false),
    Spot(name: "title, Mac, meta", page: "bgDesktop", layers: titleHero(width: narrowestMac, height: TitleSpots.grown, end: TitleSpots.end, top: TitleSpots.meta), word: false, text: 0.64),
    Spot(name: "title, Mac 1440, meta", page: "bgDesktop", layers: titleHero(width: 1440, height: 810, end: TitleSpots.end, top: TitleSpots.meta), word: false, text: 0.64),
    Spot(name: "title, Mac 1440, chips", page: "bgDesktop", layers: titleHero(width: 1440, height: 810, end: TitleSpots.end, top: TitleSpots.chips), word: false),
    // Starring at the right, 75 above the foot, three lines of 18 (top 129), its "Starring" key at 64
    // percent, read where it first shows (WideLayout.starringFrom, 1240) and at 1440.
    Spot(name: "title, Mac, Starring 1240", page: "bgDesktop", layers: titleHero(width: 1240, height: Double(WideLayout.titleHeroHeight(width: 1240, height: 800)), end: 1200, top: 129), word: false, text: 0.64),
    Spot(name: "title, Mac, Starring 1440", page: "bgDesktop", layers: titleHero(width: 1440, height: 810, end: 1400, top: 129), word: false, text: 0.64),
    Spot(name: "title, TV", page: "bg", layers: titlePage(width: 1920, height: 640, end: 653, top: 134)),
    // An episode card's words on its black legibility ramp over a white still, each line's top in
    // points above the card's foot: 12 in, the caption (12/600, 15 tall, 2 above it), the overview
    // (13 pt, 16 a line: two on the Mac, three on the iPhone), the name (15/600) and the tag (11 pt).
    // The ramp covers the Mac card's lower 141 and the iPhone card's lower 170.
    // The Mac card's "..." glyph over the still's top, past the ramp: on its black disc alone.
    Spot(name: "card, Mac, more", page: "bgDesktop", layers: [(false, BackdropStops.cardMoreBacking)], word: false, text: 1),
    Spot(name: "card, Mac, tag", page: "bgDesktop", layers: [(false, BackdropStops.alpha(BackdropStops.cardRamp, at: 98.0 / 141))], word: false, text: 0.7),
    Spot(name: "card, Mac, name", page: "bgDesktop", layers: [(false, BackdropStops.alpha(BackdropStops.cardRamp, at: 83.0 / 141))], word: false, text: 1),
    Spot(name: "card, Mac, overview", page: "bgDesktop", layers: [(false, BackdropStops.alpha(BackdropStops.cardRamp, at: 63.0 / 141))], word: false, text: 0.7),
    Spot(name: "card, Mac, caption", page: "bgDesktop", layers: [(false, BackdropStops.alpha(BackdropStops.cardRamp, at: 27.0 / 141))], word: false, text: 0.7),
    Spot(name: "card, iPhone, tag", page: "bgPhone", layers: [(false, BackdropStops.alpha(BackdropStops.cardRamp, at: 122.0 / 170))], word: false, text: 0.7),
    Spot(name: "card, iPhone, name", page: "bgPhone", layers: [(false, BackdropStops.alpha(BackdropStops.cardRamp, at: 107.0 / 170))], word: false, text: 1),
    Spot(name: "card, iPhone, overview", page: "bgPhone", layers: [(false, BackdropStops.alpha(BackdropStops.cardRamp, at: 85.0 / 170))], word: false, text: 0.7),
]

@Test(arguments: spots.map(\.name)) func aSecondaryOverWhiteArtKeepsItsLabel(_ name: String) {
    let s = spots.first { $0.name == name }!
    let page = tone(palette[s.page]!)
    let under = s.layers.reduce(Tone(r: 255, g: 255, b: 255, a: 1)) { art, layer in
        over(layer.page ? Tone(r: page.r, g: page.g, b: page.b, a: layer.alpha) : Tone(r: 0, g: 0, b: 0, a: layer.alpha), art)
    }
    if let text = s.text {
        let ratio = contrast(Tone(r: 255, g: 255, b: 255, a: text), on: Tone(r: 0, g: 0, b: 0, a: 0), page: under)
        print(#"glass-measure {"spot":"\#(name)","text":\#(text),"ratio":\#((ratio * 100).rounded() / 100)}"#)
        #expect(ratio >= 4.5, "text at \(text), \(name)")
        return
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

// A selected state that needs the key color takes `keyFill` with the white `onKey` label (the key blue on
// the track is about 3.65:1; plans/design-v3-phase7-apple.md 4.1).
@Test func theKeyFillCarriesItsLabel() {
    let page = tone(palette["bgDesktop"]!)
    #expect(contrast(tone(palette["onKey"]!), on: tone(palette["keyFill"]!), page: page) >= 4.5)
}
