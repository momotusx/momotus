import AppKit
import CoreText
import SwiftUI
import Testing

// The widest scores there are ("IMDb 10.0", "RT 100%") on a poster's chip, rendered with Inter as
// the app ships it: the chip fits inside the poster on iPhone, Mac and TV, stacked on iPhone and
// side by side wherever the pair fits. Widths print as `ratings-measure` lines for
// bench/core/ratings.jsonl; the snapshots go to the temporary directory.

private let widest = ["IMDb 10.0", "RT 100%"]

/// Inter from the source tree: the test bundle carries no fonts.
private let registered: Bool = {
    let fonts = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .appendingPathComponent("../Marquee/Resources/Fonts/Inter").standardized
    let files = (try? FileManager.default.contentsOfDirectory(at: fonts, includingPropertiesForKeys: nil)) ?? []
    for url in files where url.pathExtension == "ttf" {
        CTFontManagerRegisterFontsForURL(url as CFURL, .process, nil)
    }
    return !files.isEmpty
}()

@MainActor private func render(_ ratings: [String], _ metrics: Metrics, width: CGFloat?) -> (CGSize, NSImage?) {
    let renderer = ImageRenderer(content: RatingChip(ratings: ratings).environment(\.metrics, metrics))
    renderer.proposedSize = ProposedViewSize(width: width, height: nil)
    renderer.scale = 2
    let image = renderer.nsImage
    return (image?.size ?? .zero, image)
}

@MainActor private func check(_ name: String, _ metrics: Metrics, stacked: Bool, ratings: [String] = widest) throws {
    #expect(registered)
    // What the poster leaves the chip: its width less the chip's inset on both sides.
    let room = metrics.poster.width - 2 * metrics.chip * 0.55
    let (size, image) = render(ratings, metrics, width: room)
    let (one, _) = render([ratings[0]], metrics, width: room)
    #expect(size.width <= room + 0.5, "\(name): \(size.width) pt chip in \(room) pt")
    if stacked {
        #expect(size.height > one.height * 1.5, "\(name): not stacked (\(size.height) pt high)")
    } else {
        #expect(abs(size.height - one.height) < 0.5, "\(name): not side by side (\(size.height) pt high)")
    }
    let out = FileManager.default.temporaryDirectory.appendingPathComponent("marquee-ratings")
    try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
    if let tiff = image?.tiffRepresentation, let png = NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:]) {
        let scores = ratings.joined(separator: "-").replacingOccurrences(of: " ", with: "").replacingOccurrences(of: "%", with: "")
        try png.write(to: out.appendingPathComponent("chip-\(name)-\(scores).png"))
    }
    print(#"ratings-measure {"platform":"apple","form":"\#(name)","scores":"\#(ratings.joined(separator: " "))","chip":\#(size.width),"room":\#(room),"poster":\#(metrics.poster.width),"height":\#(size.height)}"#)
}

@MainActor @Test func theWidestScoresStackOnAnIPhonePoster() throws {
    try check("compact", .compact, stacked: true)
}

@MainActor @Test func theWidestScoresSitSideBySideOnAMacPoster() throws {
    try check("regular", .regular, stacked: false)
}

@MainActor @Test func theWidestScoresStackOnATVPoster() throws {
    // Side by side the pair would need more than the poster leaves at 20 pt.
    try check("tv", .tv, stacked: true)
}

@MainActor @Test func usualScoresSitSideBySideOnATVPoster() throws {
    try check("tv", .tv, stacked: false, ratings: ["IMDb 8.8", "RT 87%"])
    try check("tv", .tv, stacked: false, ratings: ["IMDb 8.8", "RT 100%"])
}

@Test func voiceOverSaysTheNames() {
    #expect(spokenScore("RT 87%") == "Rotten Tomatoes 87 percent")
    #expect(spokenScore("IMDb 8.8") == "IMDb 8.8")
}
