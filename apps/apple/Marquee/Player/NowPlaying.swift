import AVFoundation
import MediaPlayer
#if canImport(UIKit)
import UIKit
#endif

/// The system's side of a playback: the audio session that keeps a film's
/// sound with the Silent switch on and on a locked screen, and the Lock
/// Screen and Control Center's Now Playing with play, pause, 10 s skips and
/// seeking (AirPods, a remote, the Lock Screen scrubber).
@MainActor
final class NowPlaying {
    private var targets: [(command: MPRemoteCommand, token: Any)] = []
    private var artwork: MPMediaItemArtwork?

    /// Playback, not ambient sound: the Silent switch does not mute it, and
    /// with the audio background mode it plays on with the screen locked.
    static func activate() {
        #if os(iOS) || os(tvOS)
        let session = AVAudioSession.sharedInstance()
        try? session.setCategory(.playback, mode: .moviePlayback)
        try? session.setActive(true)
        #endif
    }

    /// Lets other apps' audio resume once the player is gone.
    static func deactivate() {
        #if os(iOS) || os(tvOS)
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        #endif
    }

    /// The remote commands, held until `stop`; each runs on the main actor.
    func start(_ model: PlayerModel) {
        let center = MPRemoteCommandCenter.shared()
        add(center.playCommand) { [weak model] _ in
            Task { @MainActor in if model?.playing == false { model?.togglePlay() } }
        }
        add(center.pauseCommand) { [weak model] _ in
            Task { @MainActor in if model?.playing == true { model?.togglePlay() } }
        }
        add(center.togglePlayPauseCommand) { [weak model] _ in
            Task { @MainActor in model?.togglePlay() }
        }
        center.skipBackwardCommand.preferredIntervals = [10]
        center.skipForwardCommand.preferredIntervals = [10]
        add(center.skipBackwardCommand) { [weak model] _ in
            Task { @MainActor in model?.skip(-10) }
        }
        add(center.skipForwardCommand) { [weak model] _ in
            Task { @MainActor in model?.skip(10) }
        }
        add(center.changePlaybackPositionCommand) { [weak model] event in
            guard let event = event as? MPChangePlaybackPositionCommandEvent else { return }
            let seconds = event.positionTime
            Task { @MainActor in model?.seek(to: seconds) }
        }
    }

    private func add(_ command: MPRemoteCommand, _ run: @escaping @Sendable (MPRemoteCommandEvent) -> Void) {
        command.isEnabled = true
        targets.append((command, Self.target(command, run)))
    }

    /// The system calls handlers on its own queues: they are made outside the
    /// main actor, so none of them claims it (Swift 6 traps when one does).
    private nonisolated static func target(_ command: MPRemoteCommand, _ run: @escaping @Sendable (MPRemoteCommandEvent) -> Void) -> Any {
        command.addTarget { event in
            run(event)
            return .success
        }
    }

    /// What the Lock Screen shows: the title, the episode, the poster, the
    /// length, where the playhead is and how fast it moves.
    func update(title: String, episode: String?, duration: Double, position: Double, rate: Float) {
        // Only the playback that runs speaks: SwiftUI builds and drops spare
        // player models, and one of those published "paused at 0:00" over a
        // film playing at 2x.
        guard !targets.isEmpty else { return }
        var info: [String: Any] = [
            MPMediaItemPropertyTitle: title,
            MPNowPlayingInfoPropertyElapsedPlaybackTime: position,
            MPNowPlayingInfoPropertyPlaybackRate: Double(rate),
            MPNowPlayingInfoPropertyMediaType: MPNowPlayingInfoMediaType.video.rawValue,
        ]
        if let episode { info[MPMediaItemPropertyArtist] = episode }
        if duration > 0 { info[MPMediaItemPropertyPlaybackDuration] = duration }
        if let artwork { info[MPMediaItemPropertyArtwork] = artwork }
        MPNowPlayingInfoCenter.default().nowPlayingInfo = info
    }

    /// The poster, once, for the Lock Screen.
    func loadArtwork(_ url: String?) async {
        #if canImport(UIKit)
        guard artwork == nil, let art = await ArtLoader.shared.image(url, maxPixels: ArtSize.poster) else { return }
        artwork = Self.artwork(UIImage(cgImage: art.image))
        #endif
    }

    #if canImport(UIKit)
    /// Made outside the main actor: Now Playing asks for the image on its own queue.
    private nonisolated static func artwork(_ image: UIImage) -> MPMediaItemArtwork {
        MPMediaItemArtwork(boundsSize: image.size) { _ in image }
    }
    #endif

    func stop() {
        for (command, token) in targets { command.removeTarget(token) }
        targets = []
        MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
    }
}
