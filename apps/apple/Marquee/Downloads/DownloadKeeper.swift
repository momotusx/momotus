// iOS and macOS only: TV forms have no downloads (plans/offline-downloads.md 2, decision 14).
#if !os(tvOS)
import Foundation
import Observation
#if os(iOS)
import BackgroundTasks
#else
import AppKit
#endif

/// Which downloads an op names.
enum DownloadTarget: Sendable {
    case ids([String])
    /// Every download of a title.
    case title(String)
    case all

    var fields: [String: any Sendable] {
        switch self {
        case .ids(let ids): ["ids": ids]
        case .title(let itemId): ["itemId": itemId]
        case .all: ["all": true]
        }
    }
}

/// The app's one subscription to the `downloads` view (every screen reads its snapshot), the download
/// ops, and the platform glue of plans/offline-downloads.md 5: it feeds `KeeperLogic` and only performs
/// its effects (the continued task on iOS 26 and later, `downloadsAllowed`, the Mac's activity and
/// Dock tile).
@MainActor @Observable
final class DownloadKeeper {
    /// The keeper the Mac's Quit asks.
    static weak var current: DownloadKeeper?

    private(set) var view: LiveRecord<DownloadsSnapshot>?
    /// Why the last op did not go through, in the words Android and web use. Every screen with download
    /// controls shows it (`downloadProblem`); the next op or the user's Dismiss clears it, never a success of
    /// another op or a timer (review r3 N1).
    private(set) var failure: String?

    @ObservationIgnored private let core: CoreClient
    @ObservationIgnored private var logic: KeeperLogic
    @ObservationIgnored private var minute: Task<Void, Never>?
    /// The core generation the view was opened for: every window runs `begin()`, one view serves them all
    /// (review r3 N3).
    @ObservationIgnored private var openedFor: Int?
    #if os(iOS)
    /// The continued task once the system started it, and the identifier of the one submitted.
    @ObservationIgnored private var task: AnyObject?
    @ObservationIgnored private var submitted: String?
    #else
    @ObservationIgnored private var activity: NSObjectProtocol?
    #endif

    init(core: CoreClient) {
        self.core = core
        logic = KeeperLogic(platform: Self.platform)
        DownloadKeeper.current = self
    }

    var snapshot: DownloadsSnapshot? { view?.snapshot }

    static var platform: KeeperPlatform {
        #if os(macOS)
        return .mac
        #else
        if #available(iOS 26, *) { return .continued }
        return .foreground
        #endif
    }

    /// Opens the view once per core, for its life: the core keeps the same subscription across a profile
    /// switch, a lock and an unlock and pushes that session's snapshot. A locked session (a cold start on a
    /// PIN profile, the picker) gets the device's `active` and `settings` alone, so the Dock bar, the Mac's
    /// activity, the Quit question and the continued task stay right before anyone unlocks (review r1 M2, r2 C1).
    func begin() {
        guard openedFor != core.generation else { return }
        openedFor = core.generation
        logic.resend()
        let record = LiveRecord<DownloadsSnapshot>("downloads", core: core)
        record.onSnapshot = { [weak self] snapshot in
            guard let self else { return }
            handle(.facts(KeeperFacts.of(snapshot, previous: logic.current)))
        }
        record.start()
        view = record
    }

    func handle(_ event: KeeperEvent) {
        for effect in logic.on(event) { perform(effect) }
        let running = (logic.current?.left ?? 0) > 0
        if running, minute == nil {
            minute = Task { [weak self] in
                while !Task.isCancelled {
                    try? await Task.sleep(for: .seconds(60))
                    guard let self, !Task.isCancelled else { return }
                    self.handle(.minute)
                }
            }
        } else if !running {
            minute?.cancel()
            minute = nil
        }
    }

    #if os(macOS)
    /// Downloads keep App Nap off, never the Mac awake: an idle Mac on battery may still sleep (review r1 m3).
    static let activityOptions: ProcessInfo.ActivityOptions = .userInitiatedAllowingIdleSystemSleep
    #endif

    /// Quitting the Mac asks first while downloads run.
    var asksBeforeQuit: Bool { KeeperLogic.asksBeforeQuit(logic.current) }

    /// Quit: the core is told to hold the downloads until the next launch; `done` runs once it has answered,
    /// or after `limit` at most. The jobs stop and checkpoint after that answer, so a quit loses at most the
    /// last 8 MiB a job holds since its checkpoint (review r1 M4, r2).
    func holdForQuit(within limit: Duration = .seconds(2), done: @escaping @MainActor () -> Void) {
        let once = Once(done)
        core.sendNow("downloadsAllowed", ["allowed": false, "reason": "app"]) { _ in once() }
        Task {
            try? await Task.sleep(for: limit)
            once()
        }
    }

    /// The first of two answers runs; the other is dropped.
    @MainActor private final class Once {
        private var run: (@MainActor () -> Void)?

        init(_ run: @escaping @MainActor () -> Void) {
            self.run = run
        }

        func callAsFunction() {
            run?()
            run = nil
        }
    }

