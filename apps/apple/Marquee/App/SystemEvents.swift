import Network
import SwiftUI

/// Forwards memory pressure and network changes to the core.
@MainActor
final class SystemEvents {
    private let memory = DispatchSource.makeMemoryPressureSource(eventMask: [.warning, .critical], queue: .main)
    private let network = NWPathMonitor()
    private var sawFirstPath = false

    init(core: CoreClient) {
        memory.setEventHandler {
            MainActor.assumeIsolated { core.lifecycle("memoryPressure") }
        }
        memory.resume()
        network.pathUpdateHandler = { [weak self] path in
            MainActor.assumeIsolated {
                guard let self else { return }
                let kind = StreamChoice.network(
                    wifi: path.usesInterfaceType(.wifi), ethernet: path.usesInterfaceType(.wiredEthernet),
                    cellular: path.usesInterfaceType(.cellular))
                DeviceCaps.network = kind
                // The first update is the current path, not a change.
                if self.sawFirstPath { core.lifecycle("networkChanged", ["network": kind]) }
                self.sawFirstPath = true
            }
        }
        network.start(queue: .main)
    }
}

/// Forwards foreground and background to the core.
struct ScenePhaseForward: ViewModifier {
    let core: CoreClient
    let sync: SyncModel
    @Environment(\.scenePhase) private var phase

    func body(content: Content) -> some View {
        content.onChange(of: phase, initial: true) { _, phase in
            switch phase {
            case .active:
                core.lifecycle("foreground")
                // Pull what other devices changed while this one was away.
                if sync.joined { sync.request() }
            case .background:
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
