import SwiftUI

@main
struct MarqueeApp: App {
    @State private var core: CoreClient
    @State private var home: HomeModel
    @State private var system: SystemEvents

    init() {
        Fonts.register()
        URLCache.shared = URLCache(memoryCapacity: 64 << 20, diskCapacity: 512 << 20)
        let core = CoreClient()
        _core = State(initialValue: core)
        _home = State(initialValue: HomeModel(core: core))
        _system = State(initialValue: SystemEvents(core: core))
        #if DEBUG && os(macOS)
        DebugHooks.listenForSnapshots()
        #endif
        coreLog.notice("metric core_open_s=\(Launch.elapsed(), format: .fixed(precision: 3), privacy: .public)")
    }

    var body: some Scene {
        WindowGroup {
            RootView(home: home)
                .environment(core)
                .modifier(ScenePhaseForward(core: core))
        }
        #if os(macOS)
        .defaultSize(width: 1440, height: 900)
        .windowToolbarStyle(.unified(showsTitle: false))
        #endif
    }
}
