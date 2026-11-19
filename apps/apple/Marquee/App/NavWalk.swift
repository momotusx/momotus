#if MARQUEE_WALK
import QuartzCore
import SwiftUI
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// `-MQNavWalk YES` (walk builds only): 20 s after the shell shows, the app walks itself through the places a viewer
/// moves between (Discover, Search with a query, Settings, Library, Home, a title, its Versions, back twice, Home's
/// scroll), three rounds, with no pointer, key or touch sent. Each step logs one `metric nav` line with the frames
/// that followed it: the first frame after the step (its click-to-frame time), late frames, hitch time per second
/// and the main thread's CPU. On a phone the lines also go to `Documents/walk.log`.
@MainActor final class NavWalk: NSObject {
    static var on: Bool { UserDefaults.standard.bool(forKey: "MQNavWalk") }
    static let versions = Notification.Name("app.momotus.walk.versions")
    static let versionsBack = Notification.Name("app.momotus.walk.versionsBack")

    /// What the walk can do to the shell.
    struct Driver {
        let select: (AppTab) -> Void
        let search: (String) -> Void
        let push: (TitleRoute) -> Void
        let popHome: () -> Void
    }

    enum Step {
        case rest, tab(AppTab), search(String), title(TitleRoute), versions, versionsBack, back, scroll

        var name: String {
            switch self {
            case .rest: "rest"
            case .tab(let tab): "tab_\(tab.rawValue)"
            case .search: "search_query"
            case .title: "title"
            case .versions: "versions"
            case .versionsBack: "versions_back"
            case .back: "title_back"
            case .scroll: "home_scroll"
            }
        }
    }

    static let script: [Step] = [
        .rest, .tab(.discover), .tab(.search), .search("batman"), .tab(.settings), .tab(.library), .tab(.home),
        .title(TitleRoute(kind: "movie", id: "tt0468569", name: "")), .versions, .versionsBack, .back, .scroll,
    ]
    static let rounds = 3
    /// How long each step's frames are counted: the page's data and art land in it.
    static let window = 2.5
    static let scrollSpeed = 2400.0
    static let scrollDepth = 4000.0

    private var intervals: [Double] = []
    private var last: CFTimeInterval?
    private var frame = 1.0 / 60
    private var scroll: Scroll?

