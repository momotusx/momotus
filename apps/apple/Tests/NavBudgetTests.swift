import AppKit
import SwiftUI
import Testing

// Navigation budget (plans/perf-nav.md): what each update may measure, counted, so the guard holds on any CI machine.
// A press on the Mac or the iPhone cost one layout of the page in front; these fail when an update starts measuring
// the pages out of sight again (the places' minimum size, 5 to 19 percent of the Mac's main thread on 2026-11-19) or
// the cards of every shelf on each scroll step.

@MainActor private final class Count {
    var asked = 0
    /// Questions at no size or a small one: a minimum, never the frame the content is laid out in.
    var minimum = 0
}

/// Counts the size questions its content is asked.
private struct Probe: Layout {
    let count: Count

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let small = (proposal.width ?? 0) < 100 || (proposal.height ?? 0) < 100
        MainActor.assumeIsolated {
            count.asked += 1
            if small { count.minimum += 1 }
        }
        return subviews.first?.sizeThatFits(proposal) ?? .zero
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        subviews.first?.place(at: bounds.origin, proposal: ProposedViewSize(bounds.size))
    }
}

private func source(_ path: String) throws -> String {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    return try String(contentsOf: app.appending(path: path), encoding: .utf8)
}

/// Three places as the shell holds them, under an inset as the shell's banner and bar are, each a scrolling page of
/// shelves; the size questions each place is asked are counted.
private struct Shell: View {
    let shown: Int
    let places: Count
    let fill: Bool

    var body: some View {
        Group {
            if fill {
                Fill { pages }
            } else {
                ZStack { pages }
            }
        }
        .safeAreaInset(edge: .top, spacing: 0) { Color.clear.frame(height: 0) }
    }

    @ViewBuilder private var pages: some View {
        ForEach(0..<3, id: \.self) { place in
            Probe(count: places) { ScrollView {
                VStack(spacing: 20) {
                    ForEach(0..<4, id: \.self) { _ in
                        Shelf(spacing: 10, room: 12) {
                            ForEach(0..<6, id: \.self) { _ in Color.gray.frame(width: 100, height: 150) }
                        }
                    }
                }
            } }
            .opacity(place == shown ? 1 : 0)
        }
    }
}

@MainActor @Test func theShellsMinimumSizeAsksNoPlace() throws {
    let places = Count()
    let shell = NSHostingController(rootView: Shell(shown: 0, places: places, fill: true))
    places.minimum = 0
    // The Mac window's content minimum (windowResizability contentMinSize) and an inset's minimum both ask this.
    _ = shell.sizeThatFits(in: .zero)
    #expect(places.minimum == 0, "the shell's minimum size measured its places \(places.minimum) times")
    // The guard's own check: the ZStack the places sat in before measures them.
    let before = Count()
    let stacked = NSHostingController(rootView: Shell(shown: 0, places: before, fill: false))
    before.minimum = 0
    _ = stacked.sizeThatFits(in: .zero)
    #expect(before.minimum > 0)
    // The shell stacks its places in a Fill on every platform.
    let root = try source("App/RootView.swift")
    #expect(root.range(of: #"var places: some View \{\s*Fill \{"#, options: .regularExpression) != nil)
}

@MainActor @Test func aShelfMeasuresItsCardsOncePerWidth() throws {
    // A lazy page asks each row above and around the screen for its height on every scroll step, at its own width
    // and at proposed heights that change: a strip is asked at its width alone, so its cards are measured once.
    let cards = Count()
    let strip = NSHostingController(rootView: NaturalHeight {
        Probe(count: cards) { Color.gray.frame(width: 100, height: 150) }
    })
    let first = strip.sizeThatFits(in: CGSize(width: 500, height: 1000))
    let asked = cards.asked
    for height in [10.0, 300, 1000] { _ = strip.sizeThatFits(in: CGSize(width: 500, height: height)) }
    #expect(first.height == 150)
    #expect(asked > 0)
    #expect(cards.asked == asked, "the strip measured its cards \(cards.asked - asked) more times at the same width")
    // The page's strip asks the same way: a shelf of real cards, asked again at its width, measures none of them.
    let shelfCards = Count()
    let shelf = NSHostingController(rootView: Shelf(spacing: 10, room: 12) {
        ForEach(0..<6, id: \.self) { _ in Probe(count: shelfCards) { Color.gray.frame(width: 100, height: 150) } }
    })
    _ = shelf.sizeThatFits(in: CGSize(width: 500, height: 1000))
    let shelfAsked = shelfCards.asked
    for height in [10.0, 300, 1000] { _ = shelf.sizeThatFits(in: CGSize(width: 500, height: height)) }
    #expect(shelfCards.asked == shelfAsked, "a shelf measured its cards \(shelfCards.asked - shelfAsked) more times")
}

@MainActor @Test func aCardAScrollCarriesUnderAStillPointerDoesNotLight() {
    // A trackpad scroll moves the page under a resting pointer: each card it carries there used to light and fade,
    // a state change and an animation on every step.
    _ = PointerRest.moved(to: NSPoint(x: 10, y: 10))
    #expect(!PointerRest.moved(to: NSPoint(x: 10, y: 10)))
    #expect(PointerRest.moved(to: NSPoint(x: 11, y: 10)))
    // Carried under a still pointer: stays unlit; the pointer then moves inside it: lights; leaves: goes out.
    #expect(!CardHover.next(hovered: false, inside: true, pointerMoved: false))
    #expect(CardHover.next(hovered: false, inside: true, pointerMoved: true))
    #expect(CardHover.next(hovered: true, inside: true, pointerMoved: false))
    #expect(!CardHover.next(hovered: true, inside: false, pointerMoved: true))
}

