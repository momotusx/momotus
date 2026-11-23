#if MARQUEE_WALK
import os
import QuartzCore
import SwiftUI
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// `-MQNavWalk YES` (walk builds only): 20 s after the shell shows, the app walks itself through the shared walk of
/// plans/perf-nav.md (Home down 3 rows, right 5, a title, its Versions, Back twice, Discover, right 3, Search, 3
/// letters, Back), then scrolls Home from top to bottom and back, three rounds, with no pointer, key or touch sent.
/// Each press logs one `metric nav` line: `press_ms`, from the press to the presented time of the first frame after it,
/// and the frames of the 2 s that follow (late frames, hitch time per second, the main thread's CPU). The scroll logs
/// every late frame with its time, and the core pushes that arrived during it. On a phone the lines also go to
/// `Documents/walk.log`.
///
/// `-MQWalkOff art,pushes,hover,hoverfix` turns one suspect off for an A/B of the same walk: art never loads, Home
/// applies no core push during the scroll, the cards' hover handlers do nothing, or a card carried under a still
/// pointer lights again as it did. `-MQWalkHover YES` tells the window the pointer rests over the shelves on each
/// scroll step; `-MQNavOnly scroll` walks rest and the scroll alone.
@MainActor final class NavWalk: NSObject {
    static var on: Bool { UserDefaults.standard.bool(forKey: "MQNavWalk") }
    static let versions = Notification.Name("app.momotus.walk.versions")
    static let versionsBack = Notification.Name("app.momotus.walk.versionsBack")
    static let discoverRight = Notification.Name("app.momotus.walk.discoverRight")

    enum Suspect: String { case art, pushes, hover, hoverfix }
    nonisolated static let offList: Set<String> =
        Set((UserDefaults.standard.string(forKey: "MQWalkOff") ?? "").split(separator: ",").map(String.init))
    nonisolated static func off(_ suspect: Suspect) -> Bool { offList.contains(suspect.rawValue) }

    /// Home's own scroll view, found from inside its page (a hidden place's page can sit over it in the view tree).
    #if os(macOS)
    static weak var homeScroll: NSScrollView?
    #else
    static weak var homeScroll: UIScrollView?
    #endif
    /// Home's scroll is under way: `-MQWalkOff pushes` holds Home's updates meanwhile.
    static var scrolling = false
    /// Hover callbacks the cards got, and `-MQWalkHover YES`: each scroll step tells the window the pointer rests at
    /// one point over the shelves, as a trackpad scroll under a still pointer does (the owner's pointer is never moved).
    static var hoverCalls = 0
    static let feedsHover = UserDefaults.standard.bool(forKey: "MQWalkHover")

    /// Core pushes by view, counted on the core's thread with the time their JSON took to decode.
    nonisolated static let pushes = OSAllocatedUnfairLock(initialState: [String: (count: Int, decodeMs: Double)]())
    nonisolated static func pushed(_ view: String, seconds: Double) {
        pushes.withLock { $0[view, default: (0, 0)].count += 1; $0[view, default: (0, 0)].decodeMs += seconds * 1000 }
    }

    /// What the walk can do to the shell.
    struct Driver {
        let select: (AppTab) -> Void
        let search: (String) -> Void
        let push: (TitleRoute) -> Void
        let popHome: () -> Void
    }

    enum Step {
        case rest, rowDown, shelfRight, title(TitleRoute), versions, versionsBack, back, tab(AppTab), discoverRight,
             type(String), scroll

        var name: String {
            switch self {
            case .rest: "rest"
            case .rowDown: "home_down"
            case .shelfRight: "home_right"
            case .title: "open_title"
            case .versions: "open_versions"
            case .versionsBack: "back_versions"
            case .back: "back_title"
            case .tab(let tab): "tab_\(tab.rawValue)"
            case .discoverRight: "discover_right"
            case .type: "type_letter"
            case .scroll: "home_scroll"
            }
        }
    }

