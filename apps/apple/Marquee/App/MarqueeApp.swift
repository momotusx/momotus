import SwiftUI

@main
struct MarqueeApp: App {
    @State private var core: CoreClient
    @State private var home: HomeModel
    @State private var system: SystemEvents
    @State private var sync: SyncModel

    init() {
        Fonts.register()
        URLCache.shared = URLCache(memoryCapacity: 64 << 20, diskCapacity: 512 << 20)
        let core = CoreClient()
        _core = State(initialValue: core)
        _home = State(initialValue: HomeModel(core: core))
        _system = State(initialValue: SystemEvents(core: core))
        _sync = State(initialValue: SyncModel(core: core))
        #if DEBUG && os(macOS)
        DebugHooks.listenForSnapshots()
        #endif
    }

    var body: some Scene {
        WindowGroup {
            RootView(home: home)
                .environment(core)
                .environment(sync)
                .modifier(ScenePhaseForward(core: core, sync: sync))
                // Debrid keys from the Keychain, at start and after the core reopens.
                .task(id: core.generation) {
                    guard DebridKeys.services.contains(where: { DebridKeys.key($0.id) != nil }) else { return }
                    do {
                        let stores = try await DebridKeys.handOver(to: core)
                        coreLog.notice("debrid keys handed over, stores=\(stores, privacy: .public)")
                    } catch {
                        coreLog.error("debrid keys not taken: \(error.localizedDescription, privacy: .public)")
                    }
                }
        }
        #if os(macOS)
        .defaultSize(width: 1440, height: 900)
        .windowToolbarStyle(.unified(showsTitle: false))
        #endif
    }
}