    // MARK: Ops (plans/core-protocol.md, Ops)

    /// Downloads a video, a season's aired episodes or every regular season's; `pick` is a row of an open
    /// download-purpose Versions view.
    func add(kind: String, itemId: String, scope: String, videoId: String? = nil, season: Int? = nil, pick: (sub: UInt64, key: String)? = nil) {
        var fields: [String: any Sendable] = ["kind": kind, "itemId": itemId, "scope": scope, "params": DeviceCaps.params()]
        if let videoId { fields["videoId"] = videoId }
        if let season { fields["season"] = season }
        if let pick { fields["pick"] = ["versionsSub": pick.sub, "key": pick.key] as [String: any Sendable] }
        tapped("downloadAdd", fields)
    }

    /// A tap that starts work: the continued task, which waits for the op's answer however slow the add is
    /// (a season's meta on a cold connection); a second after it, or at once on an error, `answered` lets the
    /// task go when the core counted nothing. A core that never answers lets it go after `settleLimit` (review r3 N2).
    private func tapped(_ op: String, _ fields: [String: any Sendable]) {
        handle(.userAction)
        send(op, fields) { [weak self] error in
            Task { [weak self] in
                if error == nil { try? await Task.sleep(for: .seconds(1)) }
                self?.handle(.answered)
            }
        }
        Task { [weak self] in
            try? await Task.sleep(for: Self.settleLimit)
            self?.handle(.settle)
        }
    }

    nonisolated static let settleLimit: Duration = .seconds(60)

    /// Pause, Resume, Cancel or Delete (remove), Try again.
    func act(_ op: DownloadOp, _ target: DownloadTarget, mobileData: Bool = false) {
        var fields = target.fields
        if mobileData { fields["mobileData"] = true }
        if op == .resume || op == .retry { tapped(op.rawValue, fields) } else { send(op.rawValue, fields) }
    }

    /// An item of an existing download that is an op: sent, true. The rest go somewhere and are the caller's.
    @discardableResult
    func run(_ item: DownloadItem, id: String) -> Bool {
        guard let (op, mobileData) = DownloadCopy.op(item) else { return false }
        act(op, .ids([id]), mobileData: mobileData)
        return true
    }

    func setSettings(wifiOnly: Bool? = nil, smart: Bool? = nil) {
        var fields: [String: any Sendable] = [:]
        if let wifiOnly { fields["wifiOnly"] = wifiOnly }
        if let smart { fields["smart"] = smart }
        send("setDownloadSettings", fields)
    }

    func dismissFailure() { failure = nil }

    private func send(_ op: String, _ fields: [String: any Sendable], answered: (@MainActor @Sendable (CoreError?) -> Void)? = nil) {
        failure = nil
        core.sendNow(op, fields) { [weak self] error in
            if let error {
                coreLog.notice("downloads: \(op, privacy: .public) failed: \(error.localizedDescription, privacy: .public)")
                self?.failure = op == "downloadAdd" ? DownloadCopy.addFailed : DownloadCopy.actFailed
            }
            answered?(error)
        }
    }

    // MARK: Effects

    private func perform(_ effect: KeeperEffect) {
        switch effect {
        case .allowed(let allowed, let reason):
            var fields: [String: any Sendable] = ["allowed": allowed]
            if let reason { fields["reason"] = reason }
            if KeeperLogic.reportsFreeBytes(Self.platform), let free = freeBytes() { fields["freeBytes"] = free }
            core.sendNow("downloadsAllowed", fields) { _ in }
        case .submit(let progress): submit(progress)
        case .progress(let progress): show(progress)
        case .end(let success): end(success: success)
        case .beginActivity:
            #if os(macOS)
            activity = ProcessInfo.processInfo.beginActivity(options: Self.activityOptions, reason: "Downloading")
            #endif
        case .endActivity:
            #if os(macOS)
            if let activity { ProcessInfo.processInfo.endActivity(activity) }
            activity = nil
            #endif
        case .dock(let fraction):
            #if os(macOS)
            DockProgress.show(fraction)
            #endif
        }
    }

    /// The free space the core plans downloads with: `volumeAvailableCapacityForImportantUsage`
    /// (plans/offline-downloads.md 4.9), which counts what the system can purge.
    private func freeBytes() -> Int64? {
        let values = try? CoreClient.dataDirectory().resourceValues(forKeys: [.volumeAvailableCapacityForImportantUsageKey])
        return values?.volumeAvailableCapacityForImportantUsage
    }