    static let script: [Step] = [.rest, .rowDown, .rowDown, .rowDown]
        + Array(repeating: .shelfRight, count: 5)
        + [.title(TitleRoute(kind: "movie", id: "tt0468569", name: "")), .versions, .versionsBack, .back, .tab(.discover)]
        + Array(repeating: .discoverRight, count: 3)
        + [.tab(.search), .type("b"), .type("a"), .type("t"), .tab(.home), .scroll]
    static let rounds = 3
    /// `-MQNavOnly scroll`: rest and Home's scroll alone, for the scroll A/B.
    static let scrollOnly = UserDefaults.standard.string(forKey: "MQNavOnly") == "scroll"
    /// How long each press's frames are counted: the page's data and art land in it.
    static let window = 2.0
    static let scrollSpeed = 2400.0
    static let rowStep = 320.0

    private var intervals: [(at: Double, ms: Double)] = []
    private var last: CFTimeInterval?
    private var frame = 1.0 / 60
    private var pressAt: CFTimeInterval?
    private var pressMs: Double?
    private var visible = true
    /// The main run loop's busy stretches (woken to going back to sleep), for a window out of sight too.
    private var busy: [(at: Double, ms: Double)] = []
    private var woke: CFTimeInterval?
    private var pressBusyMs: Double?
    private var scroll: Scroll?
    private var typed = ""

