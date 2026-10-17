import AVFoundation
import SwiftUI
import VideoToolbox

/// What this device's player can play, sent with every Versions view so the
/// core ranks and hides sources for it. AVPlayer is the engine for now: it
/// opens MP4 and MOV, not MKV, and there is no fallback engine yet.
enum DeviceCaps {
    @MainActor static func params() -> [String: any Sendable] {
        let decoders = Self.decoders
        var hdr: [String] = []
        if AVPlayer.eligibleForHDRPlayback { hdr += ["hdr10", "hlg"] }
        return [
            "displayHeight": displayHeight(),
            "hdr": hdr,
            "decoders": decoders,
            "tenBitHevc": decoders.contains("h265"),
            "audioDecoders": ["aac", "ac3", "eac3", "atmos", "flac", "mp3", "pcm"],
            "containers": containers,
            "hasFallbackEngine": false,
            "language": Locale.current.language.languageCode?.identifier ?? "en",
        ]
    }

    private static let containers = ["mp4", "m4v", "mov"]

    private static var decoders: [String] {
        var decoders = ["h264"]
        if VTIsHardwareDecodeSupported(kCMVideoCodecType_HEVC) { decoders.append("h265") }
        if VTIsHardwareDecodeSupported(kCMVideoCodecType_AV1) { decoders.append("av1") }
        return decoders
    }

    /// "This screen: 1080p, plays H.264, HEVC in MP4, M4V, MOV": what Versions ranks for.
    @MainActor static var line: String {
        VersionWords.device(screen: displayWords, decoders: decoders, containers: containers)
    }

    /// "4K, HDR", "1080p": the display as the setup check names it.
    @MainActor static var displayWords: String {
        let lines = displayHeight()
        let name = lines >= 2000 ? "4K" : lines >= 1000 ? "1080p" : "720p"
        return AVPlayer.eligibleForHDRPlayback ? "\(name), HDR" : name
    }

    /// The display's short side in pixels: what a landscape video fills.
    @MainActor private static func displayHeight() -> Int {
        #if os(macOS)
        guard let screen = NSScreen.main else { return 1080 }
        return Int(min(screen.frame.width, screen.frame.height) * screen.backingScaleFactor)
        #else
        let screen = UIApplication.shared.connectedScenes.compactMap { ($0 as? UIWindowScene)?.screen }.first
        guard let bounds = screen?.nativeBounds else { return 1080 }
        return Int(min(bounds.width, bounds.height))
        #endif
    }
}