    #if os(iOS)
    private func submit(_ progress: KeeperProgress) {
        guard #available(iOS 26, *), let bundle = Bundle.main.bundleIdentifier else { return handle(.expired) }
        // One identifier, under the wildcard in Info.plist (`<bundle>.downloads.*`), registered once for the
        // process: a handler per submit would stay registered for good (review r1 m5).
        let id = KeeperLogic.taskIdentifier(bundle: bundle)
        if !handlerRegistered {
            handlerRegistered = BGTaskScheduler.shared.register(forTaskWithIdentifier: id, using: .main) { [weak self] task in
                MainActor.assumeIsolated {
                    // A request ended before the system started it: nothing to keep.
                    guard let self, let task = task as? BGContinuedProcessingTask, self.submitted != nil else {
                        task.setTaskCompleted(success: true)
                        return
                    }
                    self.task = task
                    task.expirationHandler = { [weak self] in
                        MainActor.assumeIsolated { self?.handle(.expired) }
                    }
                    if let last = self.lastProgress { self.show(last) }
                }
            }
        }
        let registered = handlerRegistered
        let request = BGContinuedProcessingTaskRequest(identifier: id, title: progress.title, subtitle: progress.subtitle)
        request.strategy = .queue
        do {
            guard registered else { throw CoreError("not registered") }
            try BGTaskScheduler.shared.submit(request)
            submitted = id
            lastProgress = progress
        } catch {
            coreLog.notice("downloads: the continued task was refused: \(error.localizedDescription, privacy: .public)")
            // Refused: the downloads go on while the app is open, as below iOS 26.
            handle(.expired)
        }
    }

    @ObservationIgnored private var lastProgress: KeeperProgress?
    @ObservationIgnored private var handlerRegistered = false

    private func show(_ progress: KeeperProgress) {
        lastProgress = progress
        guard #available(iOS 26, *), let task = task as? BGContinuedProcessingTask else { return }
        task.progress.totalUnitCount = progress.total
        task.progress.completedUnitCount = min(progress.completed, progress.total)
        task.updateTitle(progress.title, subtitle: progress.subtitle)
    }

    private func end(success: Bool) {
        if #available(iOS 26, *) {
            if let task = task as? BGContinuedProcessingTask {
                task.setTaskCompleted(success: success)
            } else if let submitted {
                // Still waiting in the system's queue: it never starts.
                BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: submitted)
            }
        }
        task = nil
        submitted = nil
        lastProgress = nil
    }
    #else
    private func submit(_: KeeperProgress) {}
    private func show(_: KeeperProgress) {}
    private func end(success _: Bool) {}
    #endif
}

#if os(macOS)
/// The Dock icon's progress bar while downloads run (plans/offline-downloads.md 3.8).
@MainActor enum DockProgress {
    private final class Bar: NSView {
        var fraction = 0.0

        override func draw(_ dirtyRect: NSRect) {
            NSApp.applicationIconImage?.draw(in: bounds)
            let track = NSRect(x: bounds.width * 0.1, y: bounds.height * 0.08, width: bounds.width * 0.8, height: bounds.height * 0.09)
            let radius = track.height / 2
            NSColor.black.withAlphaComponent(0.55).setFill()
            NSBezierPath(roundedRect: track, xRadius: radius, yRadius: radius).fill()
            var filled = track
            filled.size.width = max(track.height, track.width * fraction)
            NSColor.white.setFill()
            NSBezierPath(roundedRect: filled, xRadius: radius, yRadius: radius).fill()
        }
    }

    static func show(_ fraction: Double?) {
        // No application (a test host): no Dock tile to draw on.
        guard NSApp != nil else { return }
        let tile = NSApp.dockTile
        if let fraction {
            let bar = tile.contentView as? Bar ?? Bar(frame: NSRect(origin: .zero, size: tile.size))
            bar.fraction = fraction
            tile.contentView = bar
        } else {
            tile.contentView = nil
        }
        tile.display()
    }
}

/// Quit while downloads run asks first: "Keep downloading" is the key, Quit holds the downloads until the
/// next launch (plans/offline-downloads.md 3.8).
final class MacAppDelegate: NSObject, NSApplicationDelegate {
    /// A logout, restart or shutdown never waits on the question (it would interrupt them): the downloads
    /// are held and the app goes. Cmd-Q, the menu and the Dock ask (review r1 m4).
    static func asks(quitReason: OSType?) -> Bool {
        let system: Set<OSType> = [
            OSType(kAELogOut), OSType(kAEReallyLogOut), OSType(kAEShowRestartDialog), OSType(kAERestart),
            OSType(kAEShowShutdownDialog), OSType(kAEShutDown),
        ]
        return quitReason.map { !system.contains($0) } ?? true
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        MainActor.assumeIsolated {
            guard let keeper = DownloadKeeper.current, keeper.asksBeforeQuit else { return .terminateNow }
            let reason = NSAppleEventManager.shared().currentAppleEvent?.attributeDescriptor(forKeyword: kAEQuitReason)?.enumCodeValue
            if Self.asks(quitReason: reason) {
                let alert = NSAlert()
                alert.messageText = "Downloads are in progress."
                alert.addButton(withTitle: "Keep downloading")
                alert.addButton(withTitle: "Quit")
                guard alert.runModal() == .alertSecondButtonReturn else { return .terminateCancel }
            }
            // The app goes once the core is told to hold the downloads (it answers, or 2 s pass).
            keeper.holdForQuit { sender.reply(toApplicationShouldTerminate: true) }
            return .terminateLater
        }
    }
}
#endif
#endif
