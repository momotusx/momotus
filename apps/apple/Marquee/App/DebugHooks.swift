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
        default: break
        }
        return pairedPreview ? .pairing : nil
    }

    @MainActor private static var settingsPageTaken = false

    /// `-MQKillCoreAfter 12`: that many seconds after launch the app gets the
    /// message a panicking core sends, to walk a core death and its restart.
    static var killCoreAfter: Int { UserDefaults.standard.integer(forKey: "MQKillCoreAfter") }

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
