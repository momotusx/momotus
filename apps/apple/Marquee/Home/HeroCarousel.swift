import SwiftUI

#if !os(tvOS)
/// The carousel: the core's pages side by side, swiped by hand or paged on input idle, the dots in the hero's foot,
/// the large title and the profile button fixed over them (HomeScreen's overlay). The page shown plays its trailer
/// under its art.
struct HeroCarousel: View {
    let heroes: [Hero]
    let height: CGFloat
    var play: (Tile) -> Void = { _ in }
    @Environment(\.metrics) private var metrics
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.scenePhase) private var scenePhase
    @State private var pager = HeroPagerModel(count: 0)
    @State private var page: String?
    private var trailer: TrailerState { .shared }
    @State private var visible = true
    /// The pointer rests on the hero: the clock holds where it is, while this app is the one in use (a pointer
    /// passing over a window behind another app's holds nothing).
    @State private var hovering = false
    @State private var sliding = false
    @State private var pageHeights: [String: CGFloat] = [:]
    /// Pages whose trailer ended, failed or was cut this session.
    @State private var spent: Set<String> = []

    #if os(macOS)
    @Environment(\.controlActiveState) private var windowState
    /// The key window of the app in front: the Mac's scene stays active behind other apps' windows.
    private var inUse: Bool { windowState == .key }
    #else
    private var inUse: Bool { scenePhase == .active }
    #endif
    /// The window is on screen: a Mac window behind another app's still pages, as a TV left on does.
    private var shown: Bool { scenePhase != .background }
    private var index: Int { heroes.firstIndex { $0.tile.id == page } ?? 0 }
    private var running: Bool {
        HeroPagerModel.runs(visible: visible, active: shown, reduceMotion: reduceMotion, trailerPlaying: trailer.playing, held: (hovering && inUse) || sliding, pages: heroes.count)
    }

    var body: some View {
        ScrollView(.horizontal) {
            LazyHStack(spacing: 0) {
                ForEach(heroes, id: \.tile.id) { hero in
                    let on = hero.tile.id == (page ?? heroes.first?.tile.id)
                    HeroView(
                        hero: hero, height: height, play: play, showsTitle: false,
                        artOpacity: on && trailer.shown ? 0 : 1,
                        under: on ? AnyView(TrailerSurface()) : nil,
                        parallax: true)
                        .containerRelativeFrame(.horizontal)
                        // B2's hard edge: each page's art, scaled for its lag, stays inside its own page.
                        .clipped()
                        .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { pageHeights[hero.tile.id] = $0 }
                }
            }
            .scrollTargetLayout()
        }
        .scrollTargetBehavior(.paging)
        .scrollPosition(id: $page)
        .scrollIndicators(.hidden)
        // The tallest page sets the height, so the rows below never jump on a slide.
        .frame(height: max(height, pageHeights.values.max() ?? height))
        .onScrollPhaseChange { _, phase in
            // A page's 8 s start once it has settled, as on the web and Android.
            sliding = phase != .idle
            if phase == .interacting { pager.manual() }
        }
        .onScrollVisibilityChange(threshold: 0.5) { visible = $0 }
        .overlay(alignment: .bottom) { dots }
        .overlay(alignment: .topLeading) {
            // The iPhone's large title, fixed over the pages; each page keeps its room (HeroView's spacer).
            if !metrics.wide {
                Text("Home")
                    .display(34)
                    .foregroundStyle(Theme.Palette.text)
                    .accessibilityAddTraits(.isHeader)
                    .padding(.leading, 16)
                    .padding(.top, 2)
            }
        }
        .simultaneousGesture(DragGesture(minimumDistance: 0).onChanged { _ in pager.input() })
        .onHover { hovering = $0 }
        .onAppear {
            pager.count = heroes.count
            // The network's kind is known before the first trailer is due.
            _ = TrailerNetwork.shared
        }
        .onChange(of: heroes.map(\.tile.id)) { _, ids in pager.count = ids.count }
        // The clock: one value for the pill and the advance (decision 7).
        .task {
            // The time that really passed: a sleep can run long (a busy main thread, App Nap behind other windows),
            // and the dwell must still be 8 s.
            let clock = ContinuousClock()
            var last = clock.now
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(1.0 / 30))
                let now = clock.now
                let dt = (now - last) / .seconds(1)
                last = now
                if pager.tick(dt, running: running) { advance() }
            }
        }
        // The pages either side are drawn only once they slide in: their art is fetched ahead, so a slide never
        // brings a blank page in.
        .task(id: index) { await prefetch(around: index) }
        .onChange(of: trailer.fraction) { _, f in if trailer.playing { pager.trailer(f) } }
        // The page shown plays its trailer once it is due and every condition holds (decision 5).
        .task(id: TrailerKey(page: page ?? heroes.first?.tile.id, visible: visible, active: shown)) { await startTrailer() }
        .onDisappear { TrailerPlayer.shared.stop() }
        #if os(macOS)
        // The Mac has no swipe without a trackpad: arrows at the edges under the pointer, and Left and Right
        // once the hero has focus (a click on it gives it).
        .overlay { if hovering && heroes.count > 1 { arrows.transition(.opacity) } }
        .animation(Theme.Motion.panel, value: hovering)
        .focusable(interactions: .edit)
        .focusEffectDisabled()
        .onKeyPress(.leftArrow) { key(-1) }
        .onKeyPress(.rightArrow) { key(1) }
        #endif
    }

    #if os(macOS)
    private var arrows: some View {
        HStack {
            arrow("chevron-left", by: -1, label: "Previous")
            Spacer()
            arrow("chevron-right", by: 1, label: "Next")
        }
        .padding(.horizontal, 16)
    }

    /// No wrap, as with the keys: the first page has no Previous, the last no Next.
    private func arrow(_ glyph: String, by delta: Int, label: String) -> some View {
        let open = pager.step(from: index, by: delta) != nil
        return Button { _ = key(delta) } label: {
            Glyph(glyph, size: 20)
                .foregroundStyle(.white)
                .frame(width: 44, height: 44)
                .background(SecondaryFill(shape: Circle()))
                .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .opacity(open ? 1 : 0)
        .disabled(!open)
        .accessibilityLabel(label)
    }
    #endif

    @ViewBuilder private var dots: some View {
        if heroes.count > 1 {
            HeroDots(count: heroes.count, active: index, progress: pager.progress, names: heroes.map(\.tile.name)) { to in
                pager.manual()
                withAnimation(Theme.Motion.heroSlide) { page = heroes[to].tile.id }
            }
            .padding(.bottom, metrics.wide ? 28 : 20)
        }
    }

    private func prefetch(around index: Int) async {
        guard heroes.count > 1 else { return }
        for i in [index + 1, index - 1] {
            let tile = heroes[(i + heroes.count) % heroes.count].tile
            if let art = (tile.background ?? tile.plainPoster).flatMap(URL.init(string:)) {
                _ = await ArtLoader.shared.image(art, maxPixels: ArtSize.backdrop)
            }
            if let logo = tile.logo.flatMap(URL.init(string:)) {
                _ = await ArtLoader.shared.image(logo, maxPixels: ArtSize.logo)
            }
        }
    }

    private func advance() {
        let to = pager.next(after: index)
        if reduceMotion { page = heroes[to].tile.id } else { withAnimation(Theme.Motion.heroSlide) { page = heroes[to].tile.id } }
    }

    private func key(_ delta: Int) -> KeyPress.Result {
        guard let to = pager.step(from: index, by: delta) else { return .ignored }
        pager.manual()
        withAnimation(Theme.Motion.heroSlide) { page = heroes[to].tile.id }
        return .handled
    }

    private func startTrailer() async {
        TrailerPlayer.shared.stop()
        guard let hero = heroes.first(where: { $0.tile.id == (page ?? heroes.first?.tile.id) }),
              let id = HeroTrailers.checkedId(hero.trailer), visible, shown else { return }
        try? await Task.sleep(for: .milliseconds(700))
        guard !Task.isCancelled else { return }
        let signals = TrailerSignals.now(visible: visible, reducedMotion: reduceMotion, hasTrailer: !spent.contains(hero.tile.id))
        guard signals.allowed else { return }
        TrailerPlayer.shared.play(id, into: trailer) { why in
            if why != "stopped" { spent.insert(hero.tile.id) }
            // Played to its cap: the art fades back, and the same clock pages on 600 ms later.
            if why == "cap" { pager.trailer(1 - 0.6 / pager.dwell) }
        }
    }
}

