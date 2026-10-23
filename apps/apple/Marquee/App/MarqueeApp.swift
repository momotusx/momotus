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
    @State private var trakt: TraktModel
    @State private var wipe: AccountWipe

    init() {
        Fonts.register()
        CoreBuild.check()
        #if os(macOS)
        // A kit copied before the app last stopped, if nothing was copied since.
        Clipboard.clearLeftover()
        #endif
        URLCache.shared = URLCache(memoryCapacity: 64 << 20, diskCapacity: 512 << 20)
        // A sync key left by a deleted install goes before the core opens an empty library.
        let library = CoreClient.dataDirectory().appending(path: "library.sqlite").path(percentEncoded: false)
        let leftover = SyncRecord.keychain.forgetLeftover(libraryExists: FileManager.default.fileExists(atPath: library))
        let core = CoreClient()
        _core = State(initialValue: core)
        _system = State(initialValue: SystemEvents(core: core))
        let sync = SyncModel(core: core, leftover: leftover)
        _sync = State(initialValue: sync)
        #if DEBUG
        if DebugHooks.kitPreview { sync.kits.hold(DebugHooks.previewKit) }
        #endif
        let trakt = TraktModel(core: core)
        _trakt = State(initialValue: trakt)
        let profiles = ProfileSession(core: core, sync: sync, trakt: trakt)
        _profiles = State(initialValue: profiles)
        let debrid = DebridSession(core: core)
        _debrid = State(initialValue: debrid)
        _wipe = State(initialValue: AccountWipe(
            core: core, sync: sync, wipePending: { profiles.snapshot?.wipePending }, traktGone: { trakt.profileGone($0) },
            debridAccounts: { debrid.view.snapshot?.accounts ?? [] }, disconnect: { try await debrid.disconnect($0) }))
        #if DEBUG && os(macOS)
        DebugHooks.listenForSnapshots()
        #endif
        #if os(macOS)
        // One window on one core: no tab bar, whose "+" would open a second copy.
        NSWindow.allowsAutomaticWindowTabbing = false
        #endif
        #if DEBUG
        // Said at launch, before anything can play: a walk checks it first.
        coreLog.notice("debug: players start \(DebugHooks.mute ? "muted" : "with sound", privacy: .public)")
        #endif
        #if MARQUEE_WALK
        WalkSampler.start()
        #endif
    }

    /// D13's removal resumes on the owner, unlocked, when the core says one is left.
    private var wipeKey: String {
        guard profiles.open, profiles.snapshot?.current?.isOwner == true, profiles.snapshot?.wipePending != nil else { return "" }
        return "\(core.generation)/\(profiles.snapshot?.active ?? "")"
    }

    /// Which profile's Trakt runs: none while the picker shows, a new one for every
    /// core and after the wipe (the same owner id, its view closed by the core).
    private var traktKey: String {
        guard profiles.open, let active = profiles.snapshot?.active else { return "" }
        return "\(core.generation)/\(active)/\(profiles.epoch)"
    }

    var body: some Scene {
        WindowGroup {
            RootView()
                #if os(macOS)
                // The narrowest window at which every secondary over art still reads (the glass check).
                .frame(minWidth: BackdropStops.macMinWidth)
                #endif
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
                .environment(trakt)
                .environment(wipe)
                .task(id: wipeKey) {
                    guard !wipeKey.isEmpty else { return }
                    await wipe.resume(profiles: profiles.snapshot?.profiles.map(\.id) ?? [])
                }
                #if os(iOS)
                // Sign in another device on the home screen while this phone is in an account.
                .onChange(of: sync.joined, initial: true) { _, joined in AppRouter.updateQuickAction(joined: joined) }
                #endif
                // The running profile's Trakt: its record, then its view, each time a profile
                // opens (a start, an unlock, a switch, a new core); closed while none is open.
                .task(id: traktKey) {
                    if let profile = profiles.open ? profiles.snapshot?.active : nil {
                        await trakt.begin(profile: profile)
                    } else {
                        trakt.end()
                    }
                }
        }
        #if os(macOS)
        .defaultSize(width: 1440, height: 900)
        .windowResizability(.contentMinSize)
        .windowToolbarStyle(.unified(showsTitle: false))
        .commands { AppCommands() }
        #endif
    }
}
