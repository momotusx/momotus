#if os(iOS)
import AVFoundation
import AVKit
import SwiftUI

/// Picture in Picture on the player's own layer: a button while the system
/// says it is possible, and it starts by itself when the app goes to the
/// background mid-film.
@MainActor @Observable
final class PictureInPicture {
    private(set) var possible = false
    @ObservationIgnored private var controller: AVPictureInPictureController?
    @ObservationIgnored private var observation: NSKeyValueObservation?
    @ObservationIgnored private let delegate = PipDelegate()
    /// The layer of a page shown again while the window still floats: the
    /// controller moves to it once the window closes.
    @ObservationIgnored private weak var waiting: AVPlayerLayer?
    /// The addon subtitle note was said for this film.
    @ObservationIgnored var told = false
    /// The window's own watch of the first frame, on the layer it keeps: a
    /// source loaded while the page is away (Up Next, a failover) still
    /// reports its picture (review apple-journey r3 P1-1).
    @ObservationIgnored private var frames: NSKeyValueObservation?
    @ObservationIgnored var onFrame: (@MainActor () -> Void)?
    /// A frame watch lives on the window's layer.
    var watchesFrames: Bool { frames != nil }

    /// What the page does as the window starts, comes back and closes.
    func on(start: @escaping @MainActor () -> Void, restore: @escaping @MainActor () -> Void, stop: @escaping @MainActor () -> Void) {
        delegate.start = start
        delegate.restore = restore
        delegate.stop = { [weak self] in
            stop()
            if let self, let layer = self.waiting { self.attach(layer) }
        }
    }

    func attach(_ layer: AVPlayerLayer) {
        guard controller?.playerLayer !== layer, AVPictureInPictureController.isPictureInPictureSupported() else { return }
        if controller?.isPictureInPictureActive == true {
            waiting = layer
            return
        }
        guard let controller = AVPictureInPictureController(playerLayer: layer) else { return }
        waiting = nil
        controller.canStartPictureInPictureAutomaticallyFromInline = true
        controller.delegate = delegate
        self.controller = controller
        observation = Self.watch(controller) { [weak self] possible in
            Task { @MainActor in self?.possible = possible }
        }
        frames = Self.watchFrames(layer) { [weak self] in
            Task { @MainActor in self?.onFrame?() }
        }
    }

    func start() {
        controller?.startPictureInPicture()
    }

    func stop() {
        controller?.stopPictureInPicture()
    }

    /// Made outside the main actor, as `watch`.
    private nonisolated static func watchFrames(_ layer: AVPlayerLayer, _ shown: @escaping @Sendable () -> Void) -> NSKeyValueObservation {
        layer.observe(\.isReadyForDisplay, options: [.initial, .new]) { layer, _ in
            if layer.isReadyForDisplay { shown() }
        }
    }

    /// Made outside the main actor: KVO calls back on the controller's queue.
    private nonisolated static func watch(
        _ controller: AVPictureInPictureController, _ update: @escaping @Sendable (Bool) -> Void
    ) -> NSKeyValueObservation {
        controller.observe(\.isPictureInPicturePossible, options: [.initial, .new]) { controller, _ in
            update(controller.isPictureInPicturePossible)
        }
    }
}

/// Picture in Picture's lifecycle, told to the page that started it; a start
/// that fails is logged (the error kind only, no address) and brings the
/// player back, as its page has already left. UIKit calls it on the main
/// thread, which the conformance checks at run time.
@MainActor private final class PipDelegate: NSObject, @preconcurrency AVPictureInPictureControllerDelegate {
    var start: (@MainActor () -> Void)?
    var restore: (@MainActor () -> Void)?
    var stop: (@MainActor () -> Void)?

    func pictureInPictureControllerWillStartPictureInPicture(_ controller: AVPictureInPictureController) {
        start?()
    }

    func pictureInPictureController(
        _ controller: AVPictureInPictureController,
        restoreUserInterfaceForPictureInPictureStopWithCompletionHandler completion: @escaping (Bool) -> Void
    ) {
        restore?()
        completion(true)
    }

    func pictureInPictureControllerDidStopPictureInPicture(_ controller: AVPictureInPictureController) {
        stop?()
    }

    func pictureInPictureController(_ controller: AVPictureInPictureController, failedToStartPictureInPictureWithError error: any Error) {
        coreLog.error("picture in picture did not start: \((error as NSError).domain, privacy: .public) \((error as NSError).code, privacy: .public)")
        restore?()
        stop?()
    }
}

/// A film that plays on in Picture in Picture after its page left, so the
/// viewer can browse; bringing it back shows it full screen over the places
/// (review apple-journey P2-4).
@MainActor @Observable
final class FloatingPlayer {
    struct Held: Identifiable {
        let model: PlayerModel
        let pip: PictureInPicture
        var id: ObjectIdentifier { ObjectIdentifier(model) }
    }

    @ObservationIgnored private var held: Held?
    /// The film brought back from the window, shown over the places.
    var shown: Held?
    /// Said while the film floats.
    private(set) var note: String?

    /// The window starts: the film plays on with no page.
    func float(_ held: Held) {
        self.held = held
        held.model.floating = true
        guard let words = PlayerLayout.pipSubtitleNote(addonSubtitle: held.model.subtitle != nil, fileSubtitle: held.model.textTrack != nil, told: held.pip.told) else { return }
        held.pip.told = true
        note = words
        announce(words)
        Task {
            try? await Task.sleep(for: .seconds(4))
            if note == words { note = nil }
        }
    }

    func restore() {
        shown = held
        playingAtRestore = (held?.model.player.rate ?? 0) > 0
    }

    /// The film was playing when the viewer brought it back. On the iPhone it
    /// was playing at the restore and paused by the time the window had closed
    /// (the walk's trace), a pause none of the app's code makes.
    @ObservationIgnored private var playingAtRestore = false

    /// The window closed. True when the film is back on screen; false when
    /// its X ended it.
    func stopped(_ model: PlayerModel) -> Bool {
        guard held?.model === model else { return true }
        held = nil
        model.floating = false
        let back = shown?.model === model
        // Brought back playing: it plays on, as the window showed it.
        if back, PlayerLayout.resumesAfterRestore(playingAtRestore: playingAtRestore, playingNow: model.player.rate > 0) {
            model.player.play()
        }
        playingAtRestore = false
        return back
    }

    /// Another film starting, or the profile's screens leaving, closes the window.
    func end(except model: PlayerModel? = nil) {
        guard let held, held.model !== model else { return }
        held.pip.stop()
    }
}

/// The system's AirPlay picker, which lists the TVs and speakers in reach.
struct AirPlayButton: UIViewRepresentable {
    func makeUIView(context: Context) -> AVRoutePickerView {
        let view = AVRoutePickerView()
        view.tintColor = .white
        view.activeTintColor = UIColor(Theme.Palette.accent)
        view.prioritizesVideoDevices = true
        return view
    }

    func updateUIView(_ view: AVRoutePickerView, context: Context) {}
}
#endif