private struct TrailerKey: Equatable {
    let page: String?
    let visible: Bool
    let active: Bool
}

/// The sound switch, shown only while a trailer is on screen, never in a walk.
struct TrailerMute: View {
    @State private var sound = false

    var body: some View {
        Button {
            sound.toggle()
            TrailerPlayer.shared.sound(sound)
        } label: {
            Glyph(sound ? "volume-2" : "volume-x", size: 18)
                .foregroundStyle(.white)
                .frame(width: 36, height: 36)
                .background(SecondaryFill(shape: Circle()))
        }
        .buttonStyle(FocusPlainStyle(shape: Circle(), scale: Theme.Focus.scaleCard))
        .accessibilityLabel(sound ? "Sound on" : "Sound off")
    }
}
#endif

/// The page dots (the iPhone TV app's): dots on a fixed pitch, the active one a capsule whose white fill grows over
/// the page's time; each a button off the TV.
struct HeroDots: View {
    let count: Int
    let active: Int
    let progress: Double
    let names: [String]
    var pick: (Int) -> Void = { _ in }
    @Environment(\.metrics) private var metrics

    var body: some View {
        HStack(spacing: 9) {
            ForEach(0..<count, id: \.self) { i in
                let on = i == active
                Button { pick(i) } label: {
                    Capsule()
                        .fill(Color.white.opacity(on ? 0.35 : 0.6))
                        .overlay(alignment: .leading) {
                            if on {
                                GeometryReader { geo in
                                    Capsule().fill(.white).frame(width: geo.size.width * progress)
                                }
                            }
                        }
                        .frame(width: on ? 24 : 7, height: 7)
                        .padding(.vertical, 7)
                        .contentShape(Rectangle())
                        .animation(Theme.Motion.panel, value: on)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Page \(i + 1) of \(count), \(names[safe: i] ?? "")")
                .accessibilityAddTraits(on ? .isSelected : [])
            }
        }
    }
}
