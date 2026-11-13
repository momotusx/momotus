import Foundation
import SwiftUI
import Testing

// Touch controls keep a 44 pt frame: the player's pills, Picture in Picture, AirPlay and the season pull-down.

private func source(_ path: String) throws -> String {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    return try String(contentsOf: app.appending(path: path), encoding: .utf8)
}

@Test func theMinimumTargetIsFortyFourPoints() {
    #expect(Theme.minTarget == 44)
}

@Test func everyTouchControlTakesItsFrameFromTheMinimum() throws {
    // Read from the sources: a Menu's label builds no frames in a hosted view outside a running app.
    let controls = try source("Player/PlayerControls.swift")
    let pill = try body(of: "private func action(", in: controls)
    #expect(pill.contains(".frame(minWidth: Theme.minTarget, minHeight: Theme.minTarget)"), "the player's pills lost the minimum")
    // The bar's strips and its quick shift (plans/player-bar.md section 8): each chip, step, Close and each of the
    // shift's three parts.
    let strip = try source("Player/PlayerStrip.swift")
    let chip = try body(of: "struct StripChip<Mark: View>: View", in: strip)
    #expect(chip.contains(".frame(minWidth: Theme.minTarget, minHeight: Theme.minTarget)"), "the strip's chips lost the minimum")
    let shift = try body(of: "struct QuickShift: View", in: strip)
    #expect(shift.contains(".frame(minWidth: BarFit.shiftValue, minHeight: Theme.minTarget)"), "the shift's value lost the minimum")
    #expect(try body(of: "private func side(", in: shift).contains(".frame(minWidth: Theme.minTarget, minHeight: Theme.minTarget)"),
            "Earlier and Later lost the minimum")
    #expect(try body(of: "private func stepButton(", in: strip).contains(".frame(width: Theme.minTarget, height: Theme.minTarget)"))
    #expect(try body(of: "private func closeButton(", in: strip).contains(".frame(width: Theme.minTarget, height: Theme.minTarget)"))
    #expect(BarFit.shiftValue >= Theme.minTarget)
    let top = try body(of: "private var top: some View", in: controls)
    #expect(top.components(separatedBy: ".frame(width: Theme.minTarget, height: Theme.minTarget)").count - 1 >= 3,
            "Back, Picture in Picture and AirPlay each keep the minimum")
    let back = try body(of: "private var leaveBack: some ToolbarContent", in: source("Profiles/ManageProfiles.swift"))
    #expect(back.contains(".frame(minWidth: Theme.minTarget, minHeight: Theme.minTarget)"), "the editor's own Back lost the minimum")
    #expect(back.contains(".contentShape(Rectangle())"), "the frame alone does not widen the tap area")
    let shelf = try source("Title/TitleEpisodes.swift")
    let start = try #require(shelf.range(of: "struct SeasonShelf")).lowerBound
    let season = try body(of: "var body: some View", in: String(shelf[start...]))
    #expect(season.contains(".frame(minHeight: Theme.minTarget)"), "the season pull-down lost the minimum")
}

@Test func everyPartOfAStripStaysInSightOnANarrowBar() throws {
    // QA r5, R5-28: a row that scrolled had More languages cut under Style and Shadow past the iPhone's edge. Each
    // row wraps instead, as the web's strips do: every chip and group in sight, Style and Close last.
    let strip = try source("Player/PlayerStrip.swift")
    let row = try body(of: "private struct StripRow<Content: View, Pinned: View>: View", in: strip)
    #expect(row.contains("FlowRow(spacing: 8, middle: true) {\n            content\n            pinned\n        }"))
    #expect(!row.contains("ScrollView"), "a strip row scrolls again")
    let subtitles = try body(of: "struct SubtitleStrip: View", in: strip)
    #expect(subtitles.contains("StripRow { mainRow } pinned: { mainEnd }"))
    let end = try body(of: "@ViewBuilder private var mainEnd: some View", in: subtitles)
    #expect(end.contains("closeButton(close)") && end.contains("title: bar.style"))
}

@MainActor @Test func aWrappingRowKeepsEveryItemInsideItsWidth() {
    // Four 100 pt chips and a short label in 250 pt: two lines, nothing past the edge.
    let host = NSHostingView(rootView: FlowRow(spacing: 8, middle: true) {
        ForEach(0..<4, id: \.self) { _ in Color.white.frame(width: 100, height: 44) }
        Color.white.frame(width: 10, height: 20)
    }.frame(width: 250))
    let size = host.fittingSize
    #expect(size.width <= 250)
    #expect(abs(size.height - (44 * 2 + 8)) < 0.5, "two chips and then two and the label on a second line")
    withExtendedLifetime(host) {}
}

