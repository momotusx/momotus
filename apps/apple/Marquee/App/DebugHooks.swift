#if DEBUG
import SwiftUI

/// Test hooks for debug builds only, used to verify the app headless (a
/// locked Mac, a simulator driven from a script).
enum DebugHooks {
    /// `-MQOpenTitle series:tt0903747` opens that title page at launch.
    static var openTitle: TitleRoute? {
        guard let value = UserDefaults.standard.string(forKey: "MQOpenTitle") else { return nil }
        let parts = value.split(separator: ":", maxSplits: 1).map(String.init)
        guard parts.count == 2 else { return nil }
        return TitleRoute(kind: parts[0], id: parts[1], name: "")
    }

    /// `-MQTab discover` starts on that tab.
    static var tab: AppTab? { UserDefaults.standard.string(forKey: "MQTab").flatMap(AppTab.init(rawValue:)) }

    /// `-MQAutoPlay YES` presses Play on the title page once a version is picked.
    static var autoPlay: Bool { UserDefaults.standard.bool(forKey: "MQAutoPlay") }

    /// `-MQSeekTo 575` starts the first playback of the process there instead of its resume point, once.
    @MainActor static func takeSeek() -> Double? {
        guard !seekTaken, UserDefaults.standard.object(forKey: "MQSeekTo") != nil else { return nil }
        seekTaken = true
        return UserDefaults.standard.double(forKey: "MQSeekTo")
    }

    @MainActor private static var seekTaken = false

    /// `-MQShowVersions YES` opens the Versions page once versions are in.
    static var showVersions: Bool { UserDefaults.standard.bool(forKey: "MQShowVersions") }

    /// `-MQInstallAddons url1,url2` installs addons through the protocol at launch.
    static var installAddons: [String] {
        UserDefaults.standard.string(forKey: "MQInstallAddons")?.split(separator: ",").map(String.init) ?? []
    }

    #if os(macOS)
    /// `notifyutil -p app.marquee.snapshot` writes each window to
    /// $TMPDIR/marquee-window-<n>.png, drawn by AppKit (no screen recording).
    static func listenForSnapshots() {
        let center = CFNotificationCenterGetDarwinNotifyCenter()
        CFNotificationCenterAddObserver(center, nil, { _, _, _, _, _ in
            DispatchQueue.main.async {
                MainActor.assumeIsolated { DebugHooks.snapshotWindows() }
            }
        }, "app.marquee.snapshot" as CFString, nil, .deliverImmediately)
    }

    @MainActor private static func snapshotWindows() {
        for (index, window) in NSApp.windows.enumerated() where window.isVisible {
            guard let view = window.contentView?.superview ?? window.contentView,
                  let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { continue }
            view.cacheDisplay(in: view.bounds, to: rep)
            let url = URL.temporaryDirectory.appending(path: "marquee-window-\(index).png")
            try? rep.representation(using: .png, properties: [:])?.write(to: url)
            coreLog.notice("snapshot \(url.path(percentEncoded: false), privacy: .public)")
        }
    }
    #endif
}
#endif
