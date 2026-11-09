#if DEBUG
import SwiftUI

/// Test hooks for debug builds only, used to verify the app headless (a
/// locked Mac, a simulator driven from a script).
enum DebugHooks {
    /// `-MQOpenTitle series:tt0903747` opens that title page at launch; `-MQOpenSeason 2` and
    /// `-MQOpenEpisode tt0903747:2:3` open it as a Calendar link does, on that season and episode.
    static var openTitle: TitleRoute? {
        guard let value = UserDefaults.standard.string(forKey: "MQOpenTitle") else { return nil }
        let parts = value.split(separator: ":", maxSplits: 1).map(String.init)
        guard parts.count == 2 else { return nil }
        let season = UserDefaults.standard.object(forKey: "MQOpenSeason") == nil ? nil : UserDefaults.standard.integer(forKey: "MQOpenSeason")
        return TitleRoute(kind: parts[0], id: parts[1], name: "", season: season, videoId: UserDefaults.standard.string(forKey: "MQOpenEpisode"))
    }

    /// `-MQScrollBottom YES`: the title page opens scrolled to its foot, for a walk's shot.
    static var scrollBottom: Bool { UserDefaults.standard.bool(forKey: "MQScrollBottom") }

    /// `-MQHomeScrollY 640` scrolls Home that far once its rows have come: a walk's shot of the Mac's
    /// scrolled state with no pointer or key sent to the window.
    static var homeScrollY: Double { UserDefaults.standard.double(forKey: "MQHomeScrollY") }

    /// `-MQSettingsSection playback`: Settings jumps to that section once its page has laid out, as its menu entry
    /// does, for a walk's shot of the scrolled page with no pointer or key sent to the window.
    static var settingsSection: SettingsSection? {
        UserDefaults.standard.string(forKey: "MQSettingsSection").flatMap(SettingsSection.init(rawValue:))
    }

    /// `-MQShowSoundSwitch YES`: a muted walk still draws the trailer's sound switch, for its shot; it stays silent.
    static var showSoundSwitch: Bool { UserDefaults.standard.bool(forKey: "MQShowSoundSwitch") }

    /// `-MQTab discover` starts on that tab.
    static var tab: AppTab? { UserDefaults.standard.string(forKey: "MQTab").flatMap(AppTab.init(rawValue:)) }

    /// `-MQOpenDownloads YES` opens Library's Downloads page once Library shows.
    static var openDownloads: Bool { UserDefaults.standard.bool(forKey: "MQOpenDownloads") }

    /// `-MQOffline YES`: the app takes every network path as none (the shell's banner, the core held offline), so
    /// a walk shows the offline pages without the phone's airplane mode.
    static var offline: Bool { UserDefaults.standard.bool(forKey: "MQOffline") }

    /// One walk line on stderr and in `Documents/walk.log`, which `devicectl device copy from` reads off a phone.
    static func note(_ line: String) {
        FileHandle.standardError.write(Data("walk \(line)\n".utf8))
        let url = URL.documentsDirectory.appending(path: "walk.log")
        let data = Data("\(Date().ISO8601Format()) \(line)\n".utf8)
        if let handle = try? FileHandle(forWritingTo: url) {
            handle.seekToEndOfFile()
            handle.write(data)
            try? handle.close()
        } else {
            try? data.write(to: url)
        }
    }

    /// `-MQReduceMotion YES`: first run's poster wall and step changes hold still as under Reduce Motion, for a walk
    /// that may not change the device's own setting (plans/first-screens.md 10).
    static var reduceMotion: Bool { UserDefaults.standard.bool(forKey: "MQReduceMotion") }

    /// `-MQMute YES`: every player starts muted, so a walk never makes a sound.
    static var mute: Bool { UserDefaults.standard.bool(forKey: "MQMute") }

    /// `-MQPlayUpright YES`: the phone player opens upright, as after a turn.
    /// A walk on a phone lying flat: a turn set from a UI test goes back to
    /// face up at once and the screen stays across.
    static var playUpright: Bool { UserDefaults.standard.bool(forKey: "MQPlayUpright") }

