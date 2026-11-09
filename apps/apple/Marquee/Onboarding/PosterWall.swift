import SwiftUI

/// The poster wall behind every first-run step (plans/first-screens.md 3.1, round2.html combo A): the default catalog's
/// posters in turned columns, drifting on Welcome only (`WallMotion.runs`), still under a 62 percent dim and an 8 pt
/// blur under every sheet. A tile whose poster has not come is the empty `g6` rounded rectangle; posters fade in
/// (at once under Reduce Motion). Decorative: hidden from assistive tech and never hit.
struct PosterWall: View {
    let posters: [String]
    let columns: Int
    let running: Bool
    /// A sheet is over it: dimmed and blurred.
    let dimmed: Bool
    /// Welcome's ramp under its words: the phone's or the wide one.
    var wide = false
    @State private var art: [String: DecodedArt] = [:]
    @Environment(\.accessibilityReduceMotion) private var systemReduceMotion

    var body: some View {
        GeometryReader { box in
            let layout = WallLayout(screen: box.size, columns: columns, count: posters.count)
            TimelineView(.animation(paused: !running)) { context in
                let t = WallClock.shared.elapsed(at: context.date)
                HStack(alignment: .top, spacing: WallLayout.gap) {
                    ForEach(0..<columns, id: \.self) { column in
                        VStack(spacing: WallLayout.gap) {
                            ForEach(0..<(layout.rows * 2), id: \.self) { row in
                                tile(layout.poster(column, row, in: posters), size: layout.tile)
                            }
                        }
                        .offset(y: layout.offset(column, at: t))
                    }
                }
                .frame(width: layout.wall.width, height: layout.wall.height, alignment: .topLeading)
                .rotationEffect(.degrees(WallLayout.degrees))
                .position(x: box.size.width / 2, y: box.size.height / 2)
            }
            .blur(radius: dimmed ? Theme.Wall.blur : 0)
            .task(id: box.size) { await logWhenSettled(layout) }
        }
        .overlay { shade }
        .clipped()
        .background(Color.black)
        .ignoresSafeArea()
        .allowsHitTesting(false)
        .accessibilityHidden(true)
        .onChange(of: running, initial: true) { _, now in WallClock.shared.set(running: now, at: .now) }
        .task(id: posters) { await load() }
    }

    private var reduceMotion: Bool { systemReduceMotion || WallFlags.reduceMotion }

    @ViewBuilder private func tile(_ url: String?, size: CGSize) -> some View {
        let shape = RoundedRectangle(cornerRadius: WallLayout.radius, style: .continuous)
        ZStack {
            shape.fill(Theme.Palette.surfacePhone)
            if let url, let image = art[url] {
                Image(decorative: image.image, scale: 1).resizable().aspectRatio(contentMode: .fill).transition(.opacity)
            }
        }
        .frame(width: size.width, height: size.height)
        .clipShape(shape)
    }

    /// Welcome's ramp (round2.html:179, :181), or the sheets' dim.
    @ViewBuilder private var shade: some View {
        if dimmed {
            Theme.Wall.dim
        } else {
            LinearGradient(stops: (wide ? WallLayout.welcomeWide : WallLayout.welcomePhone).map { .init(color: .black.opacity($0.alpha), location: $0.at) },
                           startPoint: .top, endPoint: .bottom)
        }
    }

    /// Every poster once, each drawn as it lands.
    private func load() async {
        let urls = Array(Set(posters)).filter { art[$0] == nil }
        await withTaskGroup(of: (String, DecodedArt?).self) { group in
            for url in urls {
                group.addTask { (url, await ArtLoader.shared.image(url, maxPixels: ArtSize.poster)) }
            }
            for await (url, image) in group {
                guard let image else { continue }
                WallLog.shared.drew()
                withAnimation(reduceMotion ? nil : .easeOut(duration: 0.25)) { art[url] = image }
            }
        }
    }

    /// The walk's line, once per app run: the first poster's time and the tiles filled at 6 s over those on screen.
    private func logWhenSettled(_ layout: WallLayout) async {
        guard layout.screen.width > 0, WallLog.shared.start() else { return }
        try? await Task.sleep(for: .seconds(6))
        guard !Task.isCancelled else { return }
        let shown = layout.visible(at: WallClock.shared.elapsed(at: .now))
        let filled = shown.filter { layout.poster($0.column, $0.row, in: posters).map { art[$0] != nil } == true }.count
        WallLog.shared.settled(filled: filled, visible: shown.count)
    }
}

/// The walk flags the wall reads: `-MQReduceMotion YES` stills it as Reduce Motion does, no system setting changed.
enum WallFlags {
    static var reduceMotion: Bool {
        #if DEBUG
        DebugHooks.reduceMotion
        #else
        false
        #endif
    }
}

/// `first-wall poster <ms> tiles6s <n>/<visible>` (plans/first-screens.md 3.1, 9): the bench's source, in walk and
/// debug builds only, in the unified log (the Mac, the iPhone's console) and the walk log.
@MainActor final class WallLog {
    static let shared = WallLog()
    private var began: ContinuousClock.Instant?
    private var firstMs: Int?
    private var done = false

    /// The wall's first frame; false once this run has logged or begun.
    func start() -> Bool {
        guard began == nil, !done else { return false }
        began = .now
        return true
    }

    func drew() {
        guard firstMs == nil, let began else { return }
        let elapsed = ContinuousClock.now - began
        firstMs = Int(elapsed.components.seconds * 1000 + elapsed.components.attoseconds / 1_000_000_000_000_000)
    }

    func settled(filled: Int, visible: Int) {
        guard !done else { return }
        done = true
        #if DEBUG
        let line = "first-wall poster \(firstMs.map(String.init) ?? "none") tiles6s \(filled)/\(visible)"
        coreLog.notice("\(line, privacy: .public)")
        DebugHooks.note(line)
        #endif
    }
}
