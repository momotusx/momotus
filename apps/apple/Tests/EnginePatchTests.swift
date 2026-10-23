@testable import AetherEngine
import AetherLibavutil
import CoreGraphics
import Foundation
import Testing

// The vendored engine's patch 3 (Vendor/AetherEngine/VENDOR.md, plans/apple-second-engine.md 2.6), run for
// real: every open has an empty protocol whitelist, so a source probed as HLS cannot open a `file:` segment,
// a local file does not open (accepted: Marquee has none), and a custom reader, which every Marquee source
// goes through, still opens.

/// A byte source over memory, as the engine's custom-reader path takes it.
private final class DataReader: IOReader, @unchecked Sendable {
    private let data: Data
    private var position = 0

    init(_ data: Data) { self.data = data }

    func read(_ buffer: UnsafeMutablePointer<UInt8>?, size: Int32) -> Int32 {
        guard let buffer, position < data.count else { return 0 }
        let count = min(Int(size), data.count - position)
        data.copyBytes(to: buffer, from: position..<(position + count))
        position += count
        return Int32(count)
    }

    func seek(offset: Int64, whence: Int32) -> Int64 {
        switch whence {
        case 65536: return Int64(data.count)
        case SEEK_SET: position = Int(offset)
        case SEEK_CUR: position += Int(offset)
        case SEEK_END: position = data.count + Int(offset)
        default: return -1
        }
        return Int64(position)
    }

    func close() {}
    var discImageProbeEnabled: Bool { false }
}

private let fixtures = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "Fixtures")

@Test func aCustomReaderStillOpens() throws {
    let mkv = try Data(contentsOf: fixtures.appending(path: "tiny.mkv"))
    let probe = try AetherEngine.probe(source: .custom(DataReader(mkv), formatHint: "matroska"))
    #expect(probe.videoCodecName == "h264")
    #expect(probe.videoWidth == 64)
}

@Test func aPlaylistCannotOpenALocalFile() throws {
    // A crafted source that FFmpeg probes as HLS and that names a file on this machine as its segment. Two
    // walls refuse it: FFmpeg's HLS demuxer opens a `file:` segment only from a `file:` playlist, and patch 3's
    // empty whitelist refuses every nested open; this pins the outcome, `aLocalFileDoesNotOpen` the patch.
    let target = fixtures.appending(path: "tiny.mkv").path(percentEncoded: false)
    let playlist = "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1.0,\nfile://\(target)\n#EXT-X-ENDLIST\n"
    #expect(throws: (any Error).self) {
        _ = try AetherEngine.probe(source: .custom(DataReader(Data(playlist.utf8)), formatHint: "hls"))
    }
}

@Test func aLocalFileDoesNotOpen() {
    #expect(throws: (any Error).self) {
        _ = try AetherEngine.probe(url: fixtures.appending(path: "tiny.mkv"))
    }
}

/// The engine's cues as the overlay draws them: a styled cue reads as all its runs, a bitmap cue is not text.
@MainActor @Test func aStyledCueReadsAsItsWholeText() throws {
    let runs = [SubtitleTextRun(text: "two ", color: nil, isItalic: true), SubtitleTextRun(text: "runs", color: nil)]
    let styled = try #require(AetherSession.cue(SubtitleCue(id: 1, startTime: 2, endTime: 3.5, body: .richText(runs))))
    #expect((styled.start, styled.end, styled.text) == (2, 3.5, "two runs"))
    #expect(AetherSession.cue(SubtitleCue(id: 2, startTime: 4, endTime: 5, body: .text("plain")))?.text == "plain")
    let image = try #require(CGContext(data: nil, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4,
        space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)?.makeImage())
    let bitmap = SubtitleCue(id: 3, startTime: 6, endTime: 7, body: .image(SubtitleImage(cgImage: image, position: .zero)))
    #expect(AetherSession.cue(bitmap) == nil)
    // A bitmap is an image cue, placed where the disc put it (review apple-aether P2-5).
    let drawn = try #require(AetherSession.image(SubtitleCue(id: 4, startTime: 6, endTime: 7,
        body: .image(SubtitleImage(cgImage: image, position: CGRect(x: 0.25, y: 0.8, width: 0.5, height: 0.1))))))
    #expect((drawn.start, drawn.end, drawn.position) == (6, 7, CGRect(x: 0.25, y: 0.8, width: 0.5, height: 0.1)))
    #expect(AetherSession.image(SubtitleCue(id: 5, startTime: 4, endTime: 5, body: .text("plain"))) == nil)
}

