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

    func attach(_ layer: AVPlayerLayer) {
        guard controller == nil, AVPictureInPictureController.isPictureInPictureSupported(),
              let controller = AVPictureInPictureController(playerLayer: layer) else { return }
        controller.canStartPictureInPictureAutomaticallyFromInline = true
        controller.delegate = delegate
        self.controller = controller
        observation = Self.watch(controller) { [weak self] possible in
            Task { @MainActor in self?.possible = possible }
        }
    }

    func start() {
        controller?.startPictureInPicture()
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

/// Picture in Picture's lifecycle: the player stays where it was while the
/// window floats, so coming back restores it at once; a start that fails is
/// logged (the error kind only, no address).
private final class PipDelegate: NSObject, AVPictureInPictureControllerDelegate {
    func pictureInPictureController(
        _ controller: AVPictureInPictureController,
        restoreUserInterfaceForPictureInPictureStopWithCompletionHandler completion: @escaping (Bool) -> Void
    ) {
        completion(true)
    }

    func pictureInPictureController(_ controller: AVPictureInPictureController, failedToStartPictureInPictureWithError error: any Error) {
        coreLog.error("picture in picture did not start: \((error as NSError).domain, privacy: .public) \((error as NSError).code, privacy: .public)")
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