    static func run(_ driver: Driver) async {
        let walk = NavWalk()
        #if os(macOS)
        guard let window = NSApp.windows.max(by: { $0.frame.height < $1.frame.height }) else { return }
        let visible = await ScrollWalk.showOnSomeScreen(window)
        report("metric nav window min=\(Int(window.contentMinSize.width))x\(Int(window.contentMinSize.height)) size=\(Int(window.frame.width))x\(Int(window.frame.height))")
        let link = window.displayLink(target: walk, selector: #selector(tick))
        #else
        let visible = true
        let link = CADisplayLink(target: walk, selector: #selector(tick))
        link.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, preferred: 120)
        #endif
        link.add(to: .main, forMode: .common)
        defer { link.invalidate() }
        for round in 1...rounds {
            driver.search("")
            for step in script {
                let (seconds, cpu) = await walk.measure(step, driver)
                walk.log(round: round, step: step, seconds: seconds, cpu: cpu, visible: visible)
            }
        }
        report("metric nav done rounds=\(rounds)")
    }

    private func measure(_ step: Step, _ driver: Driver) async -> (Double, UInt64) {
        try? await Task.sleep(for: .milliseconds(300))
        intervals = []
        last = nil
        let cpu = clock_gettime_nsec_np(CLOCK_THREAD_CPUTIME_ID)
        let start = CACurrentMediaTime()
        switch step {
        case .rest: break
        case .tab(let tab): driver.select(tab)
        case .search(let text): driver.search(text)
        case .title(let route): driver.push(route)
        case .versions: NotificationCenter.default.post(name: Self.versions, object: nil)
        case .versionsBack: NotificationCenter.default.post(name: Self.versionsBack, object: nil)
        case .back: driver.popHome()
        case .scroll:
            if let found = Scroll.tallest() {
                scroll = found
                await found.walk(speed: Self.scrollSpeed, depth: Self.scrollDepth)
                scroll = nil
            }
        }
        if case .scroll = step {} else { try? await Task.sleep(for: .seconds(Self.window)) }
        return (CACurrentMediaTime() - start, clock_gettime_nsec_np(CLOCK_THREAD_CPUTIME_ID) - cpu)
    }

    private func log(round: Int, step: Step, seconds: Double, cpu: UInt64, visible: Bool) {
        let ms = intervals.map { $0 * 1000 }
        let sorted = ms.sorted()
        let frameMs = frame * 1000
        let late = ms.filter { $0 > frameMs * 1.5 }
        let hitch = late.reduce(0) { $0 + $1 - frameMs }
        let at = { (q: Double) in sorted.isEmpty ? 0 : sorted[min(sorted.count - 1, Int(Double(sorted.count) * q))] }
        let line = String(
            format: "round=%d step=%@ frames=%d frame_ms=%.2f first_ms=%.1f p50_ms=%.2f p95_ms=%.2f max_ms=%.1f late=%d stalls_50=%d hitch_ms_per_s=%.1f main_cpu_ms_per_s=%.0f visible=%d",
            round, step.name, ms.count, frameMs, ms.first ?? 0, at(0.5), at(0.95), sorted.last ?? 0, late.count,
            ms.filter { $0 > 50 }.count, hitch / seconds, Double(cpu) / 1e6 / seconds, visible ? 1 : 0)
        Self.report("metric nav \(line)")
    }

    private static func report(_ line: String) {
        coreLog.notice("\(line, privacy: .public)")
        #if os(iOS)
        DebugHooks.note(line)
        #endif
    }

    @objc private func tick(_ link: CADisplayLink) {
        let now = link.timestamp
        if let last { intervals.append(now - last) }
        last = now
        frame = link.targetTimestamp - link.timestamp
        scroll?.step(now)
    }

    /// The page's tallest scroll view on screen, moved one step per refresh as a finger or trackpad does.
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

        static func tallest() -> Scroll? {
            guard let window = NSApp.windows.max(by: { $0.frame.height < $1.frame.height }) else { return nil }
            func all(_ view: NSView) -> [NSScrollView] {
                if view.isHidden || view.alphaValue < 0.01 { return [] }
                if let scroll = view as? NSScrollView { return [scroll] }
                return view.subviews.flatMap(all)
            }
            let found = all(window.contentView ?? NSView()).max { ($0.documentView?.frame.height ?? 0) < ($1.documentView?.frame.height ?? 0) }
            return found.map(Scroll.init)
        }

        private var room: Double { (view.documentView?.frame.height ?? 0) - view.contentView.bounds.height }

        private func move(to y: Double) {
            let flipped = view.documentView?.isFlipped ?? true
            view.contentView.scroll(to: NSPoint(x: view.contentView.bounds.minX, y: flipped ? y : room - y))
            view.reflectScrolledClipView(view.contentView)
        }
        #else
        init(_ view: UIScrollView) { self.view = view }

        static func tallest() -> Scroll? {
            let windows = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.flatMap(\.windows)
            guard let window = windows.first(where: \.isKeyWindow) ?? windows.first else { return nil }
            func shown(_ view: UIView) -> Bool {
                var at: UIView? = view
                while let v = at {
                    if v.isHidden || v.alpha < 0.01 || v.layer.opacity < 0.01 { return false }
                    at = v.superview
                }
                return true
            }
            func all(_ view: UIView) -> [UIScrollView] {
                (view as? UIScrollView).map { [$0] } ?? view.subviews.flatMap(all)
            }
            let found = all(window).filter { shown($0) && $0.contentSize.height > $0.bounds.height }.max { $0.contentSize.height < $1.contentSize.height }
            return found.map(Scroll.init)
        }

        private var room: Double { view.contentSize.height + view.adjustedContentInset.bottom - view.bounds.height }

        private func move(to y: Double) {
            view.contentOffset = CGPoint(x: view.contentOffset.x, y: y - view.adjustedContentInset.top)
        }
        #endif

        func walk(speed: Double, depth: Double) async {
            self.speed = speed
            self.depth = min(depth, room)
            guard self.depth > 0 else { return }
            await withCheckedContinuation { done = $0 }
        }

        func step(_ now: CFTimeInterval) {
            guard done != nil else { return }
            let start = start ?? now
            self.start = start
            let travelled = (now - start) * speed
            guard travelled < 2 * depth else {
                move(to: 0)
                done?.resume()
                done = nil
                return
            }
            move(to: travelled < depth ? travelled : 2 * depth - travelled)
        }
    }
}
#endif