/// A bitmap cue on a 1920x800 film fitted into a 1280x760 window: letterboxed, its [0, 1] rect scales with the
/// picture, not the window.
@Test func aBitmapCueSitsOnTheFittedPicture() {
    let rect = PlayerLayout.imageCueRect(position: CGRect(x: 0.25, y: 0.8, width: 0.5, height: 0.1),
                                         video: CGSize(width: 1920, height: 800), view: CGSize(width: 1280, height: 760))
    // The picture is 1280x533.33, 113.33 down from the top.
    #expect(abs(rect.minX - 320) < 0.01 && abs(rect.width - 640) < 0.01)
    #expect(abs(rect.minY - (113.333 + 426.667)) < 0.01 && abs(rect.height - 53.333) < 0.01)
    #expect(PlayerLayout.imageCueRect(position: .zero, video: .zero, view: CGSize(width: 10, height: 10)) == .zero)
}

/// Review apple-aether P2-4: the engine forgets the speed on a load of another source; the session sets the
/// viewer's again once the source is open. And a stop while it opens means it does not play.
@MainActor @Test func theViewersSpeedHoldsAcrossAnEngineLoad() async throws {
    let session = try #require(AetherSession())
    let mkv = try Data(contentsOf: fixtures.appending(path: "tiny.mkv"))
    session.load(.custom(DataReader(mkv), formatHint: "matroska"), container: "mkv", at: 0, rate: 1.5, muted: true, openWithin: .seconds(30), events: .none)
    for _ in 0..<100 where session.engine.desiredRate == nil { try await Task.sleep(for: .milliseconds(100)) }
    #expect(session.engine.desiredRate == 1.5)
    session.stop()
}

@MainActor @Test func aSessionStoppedWhileItOpensDoesNotPlay() async throws {
    let session = try #require(AetherSession())
    let mkv = try Data(contentsOf: fixtures.appending(path: "tiny.mkv"))
    session.load(.custom(DataReader(mkv), formatHint: "matroska"), container: "mkv", at: 0, rate: 1.5, muted: true, openWithin: .seconds(30), events: .none)
    session.stop()
    try await Task.sleep(for: .seconds(2))
    #expect(session.engine.state != .playing)
    #expect(session.engine.desiredRate == nil)
}

extension AetherSession.Events {
    static var none: Self {
        .init(time: { _ in }, duration: { _ in }, state: { _, _ in }, firstFrame: {}, ended: {}, failed: { _ in }, tracks: { _ in }, cues: { _, _ in }, videoSize: { _ in })
    }
}

/// Matroska starts at a 1 MiB probe (its codec parameters sit in its header), everything else at 8 MiB; a probe that
/// came back with the picture and the audio's parameters needs no second one.
@MainActor @Test func matroskaProbesSmallFirst() throws {
    #expect(AetherSession.firstProbeMiB(container: "mkv") == 1)
    #expect(AetherSession.firstProbeMiB(container: "WEBM") == 1)
    #expect(AetherSession.firstProbeMiB(container: "mp4") == 8)
    #expect(AetherSession.firstProbeMiB(container: nil) == 8)
    #expect(AetherSession.options(probeMiB: 1).probesize == 1 << 20)
    #expect(AetherSession.options(probeMiB: 8).maxAnalyzeDuration == nil)
    let mkv = try Data(contentsOf: fixtures.appending(path: "tiny.mkv"))
    let probe = try AetherEngine.probe(source: .custom(DataReader(mkv), formatHint: "matroska"))
    #expect(AetherSession.resolved(probe, audio: [], active: nil))
    #expect(AetherSession.resolved(nil, audio: [], active: nil), "no probe, nothing to check")
}

/// Patch 4: segment 0 ends at the first keyframe at or after 1 s; the later segments keep the absolute 4 s cuts.
@Test func segmentZeroEndsAtTheFirstKeyframeAfterOneSecond() {
    let tb = AVRational(num: 1, den: 1000)
    // A film that opens on a 2.08 s GOP (a logo), then 10 s GOPs.
    let opening = HLSVideoEngine.buildKeyframeSegmentPlan(
        keyframes: [0, 2080, 12346, 22346, 32346], videoTimeBase: tb, sourceDurationSeconds: 40)
    #expect(opening.map { ($0.startSeconds * 1000).rounded() } == [0, 2080, 12346, 22346, 32346])
    // Keyframes every 0.5 s: segment 0 is 1 s, then the cuts land on 8, 12, 16 s as before.
    let dense = HLSVideoEngine.buildKeyframeSegmentPlan(
        keyframes: Array(stride(from: Int64(0), through: 20000, by: 500)), videoTimeBase: tb, sourceDurationSeconds: 20.5)
    #expect(dense.prefix(4).map { ($0.startSeconds * 1000).rounded() } == [0, 1000, 8000, 12000])
}

/// The spinner after the first frame is the engine's real stall only: playing without a rebuffer is no wait, a
/// rebuffer is, a pause is neither (the lead's PGS strip, 2026-10-23).
@MainActor @Test func theEnginesWaitIsItsStallOnly() {
    #expect(AetherSession.playState(.playing, buffering: false) == (true, false))
    #expect(AetherSession.playState(.playing, buffering: true) == (false, true))
    #expect(AetherSession.playState(.paused, buffering: false) == (false, false))
    #expect(AetherSession.playState(.loading, buffering: false) == (false, true))
}
