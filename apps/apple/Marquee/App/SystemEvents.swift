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
        network.pathUpdateHandler = { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                // The first update is the current path, not a change.
                if self.sawFirstPath { core.lifecycle("networkChanged") }
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
                // Trakt's scrobbles go before the app is suspended: the system waits for the answer.
                let task = UIApplication.shared.beginBackgroundTask(withName: "lifecycle", expirationHandler: nil)
                Task {
                    await core.lifecycleBackground()
                    UIApplication.shared.endBackgroundTask(task)
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