    static func run(_ driver: Driver) async {
        let walk = NavWalk()
        #if os(macOS)
        guard let window = NSApp.windows.max(by: { $0.frame.height < $1.frame.height }) else { return }
        var visible = await ScrollWalk.showOnSomeScreen(window)
        if !visible { visible = await cornered(window) }
        // A window out of sight gets no frames: nothing to measure, and the scroll step would wait for ever.
        // Out of sight there are no frames: the walk then times the main thread's run loop alone (busy_* fields).
        walk.visible = visible
        report("metric nav window min=\(Int(window.contentMinSize.width))x\(Int(window.contentMinSize.height)) size=\(Int(window.frame.width))x\(Int(window.frame.height)) off=\(offList.sorted().joined(separator: ","))")
        let link = window.displayLink(target: walk, selector: #selector(tick))
        #else
        let link = CADisplayLink(target: walk, selector: #selector(tick))
        link.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, preferred: 120)
        report("metric nav start off=\(offList.sorted().joined(separator: ","))")
        #endif
        link.add(to: .main, forMode: .common)
        let observer = CFRunLoopObserverCreateWithHandler(nil, CFRunLoopActivity.afterWaiting.rawValue | CFRunLoopActivity.beforeWaiting.rawValue, true, Int.max) { _, activity in
            MainActor.assumeIsolated { walk.runLoop(activity) }
        }
        CFRunLoopAddObserver(CFRunLoopGetMain(), observer, .commonModes)
        defer { link.invalidate(); CFRunLoopRemoveObserver(CFRunLoopGetMain(), observer, .commonModes) }
        for round in 1...rounds {
            driver.search("")
            walk.typed = ""
            for step in script where !scrollOnly || ["rest", "home_scroll"].contains(step.name) {
                let (seconds, cpu) = await walk.measure(step, driver)
                walk.log(round: round, step: step, seconds: seconds, cpu: cpu)
            }
        }
        report("metric nav done rounds=\(rounds)")
    }

    #if os(macOS)
    /// The owner's windows cover the walk's on every screen: it sits at a screen's bottom right corner with only its top
    /// left part on screen, never in front and never taking focus, so it still gets frames.
    private static func cornered(_ window: NSWindow) async -> Bool {
        for screen in NSScreen.screens {
            let area = screen.visibleFrame
            window.setFrameOrigin(NSPoint(x: area.maxX - 320, y: area.minY - window.frame.height + 200))
            guard (try? await Task.sleep(for: .seconds(2))) != nil else { break }
            if window.occlusionState.contains(.visible) { return true }
        }
        return false
    }
    #endif

    private func measure(_ step: Step, _ driver: Driver) async -> (Double, UInt64) {
        try? await Task.sleep(for: .milliseconds(400))
        intervals = []
        last = nil
        pressMs = nil
        pressBusyMs = nil
        busy = []
        let cpu = clock_gettime_nsec_np(CLOCK_THREAD_CPUTIME_ID)
        let start = CACurrentMediaTime()
        pressAt = start
        switch step {
        case .rest: break
        case .rowDown: Scroll.page()?.jump(by: Self.rowStep)
        case .shelfRight: Scroll.shelf()?.jump(by: Self.cardStep)
        case .title(let route): driver.push(route)
        case .versions: NotificationCenter.default.post(name: Self.versions, object: nil)
        case .versionsBack: NotificationCenter.default.post(name: Self.versionsBack, object: nil)
        case .back: driver.popHome()
        case .tab(let tab): driver.select(tab)
        case .discoverRight: NotificationCenter.default.post(name: Self.discoverRight, object: nil)
        case .type(let letter):
            typed += letter
            driver.search(typed)
        case .scroll:
            Scroll.page()?.jump(to: 0)
            try? await Task.sleep(for: .milliseconds(400))
            NavWalk.pushes.withLock { $0 = [:] }
            NavWalk.hoverCalls = 0
            intervals = []
            last = nil
            if let found = Scroll.page() {
                scroll = found
                Self.scrolling = true
                await found.walk(speed: Self.scrollSpeed)
                Self.scrolling = false
                scroll = nil
            }
        }
        if case .scroll = step {} else { try? await Task.sleep(for: .seconds(Self.window)) }
        return (CACurrentMediaTime() - start, clock_gettime_nsec_np(CLOCK_THREAD_CPUTIME_ID) - cpu)
    }

    private static var cardStep: Double {
        #if os(macOS)
        Double(Theme.TileDesktop.gap) + 180
        #else
        Double(Theme.Tile.gap) + 110
        #endif
    }

    private func log(round: Int, step: Step, seconds: Double, cpu: UInt64) {
        let ms = intervals.map(\.ms)
        let sorted = ms.sorted()
        let frameMs = frame * 1000
        let late = intervals.filter { $0.ms > frameMs * 1.5 }
        let hitch = late.reduce(0) { $0 + $1.ms - frameMs }
        let at = { (q: Double) in sorted.isEmpty ? 0 : sorted[min(sorted.count - 1, Int(Double(sorted.count) * q))] }
        var line = String(
            format: "round=%d step=%@ press_ms=%.1f frames=%d frame_ms=%.2f p50_ms=%.2f p90_ms=%.2f p99_ms=%.2f max_ms=%.1f late=%d stalls_50=%d hitch_ms_per_s=%.1f main_cpu_ms_per_s=%.0f busy_press_ms=%.1f busy_max_ms=%.1f busy_over_16=%d busy_ms_per_s=%.0f visible=%d",
            round, step.name, pressMs ?? -1, ms.count, frameMs, at(0.5), at(0.9), at(0.99), sorted.last ?? 0, late.count,
            ms.filter { $0 > 50 }.count, hitch / seconds, Double(cpu) / 1e6 / seconds, pressBusyMs ?? -1, busy.map(\.ms).max() ?? 0,
            busy.filter { $0.ms > 16.7 }.count, busy.reduce(0) { $0 + $1.ms } / seconds, visible ? 1 : 0)
        if case .scroll = step {
            let pushed = NavWalk.pushes.withLock { $0 }
            line += " seconds=\(String(format: "%.2f", seconds)) hover_calls=\(NavWalk.hoverCalls) pushes=" + pushed.sorted { $0.key < $1.key }
                .map { "\($0.key):\($0.value.count):\(String(format: "%.1f", $0.value.decodeMs))ms" }.joined(separator: ",")
            // Each late frame: when (ms into the scroll) and how long, for the trace.
            line += " late_at=" + late.map { String(format: "%.0f/%.0f", $0.at * 1000, $0.ms) }.joined(separator: ",")
            line += " busy_at=" + busy.filter { $0.ms > 16.7 }.map { String(format: "%.0f/%.0f", $0.at * 1000, $0.ms) }.joined(separator: ",")
        }
        Self.report("metric nav \(line)")
    }

    private static func report(_ line: String) {
        coreLog.notice("\(line, privacy: .public)")
        #if os(iOS)
        DebugHooks.note(line)
        #endif
    }

    /// One run-loop pass: from waking to going back to sleep. The first pass that ends after a press is the one that
    /// handled it and committed its frame.
    func runLoop(_ activity: CFRunLoopActivity) {
        let now = CACurrentMediaTime()
        if activity == .afterWaiting { woke = now; return }
        guard let woke else { return }
        self.woke = nil
        let since = pressAt ?? now
        busy.append((woke - since, (now - woke) * 1000))
        if let pressAt, pressBusyMs == nil, now >= pressAt { pressBusyMs = (now - pressAt) * 1000 }
    }

    @objc private func tick(_ link: CADisplayLink) {
        let now = link.timestamp
        if let pressAt, pressMs == nil, now >= pressAt { pressMs = (link.targetTimestamp - pressAt) * 1000 }
        if let last { intervals.append((now - (pressAt ?? now), (now - last) * 1000)) }
        last = now
        frame = link.targetTimestamp - link.timestamp
        scroll?.step(now)
    }

    /// The page's tallest scroll view on screen (or the first sideways shelf in sight), moved as a finger, a trackpad
    /// or an arrow key does.
    @MainActor final class Scroll {
        #if os(macOS)
        let view: NSScrollView
        #else
        let view: UIScrollView
        #endif
        private var start: CFTimeInterval?
        private var speed = 0.0
        private var depth = 0.0
        private var done: CheckedContinuation<Void, Never>?

        #if os(macOS)
        init(_ view: NSScrollView) { self.view = view }

        private static func all() -> [NSScrollView] {
            guard let window = NSApp.windows.max(by: { $0.frame.height < $1.frame.height }) else { return [] }
            func all(_ view: NSView) -> [NSScrollView] {
                if view.isHidden || view.alphaValue < 0.01 { return [] }
                return (view as? NSScrollView).map { [$0] + $0.subviews.flatMap(all) } ?? view.subviews.flatMap(all)
            }
            return all(window.contentView ?? NSView())
        }

        private var length: Double { (view.documentView?.frame.height ?? 0) - view.contentView.bounds.height }
        private var offset: CGPoint { view.contentView.bounds.origin }

        func move(x: Double? = nil, y: Double? = nil) {
            let flipped = view.documentView?.isFlipped ?? true
            let nx = x ?? view.contentView.bounds.minX
            let ny = y.map { flipped ? $0 : length - $0 } ?? view.contentView.bounds.minY
            view.contentView.scroll(to: NSPoint(x: nx, y: ny))
            view.reflectScrolledClipView(view.contentView)
        }

        private var y: Double {
            let flipped = view.documentView?.isFlipped ?? true
            return flipped ? view.contentView.bounds.minY : length - view.contentView.bounds.minY
        }

        /// The first sideways shelf in sight (the hero's pager is taller than any shelf).
        static func shelf() -> Scroll? {
            all().first { s in
                guard let root = s.window?.contentView, (s.documentView?.frame.width ?? 0) > s.contentView.bounds.width + 1,
                      s.contentView.bounds.height < 500 else { return false }
                return root.bounds.intersects(s.convert(s.bounds, to: root))
            }.map(Scroll.init)
        }

        func jump(by step: Double) {
            if (view.documentView?.frame.height ?? 0) > view.contentView.bounds.height + 1 { move(y: min(length, y + step)) } else { move(x: offset.x + step) }
        }
        #else
        init(_ view: UIScrollView) { self.view = view }

        private static func all() -> [UIScrollView] {
            let windows = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.flatMap(\.windows)
            guard let window = windows.first(where: \.isKeyWindow) ?? windows.first else { return [] }
            func shown(_ view: UIView) -> Bool {
                var at: UIView? = view
                while let v = at {
                    if v.isHidden || v.alpha < 0.01 || v.layer.opacity < 0.01 { return false }
                    at = v.superview
                }
                return true
            }
            func all(_ view: UIView) -> [UIScrollView] {
                ((view as? UIScrollView).map { [$0] } ?? []) + view.subviews.flatMap(all)
            }
            return all(window).filter { s in shown(s) && window.bounds.intersects(s.convert(s.bounds, to: window)) }
        }

        private var length: Double { view.contentSize.height + view.adjustedContentInset.bottom - view.bounds.height }
        private var y: Double { view.contentOffset.y + view.adjustedContentInset.top }

        func move(x: Double? = nil, y: Double? = nil) {
            view.contentOffset = CGPoint(x: x ?? view.contentOffset.x, y: y.map { $0 - view.adjustedContentInset.top } ?? view.contentOffset.y)
        }

        static func shelf() -> Scroll? {
            all().first { $0.contentSize.width > $0.bounds.width + 1 && $0.contentSize.height <= $0.bounds.height + 1 && $0.bounds.height < 500 }.map(Scroll.init)
        }

        func jump(by step: Double) {
            if view.contentSize.height > view.bounds.height + 1 { move(y: min(length, y + step)) } else { move(x: view.contentOffset.x + step) }
        }
        #endif

        /// The page: the outermost vertical scroll view under the window's centre (a hidden place may hold a taller
        /// one), else the tallest on screen.
        static func page() -> Scroll? {
            if let home = NavWalk.homeScroll, home.window != nil { return Scroll(home) }
            #if os(macOS)
            if let root = NSApp.windows.max(by: { $0.frame.height < $1.frame.height })?.contentView {
                var at = root.hitTest(NSPoint(x: root.bounds.midX, y: root.bounds.midY))
                var found: NSScrollView?
                while let view = at {
                    if let s = view as? NSScrollView, (s.documentView?.frame.height ?? 0) > s.contentView.bounds.height + 1 { found = s }
                    at = view.superview
                }
                if let found { return Scroll(found) }
            }
            return all().filter { ($0.documentView?.frame.height ?? 0) > $0.contentView.bounds.height + 1 }
                .max { ($0.documentView?.frame.height ?? 0) < ($1.documentView?.frame.height ?? 0) }.map(Scroll.init)
            #else
            let windows = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.flatMap(\.windows)
            if let window = windows.first(where: \.isKeyWindow) {
                var at = window.hitTest(CGPoint(x: window.bounds.midX, y: window.bounds.midY), with: nil)
                var found: UIScrollView?
                while let view = at {
                    if let s = view as? UIScrollView, s.contentSize.height > s.bounds.height + 1 { found = s }
                    at = view.superview
                }
                if let found { return Scroll(found) }
            }
            return all().filter { $0.contentSize.height > $0.bounds.height + 1 }.max { $0.contentSize.height < $1.contentSize.height }.map(Scroll.init)
            #endif
        }

        func jump(to y: Double) { move(y: y) }

        /// Top to bottom and back at `speed`, one step per refresh.
        func walk(speed: Double) async {
            self.speed = speed
            depth = length
            guard depth > 0 else { return }
            await withCheckedContinuation { done = $0 }
        }

        func step(_ now: CFTimeInterval) {
            guard done != nil else { return }
            let start = start ?? now
            self.start = start
            let travelled = (now - start) * speed
            // The page grows as rows land: the bottom is wherever it is now.
            depth = max(depth, length)
            guard travelled < 2 * depth else {
                move(y: 0)
                done?.resume()
                done = nil
                return
            }
            move(y: travelled < depth ? travelled : 2 * depth - travelled)
            #if os(macOS)
            if NavWalk.feedsHover { restPointer() }
            #endif
        }

        #if os(macOS)
        /// A mouse-moved at one fixed point over the shelves, sent to the window's hosting view: what SwiftUI hears from a
        /// still pointer while the page moves under it.
        private func restPointer() {
            guard let window = view.window, let root = window.contentView,
                  let event = NSEvent.mouseEvent(with: .mouseMoved, location: NSPoint(x: root.bounds.width * 0.3, y: root.bounds.height * 0.4),
                                                 modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                                                 context: nil, eventNumber: 0, clickCount: 0, pressure: 0) else { return }
            // SwiftUI hears the pointer through the tracking areas of its hosting views.
            func owners(_ view: NSView) -> [NSResponder] {
                view.trackingAreas.compactMap { $0.owner as? NSResponder } + view.subviews.flatMap(owners)
            }
            var seen = Set<ObjectIdentifier>()
            for owner in owners(root) where seen.insert(ObjectIdentifier(owner)).inserted { owner.mouseMoved(with: event) }
        }
        #endif
    }
}

#endif

#if MARQUEE_WALK
/// Set behind Home's page: tells the walk which scroll view is Home's.
#if os(macOS)
struct WalkPageTag: NSViewRepresentable {
    final class Tag: NSView {
        override func viewDidMoveToWindow() { NavWalk.homeScroll = enclosingScrollView }
    }
    func makeNSView(context: Context) -> Tag { Tag() }
    func updateNSView(_ view: Tag, context: Context) {}
}
#else
struct WalkPageTag: UIViewRepresentable {
    final class Tag: UIView {
        override func didMoveToWindow() {
            var at = superview
            while let view = at, !(view is UIScrollView) { at = view.superview }
            NavWalk.homeScroll = at as? UIScrollView
        }
    }
    func makeUIView(context: Context) -> Tag { Tag() }
    func updateUIView(_ view: Tag, context: Context) {}
}
#endif
#endif
