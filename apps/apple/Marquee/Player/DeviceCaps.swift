import AVFoundation
import SwiftUI
import VideoToolbox

/// What this device's player can play, sent with every Versions view so the
/// core ranks and hides sources for it. AVPlayer opens MP4 and MOV; where the second engine is on
/// (`EngineChoice`), MKV, AVI and DTS or TrueHD audio too.
enum DeviceCaps {
    /// The connection speed is measured for: "wifi", "ethernet", "cellular"
    /// or "other", kept by `SystemEvents` from the network path.
    @MainActor static var network = "other"

    @MainActor static func params() -> [String: any Sendable] {
        let decoders = Self.decoders
        #if os(macOS)
        // The Mac has no list of HDR modes: an HDR screen plays HDR10 and HLG. Dolby Vision follows VideoToolbox's
        // `dvh1` decode (an M4 Pro: yes, bench AS36) once `claimsDolbyVision` is on (plan 2.7).
        let hdr = StreamChoice.hdrNames(
            eligible: AVPlayer.eligibleForHDRPlayback, hdr10: true, hlg: true,
            dolbyVision: claimsDolbyVision && VTIsHardwareDecodeSupported(kCMVideoCodecType_DolbyVisionHEVC))
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
            // No hardware decodes 10-bit H.264 here; software decode waits for the resource gate (plan 2.5).
            "tenBitH264": false,
            "audioDecoders": EngineChoice.audioDecoders(enabled: EngineChoice.enabledHere),
            "containers": containers,
            "hasFallbackEngine": false,
            // AVFoundation shows HEVC in MP4 only from an entry with its parameter sets.
            "hvc1Only": true,
            "language": language ?? "en",
        ]
    }

    /// The Mac claims Dolby Vision only once a Profile 5 and a Profile 7 row have been walked on it: the engine path
    /// showed both P8.1 rows as HDR10 (bench AS34), and a claim only steers the ranking toward DV rows (plan 2.7).
    static let claimsDolbyVision = false

    /// The device's first preferred language as its base code ("tr").
    /// `Locale.current` follows the app's own localizations, English only, so
    /// on a Turkish iPhone it says "en".
    static var language: String? {
        Locale.preferredLanguages.first.flatMap { Locale(identifier: $0).language.languageCode?.identifier }
    }

    /// AVPlayer's files and HLS playlists, and the second engine's containers where it is on.
    private static var containers: [String] { EngineChoice.containers(enabled: EngineChoice.enabledHere) }

    private static var decoders: [String] {
        var decoders = ["h264"]
        if VTIsHardwareDecodeSupported(kCMVideoCodecType_HEVC) { decoders.append("h265") }
        if VTIsHardwareDecodeSupported(kCMVideoCodecType_AV1) { decoders.append("av1") }
        return EngineChoice.decoders(hardware: decoders, enabled: EngineChoice.enabledHere)
    }

    /// The screen as the stream choice words name it: "4K HDR, about 42 Mbit/s on Wi-Fi".
    @MainActor static func conditionsLine(_ c: StreamConditions?, account: Bool) -> String {
        StreamChoice.conditionsLine(
            screenClass: StreamChoice.screenClass(lines: displayHeight()), hdr: AVPlayer.eligibleForHDRPlayback,
            network: c?.network ?? network,
            speed: c.map { StreamChoice.speed(http: $0.http, p2p: $0.p2p, engine: $0.engine, account: account) } ?? Speed(mbps: 0, samples: 0))
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
