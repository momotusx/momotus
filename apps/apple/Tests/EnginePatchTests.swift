import AetherEngine
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
