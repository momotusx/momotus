#if os(macOS)
import Sparkle
import SwiftUI

/// The Mac's self-update (plans/auto-update.md section 3): Sparkle reads the appcast on the `updates` release,
/// downloads in the background and installs when Momotus quits. Nothing pops up on its own: a waiting update
/// shows in About, with Restart to install it now. Only a build with a feed (MarqueeUpdateFeed, set by
/// release-mac.sh) checks; local builds, deliveries and walks have none.
@MainActor @Observable
// Sparkle calls both delegates on the main thread.
final class MacUpdater: NSObject, @preconcurrency SPUUpdaterDelegate, @preconcurrency SPUStandardUserDriverDelegate {
    static let shared = MacUpdater()

    /// The version that installs when Momotus quits, once Sparkle has it ready.
    private(set) var ready: String?
    private var installNow: (() -> Void)?
    @ObservationIgnored private var controller: SPUStandardUpdaterController?

    /// The feed this build checks, or nil when it has none.
    nonisolated static var feed: URL? {
        (Bundle.main.object(forInfoDictionaryKey: "MarqueeUpdateFeed") as? String).flatMap { $0.isEmpty ? nil : URL(string: $0) }
    }

    var enabled: Bool { controller != nil }

    /// Starts the updater once, at launch, when the build has a feed.
    func start() {
        guard controller == nil, Self.feed != nil else { return }
        controller = SPUStandardUpdaterController(startingUpdater: true, updaterDelegate: self, userDriverDelegate: self)
    }

    /// The menu's and About's check: Sparkle's own window says what it found.
    func check() { controller?.checkForUpdates(nil) }

    /// Restart: installs the waiting update and opens Momotus again.
    func restart() { installNow?() }

    // MARK: SPUUpdaterDelegate

    func feedURLString(for updater: SPUUpdater) -> String? { Self.feed?.absoluteString }

    func updater(_ updater: SPUUpdater, willInstallUpdateOnQuit item: SUAppcastItem,
                 immediateInstallationBlock: @escaping () -> Void) -> Bool {
        ready = item.displayVersionString
        installNow = immediateInstallationBlock
        return true
    }

    // MARK: SPUStandardUserDriverDelegate

    var supportsGentleScheduledUpdateReminders: Bool { true }

    /// A scheduled find never shows Sparkle's window (it could land over a film): About shows it instead.
    func standardUserDriverShouldHandleShowingScheduledUpdate(_ update: SUAppcastItem,
                                                              andInImmediateFocus immediateFocus: Bool) -> Bool {
        false
    }
}
#endif
