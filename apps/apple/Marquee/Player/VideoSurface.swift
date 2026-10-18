import AVFoundation
import SwiftUI

/// The video layer. `onFirstFrame` runs each time the layer becomes ready to
/// show a frame (the model counts only the first).
struct VideoSurface {
    let player: AVPlayer
    let onFirstFrame: @MainActor () -> Void
    /// iOS: the layer, for Picture in Picture.
    var onLayer: (@MainActor (AVPlayerLayer) -> Void)?

    @MainActor fileprivate func attach(_ layer: AVPlayerLayer) -> NSKeyValueObservation {
        layer.player = player
        layer.videoGravity = .resizeAspect
        layer.backgroundColor = CGColor(gray: 0, alpha: 1)
        let onFirstFrame = onFirstFrame
        return layer.observe(\.isReadyForDisplay, options: [.initial, .new]) { layer, _ in
            guard layer.isReadyForDisplay else { return }
            DispatchQueue.main.async { MainActor.assumeIsolated { onFirstFrame() } }
        }
    }
}

#if os(macOS)
final class PlayerLayerView: NSView {
    let playerLayer = AVPlayerLayer()
    var observation: NSKeyValueObservation?

    override init(frame: NSRect) {
        super.init(frame: frame)
        layer = playerLayer
        wantsLayer = true
    }

    required init?(coder: NSCoder) { fatalError("not used") }
}

extension VideoSurface: NSViewRepresentable {
    func makeNSView(context: Context) -> PlayerLayerView {
        let view = PlayerLayerView()
        view.observation = attach(view.playerLayer)
        return view
    }

    func updateNSView(_ view: PlayerLayerView, context: Context) {}
}
#else
final class PlayerLayerView: UIView {
    override static var layerClass: AnyClass { AVPlayerLayer.self }
    var playerLayer: AVPlayerLayer { layer as! AVPlayerLayer }
    var observation: NSKeyValueObservation?
}

extension VideoSurface: UIViewRepresentable {
    func makeUIView(context: Context) -> PlayerLayerView {
        let view = PlayerLayerView()
        view.observation = attach(view.playerLayer)
        onLayer?(view.playerLayer)
        return view
    }

    func updateUIView(_ view: PlayerLayerView, context: Context) {}
}
#endif