    /// `-MQSearch Sintel` types that query into Search when it first shows: a
    /// walk whose window is out of reach of accessibility (another Space).
    static var search: String? { UserDefaults.standard.string(forKey: "MQSearch") }

    /// `-MQRetryRowsAfter 20`: that many seconds after Home opens, each failed
    /// row takes its own Try again once, as a press would.
    static var retryRowsAfter: Int { UserDefaults.standard.integer(forKey: "MQRetryRowsAfter") }

    /// `-MQAutoPlay YES` presses Play on the title page once a version is picked.
    static var autoPlay: Bool { UserDefaults.standard.bool(forKey: "MQAutoPlay") }

    /// `-MQPickRow 2` plays the Versions page's second row once, as a hand
    /// pick (a row's own Play, not the recommendation), once the list is done:
    /// a TV walk has no remote to press it.
    static var pickRow: Int { UserDefaults.standard.integer(forKey: "MQPickRow") }
    @MainActor static var pickRowTaken = false

    /// `-MQSeekTo 575` starts the first playback of the process there instead of its resume point, once.
    @MainActor static func takeSeek() -> Double? {
        guard !seekTaken, UserDefaults.standard.object(forKey: "MQSeekTo") != nil else { return nil }
        seekTaken = true
        return UserDefaults.standard.double(forKey: "MQSeekTo")
    }

    @MainActor private static var seekTaken = false

    /// `-MQText 0` turns that one of the file's own subtitles on once they are read.
    static var textTrack: Int? {
        UserDefaults.standard.object(forKey: "MQText") == nil ? nil : UserDefaults.standard.integer(forKey: "MQText")
    }

    /// `-MQOnboarding YES` opens the first-run flow over Home, once.
    @MainActor static func takeOnboarding() -> Bool {
        guard !onboardingTaken, UserDefaults.standard.bool(forKey: "MQOnboarding") else { return false }
        onboardingTaken = true
        return true
    }

    @MainActor private static var onboardingTaken = false

    /// `-MQPairedPreview YES` opens the pairing page once from Settings, in its
    /// joined state over this device's real library and addons. Nothing is
    /// stored and no sync starts: a live join needs a second device and a relay.
    static var pairedPreview: Bool { UserDefaults.standard.bool(forKey: "MQPairedPreview") }

    /// The Settings page to open once at launch: `-MQOpenScreen findAddons` or `sync`
    /// (SwiftUI menus do not open under idb's synthetic taps), or the pairing
    /// preview. It goes on the Settings stack's path, as `-MQOpenTitle` does on
    /// Home's: an `isPresented` destination turned on from Settings' first
    /// task kept SwiftUI updating at full CPU for 20 to 60 s.
    @MainActor static func takeSettingsPage() -> SettingsPage? {
        guard !settingsPageTaken else { return nil }
        settingsPageTaken = true
        switch UserDefaults.standard.string(forKey: "MQOpenScreen") {
        case "findAddons": return .findAddons
        case "sync": return .sync
        case "kit": return .kit
        case "compare": return .compare
        case "trakt": return .trakt
        case "streamChoice": return .streamChoice
        default: break
        }
        return pairedPreview ? .pairing : nil
    }

    @MainActor private static var settingsPageTaken = false

    /// `-MQKitPreview YES`: a made-up kit, never an account's, held at launch
    /// and shown on Make a recovery kit (`-MQOpenScreen kit`) without the OS
    /// prompt, for a Mac walk where nobody can answer the prompt. A kit made
    /// for real still waits for the prompt.
    static var kitPreview: Bool { UserDefaults.standard.bool(forKey: "MQKitPreview") }
    static let previewKit = HeldKit(id: "", kit: "TEST-ONLY-MADE-UPKT-TEST-ONLY-MADE-UPKT")

