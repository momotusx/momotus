import AVFoundation
import SwiftUI
import VideoToolbox

/// What this device's player can play, sent with every Versions view so the
/// core ranks and hides sources for it. AVPlayer is the engine for now: it
/// opens MP4 and MOV, not MKV, and there is no fallback engine yet.
enum DeviceCaps {
    /// The connection speed is measured for: "wifi", "ethernet", "cellular"
    /// or "other", kept by `SystemEvents` from the network path.
    @MainActor static var network = "other"

    @MainActor static func params() -> [String: any Sendable] {
        let decoders = Self.decoders
        #if os(macOS)
        // The Mac has no list of HDR modes: an HDR screen plays HDR10 and HLG; Dolby Vision is not claimed.
        let hdr = StreamChoice.hdrNames(eligible: AVPlayer.eligibleForHDRPlayback, hdr10: true, hlg: true, dolbyVision: false)
        #else
        let modes = AVPlayer.availableHDRModes
        let hdr = StreamChoice.hdrNames(
            eligible: AVPlayer.eligibleForHDRPlayback, hdr10: modes.contains(.hdr10), hlg: modes.contains(.hlg),
            dolbyVision: modes.contains(.dolbyVision))
        #endif
        return [
            "displayHeight": displayHeight(),
            "hdr": hdr,
            "network": network,
            "decoders": decoders,
            "tenBitHevc": decoders.contains("h265"),
            "audioDecoders": ["aac", "ac3", "eac3", "atmos", "flac", "mp3", "pcm"],
            "containers": containers,
            "hasFallbackEngine": false,
            // AVFoundation shows HEVC in MP4 only from an entry with its parameter sets.
            "hvc1Only": true,
            "language": language ?? "en",
        ]
    }

    /// The device's first preferred language as its base code ("tr").
    /// `Locale.current` follows the app's own localizations, English only, so
    /// on a Turkish iPhone it says "en".
    static var language: String? {
        Locale.preferredLanguages.first.flatMap { Locale(identifier: $0).language.languageCode?.identifier }
    }

    /// AVPlayer's files, and HLS playlists (`m3u8`), which it plays natively.
    private static let containers = ["mp4", "m4v", "mov", "m3u8"]

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

    /// The screen as the stream choice words name it: "4K HDR, about 42 Mbit/s on Wi-Fi".
    @MainActor static func conditionsLine(_ c: StreamConditions?, account: Bool) -> String {
        StreamChoice.conditionsLine(
            screenClass: StreamChoice.screenClass(lines: displayHeight()), hdr: AVPlayer.eligibleForHDRPlayback,
            network: c?.network ?? network,
            speed: c.map { StreamChoice.speed(http: $0.http, p2p: $0.p2p, engine: $0.engine, account: account) } ?? Speed(mbps: 0, samples: 0),
            freeBytes: c?.freeBytes)
    }

    /// "4K, HDR", "1080p": the display as the setup check names it.
    @MainActor static var displayWords: String {
        let lines = displayHeight()
        let name = lines >= 2000 ? "4K" : lines >= 1000 ? "1080p" : "720p"
        return AVPlayer.eligibleForHDRPlayback ? "\(name), HDR" : name
    }

    /// The display's short side in pixels: what a landscape video fills.
    @MainActor static func displayHeight() -> Int {
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
