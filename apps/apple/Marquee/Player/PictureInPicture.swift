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

    func attach(_ layer: AVPlayerLayer) {
        guard controller == nil, AVPictureInPictureController.isPictureInPictureSupported(),
              let controller = AVPictureInPictureController(playerLayer: layer) else { return }
        controller.canStartPictureInPictureAutomaticallyFromInline = true
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
