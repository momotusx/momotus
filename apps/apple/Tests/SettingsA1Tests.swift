import AppKit
import Foundation
import SwiftUI
import Testing

// Settings A1 (the owner's pick of 2026-10-28, design/DECISION.md, Settings): the menu's sections and their
// tiles, the header's art and its one height, the centered column, the menu's fill and hold, who sees Edit and
// Manage, and the header's words over a white backdrop.

private let tokens: [String: String] = {
    let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../../../design/tokens.json")
    let json = try! JSONSerialization.jsonObject(with: Data(contentsOf: url)) as! [String: Any]
    return (json["color"] as! [String: Any])["dark"] as! [String: String]
}()

private func rgb(_ hex: String) -> (Double, Double, Double) {
    let v = Int(hex.dropFirst(), radix: 16)!
    return (Double(v >> 16 & 255) / 255, Double(v >> 8 & 255) / 255, Double(v & 255) / 255)
}

private func luminance(_ c: (Double, Double, Double)) -> Double {
    func lin(_ v: Double) -> Double { v <= 0.04045 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4) }
    return 0.2126 * lin(c.0) + 0.7152 * lin(c.1) + 0.0722 * lin(c.2)
}

private func contrast(_ a: (Double, Double, Double), _ b: (Double, Double, Double)) -> Double {
    let (x, y) = (luminance(a) + 0.05, luminance(b) + 0.05)
    return max(x, y) / min(x, y)
}

private func tile(_ id: String, background: String?) -> Tile {
    Tile(id: id, kind: "movie", name: id, poster: nil, posterFallback: nil, background: background, logo: nil, posterShape: "poster",
         releaseInfo: nil, progress: nil, videoId: nil, rating: nil, ratings: nil)
}

