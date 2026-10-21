import Foundation
import Testing

// Colors the theme draws, from design/tokens.json: a chip's detail at 4.5:1 on every fill it sits
// on (Browse.swift ChipStyle), the control edge at 3:1 on the page, and the Mac's keyboard ring.

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

private func faded(_ c: Tone, _ opacity: Double) -> Tone { Tone(r: c.r, g: c.g, b: c.b, a: c.a * opacity) }

@Test func anUnselectedChipsDetailReadsOnItsCapsuleAtRestAndOnHover() {
    let detail = faded(tone(palette["muted"]!), chipDetailOpacity(selected: false))
    for page in ["bg", "surface"] {
        for fill in [0.08, 0.14] {
            #expect(contrast(detail, on: Tone(r: 255, g: 255, b: 255, a: fill), page: tone(palette[page]!)) >= 4.5, "\(page) at \(fill)")
        }
    }
}

@Test func theControlEdgeKeeps3To1OnThePage() {
    #expect(contrast(tone(palette["control"]!), on: tone(palette["bg"]!), page: tone(palette["bg"]!)) >= 3)
}

@Test func aSelectedChipsDetailReadsOnTheWhitePill() {
    let detail = faded(tone(palette["onPrimary"]!), chipDetailOpacity(selected: true))
    #expect(contrast(detail, on: tone(palette["primary"]!), page: tone(palette["bg"]!)) >= 4.5)
}

@Test func theMacKeyboardRingIsTheKeyBlue() {
    // FocusRing strokes FocusRule.ring; the key token is Apple's system blue.
    #expect(FocusRule.ring == .key)
    #expect(palette["key"] == "#0A84FF")
    #expect(contrast(tone(palette["key"]!), on: tone(palette["surface"]!), page: tone(palette["bg"]!)) >= 3)
}