@Test func everyGlyphTheAppNamesIsInItsIcons() throws {
    // QA r5, R5-28: Position's Higher drew an empty circle, its chevron-up missing from the catalog.
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    let catalog = app.appending(path: "Resources/Icons.xcassets")
    let names = try #require(FileManager.default.enumerator(atPath: app.path()))
        .compactMap { $0 as? String }
        .filter { $0.hasSuffix(".swift") }
        .flatMap { file -> [String] in
            let text = (try? String(contentsOf: app.appending(path: file), encoding: .utf8)) ?? ""
            let found = text.matches(of: #/(?:Glyph\(|glyph: |Image\(|down: \(|up: \()"([a-z0-9]+(?:-[a-z0-9]+)*)"/#)
            return found.map { String($0.output.1) }
        }
    #expect(names.contains("chevron-up") && names.count > 50)
    // "letter" is an avatar's first-letter choice, drawn as text, not an icon.
    let missing = Set(names).subtracting(["letter"]).filter { !FileManager.default.fileExists(atPath: catalog.appending(path: "\($0).imageset").path()) }
    let others = ["Brand.xcassets", "Marks.xcassets", "Flags.xcassets"].map { app.appending(path: "Resources/\($0)") }
    let lost = missing.filter { name in !others.contains { FileManager.default.fileExists(atPath: $0.appending(path: "\(name).imageset").path()) } }
    #expect(lost.isEmpty, "no icon for \(lost.sorted())")
}

@Test func theIPhonesChipsLinksAndBacksKeepTheMinimum() throws {
    // QA r5, R5-27: Kind 39, Clear 41 by 21, More 14, Versions and Find addons and Error reports chips 34, Calendar's
    // Back 36. The shared chip and link styles take the minimum, as do the Kind segment and Calendar's Back.
    let chip = try body(of: "private struct ChipBody: View", in: source("Common/Browse.swift"))
    #expect(chip.contains("#if os(iOS)\n                // The drawn chip stays 34 tall; a touch reaches it over the minimum target (QA r5, R5-27).\n                .frame(minHeight: Theme.minTarget)\n                .contentShape(Rectangle())"),
            "the chips lost the minimum")
    let components = try source("Theme/Components.swift")
    #expect(try body(of: "static var textLink: Self", in: components).contains("FocusPlainStyle(shape: Capsule(), minHeight: Theme.minTarget)"),
            "the link style lost the minimum")
    #expect(try body(of: "struct FocusPlainStyle<S: InsettableShape>: ButtonStyle", in: components).contains("configuration.label.frame(minHeight: minHeight)"))
    let discover = try source("Discover/DiscoverParts.swift")
    #expect(try body(of: "struct KindSegment: View", in: discover).contains(".padding(.vertical, metrics.wide ? 3 : 0)"))
    let segment = try body(of: "private struct Segment: View", in: discover)
    #expect(segment.contains("minHeight: wide ? Theme.Pill.desktop - 6 : Theme.Pill.phone)"), "a Kind segment is shorter than a target")
    #expect(Theme.Pill.phone >= Theme.minTarget)
    #expect(discover.contains("// A link's style: at least a target tall on a touch screen (QA r5, R5-27: Clear was 41 by 21).\n            .buttonStyle(.textLink)"))
    let root = try source("App/RootView.swift")
    let calendar = try #require(root.range(of: "Button { select(.library) } label: {")).upperBound
    #expect(root[calendar...].prefix(300).contains(".frame(minWidth: Theme.minTarget, minHeight: Theme.minTarget)"), "Calendar's Back lost the minimum")
}

@Test func theMacToolbarHoldsSearchAloneAndTheOrnamentEndsWithTheProfile() throws {
    // Read from the sources: a toolbar is not laid out in a hosted view. The profile took Settings' place in the
    // ornament (plans/nav-profile.md), so it shows once: at the ornament's end, never as a toolbar item of its own.
    let root = try source("App/RootView.swift")
    let tools = try body(of: "private struct TopTools<Places: View>: ToolbarContent", in: root)
    #expect(tools.components(separatedBy: "ToolbarItem(placement: .primaryAction) { search }").count - 1 == 2, "Search keeps its item on both macOS paths")
    #expect(!tools.contains("Profile"), "the profile is back in the toolbar")
    let ornament = try #require(root.range(of: "PlaceOrnament(selection: tab, strong: ornamentStrong")).lowerBound
    #expect(root[ornament...].prefix(400).contains("ProfileMenu(onSettings: { select(.settings, link: true) }, onPin: pin, panel: $showPanel)"), "the ornament lost the profile's panel")
    #expect(root.contains("OrnamentProfile(lit: tab == .settings)"), "the ornament's profile is not lit on Settings")
}

@Test func thePhonesProfileTabKeepsTheMinimum() throws {
    let tab = try body(of: "private func profile(iconsOnly: Bool) -> some View", in: source("App/RootView.swift"))
    #expect(tab.contains(".frame(minWidth: Theme.minTarget, minHeight: Theme.minTarget)"), "the profile tab lost the minimum")
    #expect(tab.contains(".contentShape(Rectangle())"), "the frame alone does not widen the tap area")
}