private func hero(_ id: String, background: String?) throws -> Hero {
    let art = background.map { #","background":"\#($0)""# } ?? ""
    return try JSONDecoder().decode(Hero.self, from: Data(#"{"tile":{"id":"\#(id)","kind":"movie","name":"\#(id)","posterShape":"poster"\#(art)},"reason":"Popular"}"#.utf8))
}

@Test func theMenuListsTheSectionsInPageOrderAndTVHasNoThisDevice() {
    #expect(SettingsSection.shown(tv: false).map(\.title) == ["Profiles", "Sources", "Account", "Home", "Playback", "This device", "About"])
    // tvOS has no setting of its own for This device (the trailer previews switch is off the TV), as Android TV.
    #expect(SettingsSection.shown(tv: true) == [.profiles, .sources, .account, .home, .playback, .about])
}

@Test(arguments: SettingsTone.allCases) func everyTileIsABlueToGreenStepItsWhiteGlyphReads(_ tone: SettingsTone) throws {
    // The fill drawn is the token's, never red or orange, and the white glyph keeps 3:1 on it (green the lowest, 3.13).
    let hex = try #require(tokens[tone.token], "no token \(tone.token)")
    let drawn = try #require(NSColor(tone.fill).usingColorSpace(.sRGB))
    let want = rgb(hex)
    #expect(abs(drawn.redComponent - want.0) < 0.01 && abs(drawn.greenComponent - want.1) < 0.01 && abs(drawn.blueComponent - want.2) < 0.01,
            "\(tone) draws \(drawn), its token is \(hex)")
    let fill = (Double(drawn.redComponent), Double(drawn.greenComponent), Double(drawn.blueComponent))
    #expect(contrast((1, 1, 1), fill) >= 3, "\(tone): \(contrast((1, 1, 1), fill))")
    if drawn.saturationComponent > 0.1 {
        #expect((120...230).contains(drawn.hueComponent * 360), "\(tone) has hue \(drawn.hueComponent * 360)")
    }
}

@Test func everySectionAndRowTakesATileFromTheSteps() {
    // Profiles blue, Sources green, Account sky, Home mint, Playback teal, This device sea, About grey (the web's).
    #expect(SettingsSection.allCases.map(\.tone) == [.blue, .green, .sky, .mint, .teal, .sea, .grey])
}

@Test func theHeaderArtIsContinueWatchingsFirstBackdropElseTheHero() throws {
    let watching = HomeRow(rowId: SettingsRules.continueRow, title: "Continue Watching", source: "", state: .ready,
                           tiles: [tile("no-art", background: nil), tile("cw", background: "https://art.example/cw.jpg")])
    let popular = HomeRow(rowId: "popular", title: "Popular", source: "", state: .ready, tiles: [tile("p", background: "https://art.example/p.jpg")])
    let heroes = [try hero("bare", background: nil), try hero("h", background: "https://art.example/h.jpg")]
    // The first Continue Watching title with a backdrop wins over the hero.
    #expect(SettingsRules.art(rows: [popular, watching], heroes: heroes, kids: false)?.id == "cw")
    // No Continue Watching backdrop: Home's first hero with one; a catalog row never lends its art.
    #expect(SettingsRules.art(rows: [popular], heroes: heroes, kids: false)?.id == "h")
    // A kids profile takes its own Continue Watching only, else none.
    #expect(SettingsRules.art(rows: [popular], heroes: heroes, kids: true) == nil)
    #expect(SettingsRules.art(rows: [popular, watching], heroes: heroes, kids: true)?.id == "cw")
    // Before Home's rows are in nothing is decided, so a hero never shows first and gives way.
    #expect(SettingsRules.art(rows: [], heroes: heroes, kids: false) == nil)
}

@Test func kidsSeeNeitherEditNorManageAndManageIsTheOwners() {
    #expect(SettingsRules.canEdit(kids: false))
    #expect(!SettingsRules.canEdit(kids: true))
    #expect(SettingsRules.canManage(kids: false, owner: true))
    #expect(!SettingsRules.canManage(kids: false, owner: false))
    #expect(!SettingsRules.canManage(kids: true, owner: true))
    #expect(SettingsRules.profileLine(many: false) == "Add a profile for someone else, or switch")
}

@Test func theMenuFillsTheSectionAtTheTopThird() {
    let tops: [(SettingsSection, CGFloat)] = [(.profiles, -900), (.sources, -400), (.account, 250), (.home, 320), (.playback, 900)]
    // A 900 pt page: the line is at 300; Account (250) has passed it, Home (320) has not.
    #expect(SettingsRules.active(tops, viewport: 900) == .account)
    // At rest the first section fills.
    #expect(SettingsRules.active([(.profiles, 0), (.sources, 450)], viewport: 900) == .profiles)
}

@Test func theMenuHoldsUnderTheBarAndNeverPastTheLists() {
    // Its column still below the bar: the menu sits where it is.
    #expect(SettingsRules.menuShift(columnTop: 400, stick: 76, room: 1200) == 0)
    // Scrolled past: it moves down by as much, so it stays at the bar.
    #expect(SettingsRules.menuShift(columnTop: -300, stick: 76, room: 1200) == 376)
    // Never past the lists' end.
    #expect(SettingsRules.menuShift(columnTop: -3000, stick: 76, room: 1200) == 1200)
}

@MainActor private final class Frames {
    var column = CGRect.zero
}

@MainActor @Test(arguments: [1440.0, 1920.0, 800.0])
func theColumnSitsInTheMiddleOfTheWindow(width: Double) {
    // The Mac's column (1120) laid out for real: the space left and right of it alike, never under the gutter.
    let frames = Frames()
    let pad = SettingsRules.pad(width: width, gutter: Metrics.regular.gutter, column: SettingsRules.Sizes.wide.column)
    let page = HStack(spacing: 0) {
        Color.clear.frame(maxWidth: .infinity, maxHeight: 10)
            .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { frames.column = $0 }
    }
    .padding(.horizontal, pad)
    let host = NSHostingView(rootView: page.frame(width: width, height: 10))
    host.frame = CGRect(x: 0, y: 0, width: width, height: 10)
    host.layoutSubtreeIfNeeded()
    RunLoop.main.run(until: Date().addingTimeInterval(0.1))
    #expect(abs(frames.column.minX - (width - frames.column.maxX)) < 0.5, "column \(frames.column) in \(width)")
    #expect(frames.column.width <= SettingsRules.Sizes.wide.column + 0.5)
    #expect(frames.column.minX >= Metrics.regular.gutter - 0.5)
}

@MainActor @Test func theHeaderKeepsOneHeightArtOrNone() {
    // The art lands late, or never: the header is the same height from its first frame (no layout shift).
    func height(_ art: some View) -> CGFloat {
        let header = Text("Main").font(.system(size: 50)).settingsHeader(height: 380) { art }
        return NSHostingView(rootView: header.frame(width: 1440)).fittingSize.height
    }
    #expect(height(EmptyView()) == 380)
    #expect(height(Color.red.frame(height: 2000)) == 380)
    #expect(height(Color.red.frame(height: 40)) == 380)
}

@Test func theHeadersWordsKeep4_5To1OverAWhiteBackdrop() {
    // The ramps as drawn (SettingsArt) over white art, at each word's spot: x and y as shares of the
    // header from its leading edge and from its foot. The Mac's 1440 window: the name at x 312 and 47 percent
    // up, its line 33 percent up; the pills' words 14 percent up. The phone: the large title 45 percent up.
    func darkness(x: Double, up: Double, wide: Bool) -> Double {
        wide
            ? BackdropStops.covered(BackdropStops.alpha(BackdropStops.settingsSide, at: x), BackdropStops.alpha(BackdropStops.settingsFoot, at: up))
            : BackdropStops.alpha(BackdropStops.settingsPhone, at: up)
    }
    // The ramps are in the page color: the Mac's #1F1F1F, the phone's black.
    func ratio(_ text: Double, x: Double, up: Double, wide: Bool) -> Double {
        let d = darkness(x: x, up: up, wide: wide), page = wide ? rgb(tokens["bgDesktop"]!).0 : 0
        let back = (1 - d) + d * page
        let words = text + (1 - text) * back
        return contrast((words, words, words), (back, back, back))
    }
    // White name and the 70 percent line at the name's start and across its likely length.
    for x in [312.0 / 1440, 560.0 / 1440] {
        #expect(ratio(1, x: x, up: 0.47, wide: true) >= 4.5, "name at \(x)")
        #expect(ratio(0.7, x: x, up: 0.33, wide: true) >= 4.5, "line at \(x)")
    }
    #expect(ratio(1, x: 400.0 / 1440, up: 0.14, wide: true) >= 4.5, "pills")
    // The phone's large title: 34 pt bold is large text, but it keeps 4.5:1 too.
    #expect(ratio(1, x: 0, up: 0.45, wide: false) >= 4.5, "phone title")
}