@MainActor @Observable private final class Shown {
    var url: String?
}

/// One art view whose url changes, as the hero's backdrop does on each rotation.
private struct Rotating: View {
    let shown: Shown

    var body: some View {
        ArtImage(url: shown.url).frame(width: 20, height: 20)
    }
}

private func solid(red: CGFloat, green: CGFloat, blue: CGFloat) throws -> DecodedArt {
    let space = try #require(CGColorSpace(name: CGColorSpace.sRGB))
    let context = try #require(CGContext(
        data: nil, width: 8, height: 8, bitsPerComponent: 8, bytesPerRow: 0, space: space,
        bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue))
    context.setFillColor(CGColor(red: red, green: green, blue: blue, alpha: 1))
    context.fill(CGRect(x: 0, y: 0, width: 8, height: 8))
    return DecodedArt(try #require(context.makeImage()))
}

@MainActor private func centre(of view: NSView) throws -> NSColor {
    view.layoutSubtreeIfNeeded()
    let rep = try #require(view.bitmapImageRepForCachingDisplay(in: view.bounds))
    view.cacheDisplay(in: view.bounds, to: rep)
    return try #require(rep.colorAt(x: rep.pixelsWide / 2, y: rep.pixelsHigh / 2)?.usingColorSpace(.sRGB))
}

@MainActor @Test func artDrawsTheCurrentURLsCachedPictureOnEveryChange() async throws {
    // A view that drew cached art for one url and is handed another (the hero's third rotation) draws the new url's
    // picture, never the one it held for the last.
    let pictures = [("red", try solid(red: 1, green: 0, blue: 0)), ("green", try solid(red: 0, green: 1, blue: 0)),
                    ("blue", try solid(red: 0, green: 0, blue: 1))]
    for (name, art) in pictures {
        let url = try #require(URL(string: "https://art.test/\(name).jpg"))
        ArtLoader.shared.remember(art, for: url, maxPixels: ArtSize.poster)
    }
    let shown = Shown()
    let window = NSWindow(
        contentRect: CGRect(x: 0, y: 0, width: 20, height: 20), styleMask: .borderless, backing: .buffered, defer: false)
    let host = NSHostingView(rootView: Rotating(shown: shown))
    window.contentView = host
    for name in ["red", "green", "blue", "red", "blue"] {
        shown.url = "https://art.test/\(name).jpg"
        for _ in 0..<5 {
            try await Task.sleep(for: .milliseconds(20))
            host.layoutSubtreeIfNeeded()
        }
        let colour = try centre(of: host)
        let channels = ["red": colour.redComponent, "green": colour.greenComponent, "blue": colour.blueComponent]
        #expect(channels.max { $0.value < $1.value }?.key == name, "\(name)'s url drew \(colour)")
    }
}

@MainActor @Observable private final class StripInputs {
    var typeSize = DynamicTypeSize.large
    var card: CGFloat = 20
    /// The strip's laid-out height.
    var height: CGFloat = 0
}

/// A card that grows with the text size, as a cast name does (the Mac has no Dynamic Type of its own to scale a
/// font), and with its own content.
private struct Sized: View {
    let base: CGFloat
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        Color.gray.frame(width: 50, height: base + (typeSize.isAccessibilitySize ? 60 : 0))
    }
}

/// A strip of a name and a card, offered no height, as a row of a page.
private struct Strip: View {
    let inputs: StripInputs

    var body: some View {
        Shelf(spacing: 10) {
            // A lazy strip takes its cross size from its first card, with or without this layout (trunk too).
            Sized(base: inputs.card)
            Color.gray.frame(width: 50, height: 10)
        }
        .environment(\.dynamicTypeSize, inputs.typeSize)
        .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { inputs.height = $0 }
        .fixedSize(horizontal: false, vertical: true)
        .frame(maxHeight: .infinity, alignment: .top)
    }
}

@MainActor @Test func aShelfMeasuresAgainForLargerTextAndATallerCard() async throws {
    // A strip is asked at its width alone, never told a height it keeps: a larger text size (the cast's names) or a
    // taller card measures it again, where a kept height would clip it or let it overlap the next row.
    let inputs = StripInputs()
    let host = NSHostingView(rootView: Strip(inputs: inputs))
    host.frame = CGRect(x: 0, y: 0, width: 300, height: 400)
    let height = { () async throws -> CGFloat in
        for _ in 0..<5 {
            try await Task.sleep(for: .milliseconds(20))
            host.layoutSubtreeIfNeeded()
        }
        return inputs.height
    }
    let small = try await height()
    inputs.typeSize = .accessibility3
    let large = try await height()
    #expect(large > small, "the text grew from \(small) to \(large)")
    inputs.card = 300
    let tall = try await height()
    #expect(tall >= 300, "a 300 pt card left the strip at \(tall)")
}