    /// `-MQForkPreview YES`: the compare page (`-MQOpenScreen compare`) shows a
    /// made-up different list instead of asking the core: two kits of the same
    /// name, a device only on the server's list, and one only this device knew.
    static var forkPreview: ForkPreview? {
        guard UserDefaults.standard.bool(forKey: "MQForkPreview") else { return nil }
        let json = #"""
        {"mine": [
          {"id": "m", "kit": false, "name": "Walk Mac", "sponsor": null, "addedMs": 1760850000000, "thisDevice": true},
          {"id": "p", "kit": false, "name": "Phone", "sponsor": "m", "addedMs": 1760853600000, "thisDevice": false},
          {"id": "k0", "kit": true, "name": null, "sponsor": "m", "addedMs": 1760857200000, "thisDevice": false}],
         "server": [
          {"id": "m", "kit": false, "name": "Walk Mac", "sponsor": null, "addedMs": 1760850000000, "thisDevice": true},
          {"id": "p", "kit": false, "name": "Phone", "sponsor": "m", "addedMs": 1760853600000, "thisDevice": false},
          {"id": "t", "kit": false, "name": "Tablet", "sponsor": "p", "addedMs": 1760860800000, "thisDevice": false},
          {"id": "k1", "kit": true, "name": null, "sponsor": "p", "addedMs": 1760864400000, "thisDevice": false},
          {"id": "k2", "kit": true, "name": null, "sponsor": "t", "addedMs": 1760868000000, "thisDevice": false}],
         "drops": [], "kits": [{"device": "p", "kits": ["k1"]}, {"device": "t", "kits": ["k2"]}],
         "onServer": true, "head": "preview"}
        """#
        return try? JSONDecoder().decode(ForkPreview.self, from: Data(json.utf8))
    }

    /// `-MQKillCoreAfter 12`: that many seconds after launch the app gets the
    /// message a panicking core sends, to walk a core death and its restart.
    static var killCoreAfter: Int { UserDefaults.standard.integer(forKey: "MQKillCoreAfter") }

    /// `-MQOpenPicker subtitles` (or `audio`) opens that picker once the player has its tracks; with
    /// `-MQPickerLanguage en` the subtitle picker opens on that language's options. For a walk's shots.
    static var openPicker: String? { UserDefaults.standard.string(forKey: "MQOpenPicker") }
    static var pickerLanguage: String? { UserDefaults.standard.string(forKey: "MQPickerLanguage") }
    /// `-MQSubtitleLanguages en,tr` sets the profile's subtitle languages at launch, so the core turns one on by itself.
    static var subtitleLanguages: [String] {
        UserDefaults.standard.string(forKey: "MQSubtitleLanguages")?.split(separator: ",").map(String.init) ?? []
    }

    /// `-MQShowVersions YES` opens the Versions page once versions are in.
    static var showVersions: Bool { UserDefaults.standard.bool(forKey: "MQShowVersions") }

    /// `-MQVersionsFixture /path/versions.json`: every Versions view shows that snapshot, for a shot of
    /// a state no live title gives on demand (needs an account, no source in reach).
    static let versionsFixture: VersionsSnapshot? = fixture("MQVersionsFixture")

    /// `-MQPlaybackFixture /path/playback.json`: the player shows that snapshot instead of opening the
    /// core's view (its failure card).
    static let playbackFixture: PlaybackSnapshot? = fixture("MQPlaybackFixture")

    private static func fixture<T: Decodable>(_ key: String) -> T? {
        guard let path = UserDefaults.standard.string(forKey: key),
              let data = FileManager.default.contents(atPath: path) else { return nil }
        return try? JSONDecoder().decode(T.self, from: data)
    }

    /// `-MQPrefsPatch '{"defaultsSeen":1000}'` patches the profile's whole-number prefs at launch, as Android's
    /// `prefs_patch` extra does: a fresh walk install that should open on Home, not the first-run flow.
    static var prefsPatch: [String: Int]? {
        guard let text = UserDefaults.standard.string(forKey: "MQPrefsPatch"), let data = text.data(using: .utf8) else { return nil }
        return try? JSONDecoder().decode([String: Int].self, from: data)
    }

    /// `-MQInstallAddons url1,url2` installs addons through the protocol at launch.
    static var installAddons: [String] {
        UserDefaults.standard.string(forKey: "MQInstallAddons")?.split(separator: ",").map(String.init) ?? []
    }

    #if os(macOS)
    /// `-MQScrollWalk YES`: 12 s after Home shows, Home scrolls itself to its foot and back three times at a trackpad
    /// flick's speed, one step per screen refresh through AppKit, and logs each pass's frame times (`metric scroll`): the Mac's
    /// scroll measured with no pointer or key sent to the window. Each line also says how many AppKit views (and shelf
    /// scroll views) the page holds after the pass, whether the Mac's pointer was inside the window's frame (hover may
    /// then update on every step) and whether the window showed (one out of sight is no measurement).
    static var scrollWalk: Bool { UserDefaults.standard.bool(forKey: "MQScrollWalk") }

    /// `notifyutil -p app.momotus.snapshot` writes each window to
    /// $TMPDIR/marquee-window-<n>.png, drawn by AppKit (no screen recording).
    static func listenForSnapshots() {
        let center = CFNotificationCenterGetDarwinNotifyCenter()
        CFNotificationCenterAddObserver(center, nil, { _, _, _, _, _ in
            DispatchQueue.main.async {
                MainActor.assumeIsolated { DebugHooks.snapshotWindows() }
            }
        }, "app.momotus.snapshot" as CFString, nil, .deliverImmediately)
    }

    @MainActor private static func snapshotWindows() {
        for (index, window) in NSApp.windows.enumerated() where window.isVisible {
            guard let view = window.contentView?.superview ?? window.contentView,
                  let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { continue }
            view.cacheDisplay(in: view.bounds, to: rep)
            let url = URL.temporaryDirectory.appending(path: "marquee-window-\(index).png")
            try? rep.representation(using: .png, properties: [:])?.write(to: url)
            // `sharingType` none: screen capture and recording leave the window out (a kit on it).
            coreLog.notice("snapshot \(url.path(percentEncoded: false), privacy: .public) sharing=\(window.sharingType.rawValue, privacy: .public)")
        }
    }
    #endif
}

#if os(macOS)
import QuartzCore

/// The scroll walk (`DebugHooks.scrollWalk`): moves the window's tallest scroll view one step per screen refresh,
/// through AppKit as a trackpad does (no SwiftUI state is written), and measures each refresh's interval. A frame
/// the main thread could not make in time shows as an interval longer than the screen's own.
@MainActor final class ScrollWalk: NSObject {
    /// A brisk trackpad flick.
    private static let speed = 2400.0
    private static let passes = 3
    private let scroll: NSScrollView
    private let room: Double
    /// The window showed when the walk began (`showOnSomeScreen`).
    private let visible: Bool
    private var finished: CheckedContinuation<Void, Never>?
    private var start: CFTimeInterval?
    private var last: CFTimeInterval?
    private var intervals: [Double] = []
    private var frame = 0.0
    /// The main thread's CPU time at the pass's start: what the pass cost, whatever else the Mac is running.
    private var cpu: UInt64 = 0
    private var pass = 0

    private init(scroll: NSScrollView, visible: Bool) {
        self.scroll = scroll
        self.visible = visible
        room = (scroll.documentView?.frame.height ?? 0) - scroll.contentView.bounds.height
    }

    /// Scrolls the main window's tallest scroll view to its foot and back, `passes` times, logging one line a pass.
    static func run() async {
        guard let window = NSApp.windows.max(by: { $0.frame.height < $1.frame.height }) else { return }
        let visible = await showOnSomeScreen(window)
        guard let scroll = scrollViews(in: window.contentView).max(by: { ($0.documentView?.frame.height ?? 0) < ($1.documentView?.frame.height ?? 0) }),
              let screen = window.screen ?? NSScreen.main else { return }
        let walk = ScrollWalk(scroll: scroll, visible: visible)
        guard walk.room > 0 else { return }
        await withCheckedContinuation { done in
            walk.finished = done
            screen.displayLink(target: walk, selector: #selector(tick)).add(to: .main, forMode: .common)
        }
    }

    /// A window in a Space out of sight still draws, but its views pile up and each pass costs more than the last: the
    /// walk tries the other displays in turn (moved only, never in front, no focus) and says whether the window shows.
    private static func showOnSomeScreen(_ window: NSWindow) async -> Bool {
        for screen in NSScreen.screens where screen != window.screen {
            if window.occlusionState.contains(.visible) { break }
            window.setFrameOrigin(NSPoint(x: screen.visibleFrame.midX - window.frame.width / 2, y: screen.visibleFrame.midY - window.frame.height / 2))
            guard (try? await Task.sleep(for: .seconds(2))) != nil else { break }
        }
        return window.occlusionState.contains(.visible)
    }

    private static func scrollViews(in view: NSView?) -> [NSScrollView] {
        guard let view else { return [] }
        // The outermost only: a shelf inside the page is a scroll view of its own.
        if let scroll = view as? NSScrollView { return [scroll] }
        return view.subviews.flatMap { scrollViews(in: $0) }
    }

    private static func count(_ view: NSView?) -> (all: Int, scrolls: Int) {
        guard let view else { return (0, 0) }
        return view.subviews.map { count($0) }.reduce((1, view is NSScrollView ? 1 : 0)) { ($0.all + $1.all, $0.scrolls + $1.scrolls) }
    }

    private func move(to y: Double) {
        let flipped = scroll.documentView?.isFlipped ?? true
        scroll.contentView.scroll(to: NSPoint(x: scroll.contentView.bounds.minX, y: flipped ? y : room - y))
        scroll.reflectScrolledClipView(scroll.contentView)
    }

    @objc private func tick(_ link: CADisplayLink) {
        let now = link.timestamp
        if let last { intervals.append(now - last) }
        last = now
        frame = link.targetTimestamp - link.timestamp
        if start == nil { cpu = clock_gettime_nsec_np(CLOCK_THREAD_CPUTIME_ID) }
        let start = start ?? now
        self.start = start
        let travelled = (now - start) * Self.speed
        guard travelled < 2 * room else { return end(pass: now - start, link: link) }
        move(to: travelled < room ? travelled : 2 * room - travelled)
    }

    private func end(pass seconds: Double, link: CADisplayLink) {
        let spent = clock_gettime_nsec_np(CLOCK_THREAD_CPUTIME_ID) - cpu
        // What the page holds after the pass: AppKit views under it, and how many of them scroll (each shelf is one).
        let views = Self.count(scroll.documentView)
        let ms = intervals.map { $0 * 1000 }.sorted()
        let frameMs = frame * 1000
        // Apple's hitch time: what each late frame overran its slot by, per second of scrolling.
        let late = ms.filter { $0 > frameMs * 1.5 }
        let hitch = late.reduce(0) { $0 + $1 - frameMs }
        let at = { (q: Double) in ms.isEmpty ? 0 : ms[min(ms.count - 1, Int(Double(ms.count) * q))] }
        let line = String(
            format: "pass=%d frames=%d frame_ms=%.2f p50_ms=%.2f p95_ms=%.2f p99_ms=%.2f max_ms=%.2f late=%d hitch_ms_per_s=%.1f main_cpu_ms_per_s=%.0f room=%.0f view=%@ views=%d scroll_views=%d pointer_in=%d visible=%d",
            pass + 1, ms.count, frameMs, at(0.5), at(0.95), at(0.99), ms.last ?? 0, late.count, hitch / seconds,
            Double(spent) / 1e6 / seconds, room,
            String(describing: type(of: scroll)), views.all, views.scrolls, scroll.window?.frame.contains(NSEvent.mouseLocation) == true ? 1 : 0, visible ? 1 : 0)
        coreLog.notice("metric scroll \(line, privacy: .public)")
        pass += 1
        (start, last, intervals) = (nil, nil, [])
        move(to: 0)
        if pass == Self.passes {
            link.invalidate()
            finished?.resume()
            finished = nil
        }
    }
}
#endif
#endif
