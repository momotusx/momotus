import Network
import SwiftUI

/// Forwards memory pressure and network changes to the core, and says whether the device is offline (the
/// shell's banner, plans/offline-downloads.md 3.5).
@MainActor @Observable
final class SystemEvents {
    private(set) var offline = false
    @ObservationIgnored private let memory = DispatchSource.makeMemoryPressureSource(eventMask: [.warning, .critical], queue: .main)
    @ObservationIgnored private let network = NWPathMonitor()
    @ObservationIgnored private let core: CoreClient
    /// The last path as told to the core, for a core opened again (`resend`).
    @ObservationIgnored private var last: NetworkReport?

    init(core: CoreClient) {
        self.core = core
        memory.setEventHandler {
            MainActor.assumeIsolated { core.lifecycle("memoryPressure") }
        }
        memory.resume()
        network.pathUpdateHandler = { [weak self] path in
            MainActor.assumeIsolated {
                let report = NetworkReport.of(
                    satisfied: path.status == .satisfied, wifi: path.usesInterfaceType(.wifi),
                    ethernet: path.usesInterfaceType(.wiredEthernet), cellular: path.usesInterfaceType(.cellular),
                    expensive: path.isExpensive, constrained: path.isConstrained, desktop: Self.desktop)
                // With no network the last real one stays the one speeds are read for.
                if report.network != "none" { DeviceCaps.network = report.network }
                // The first update too: a launch with no network, or on a metered one, holds downloads
                // from the start (plans/offline-downloads.md 4.13).
                self?.last = report
                self?.offline = report.network == "none"
                core.lifecycle("networkChanged", report.fields)
            }
        }
        network.start(queue: .main)
    }

    /// A core opened again (a restart after an overflow or a death) knows no network, and runs no download
    /// before it hears one: it is told the current path, which `NWPathMonitor` only reports on a change.
    func resend() {
        if let last { core.lifecycle("networkChanged", last.fields) }
    }

    private static var desktop: Bool {
        #if os(macOS)
        true
        #else
        false
        #endif
    }
}

/// Forwards foreground and background to the core.
struct ScenePhaseForward: ViewModifier {
    /// The app's windows still in front (an iPad can show several); the Mac's are never held.
    @MainActor static var windowsInFront: Int {
        #if os(iOS)
        UIApplication.shared.connectedScenes.filter { $0.activationState == .foregroundActive || $0.activationState == .foregroundInactive }.count
        #else
        0
        #endif
    }

    let core: CoreClient
    let sync: SyncModel
    #if !os(tvOS)
    /// Tells the keeper, which holds or lets go of downloads (plans/offline-downloads.md 5).
    var downloads: DownloadKeeper?
    #endif
    @Environment(\.scenePhase) private var phase

    func body(content: Content) -> some View {
        content.onChange(of: phase, initial: true) { _, phase in
            switch phase {
            case .active:
                Task { await core.foreground() }
                #if !os(tvOS)
                downloads?.handle(.foreground)
                #endif
                // Pull what other devices changed while this one was away.
                if sync.joined { sync.request() }
            case .background:
                #if !os(tvOS)
                // Below iOS 26 the downloads wait (`app`) before the system suspends the app: when the last window
                // leaves, never while another iPad window is in front (review r1 m7).
                if KeeperLogic.appLeft(windowsInFront: Self.windowsInFront) { downloads?.handle(.background) }
                #endif
                #if os(iOS) || os(tvOS)
                // Trakt's scrobbles go before the app is suspended: the system waits for the
                // answer (4.5 s at most), and ends the wait itself if its own time runs out first.
                let work = BackgroundWork()
                work.begin("lifecycle")
                Task {
                    await core.lifecycleBackground()
                    work.end()
                }
                #else
                core.lifecycle("background")
                #endif
            default:
                break
            }
        }
    }
}

#if os(iOS) || os(tvOS)
/// A stretch of work the system lets finish after the app leaves the front.
@MainActor private final class BackgroundWork {
    private var id = UIBackgroundTaskIdentifier.invalid

    func begin(_ name: String) {
        id = UIApplication.shared.beginBackgroundTask(withName: name) { [weak self] in
            MainActor.assumeIsolated { self?.end() }
        }
    }

    func end() {
        guard id != .invalid else { return }
        UIApplication.shared.endBackgroundTask(id)
        id = .invalid
    }
}
#endif
