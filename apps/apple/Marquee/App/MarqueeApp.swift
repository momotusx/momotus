import SwiftUI

@main
struct MarqueeApp: App {
    #if os(iOS)
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    #endif
    @State private var core: CoreClient
    @State private var profiles: ProfileSession
    @State private var system: SystemEvents
    @State private var sync: SyncModel
    @State private var debrid: DebridSession

    init() {
        Fonts.register()
        #if os(macOS)
        // A kit copied before the app last stopped, if nothing was copied since.
        Clipboard.clearLeftover()
        #endif
        URLCache.shared = URLCache(memoryCapacity: 64 << 20, diskCapacity: 512 << 20)
        // A sync key left by a deleted install goes before the core opens an empty library.
        let library = CoreClient.dataDirectory().appending(path: "library.sqlite").path(percentEncoded: false)
        SyncRecord.keychain.forgetLeftover(libraryExists: FileManager.default.fileExists(atPath: library))
        let core = CoreClient()
        _core = State(initialValue: core)
        _system = State(initialValue: SystemEvents(core: core))
        let sync = SyncModel(core: core)
        _sync = State(initialValue: sync)
        #if DEBUG
        if DebugHooks.kitPreview { sync.kits.hold(DebugHooks.previewKit) }
        #endif
        _profiles = State(initialValue: ProfileSession(core: core, sync: sync))
        _debrid = State(initialValue: DebridSession(core: core))
        #if DEBUG && os(macOS)
        DebugHooks.listenForSnapshots()
        #endif
    }

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(core)
                .environment(sync)
                .environment(profiles)
                .modifier(ScenePhaseForward(core: core, sync: sync))
                #if DEBUG
                .task {
                    guard DebugHooks.killCoreAfter > 0 else { return }
                    try? await Task.sleep(for: .seconds(DebugHooks.killCoreAfter))
                    core.debugDie()
                }
                #endif
                .environment(debrid)
                // The debrid accounts, on every core open before the app shows anything.
                .task(id: core.generation) { await debrid.opened() }
        }
        #if os(macOS)
        .defaultSize(width: 1440, height: 900)
        .windowToolbarStyle(.unified(showsTitle: false))
        #endif
    }
}
