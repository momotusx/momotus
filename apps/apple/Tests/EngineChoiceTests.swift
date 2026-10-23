import Foundation
import Testing

// plans/apple-second-engine.md 2.2 and 2.5: one switch per platform for the caps, the routing and the fallback;
// the routing by what the core says about the opened file; the caps in the core's own names.

@Test func theSecondEngineIsOnWhereItsPhaseTurnedItOn() {
    // Phase B turned the Mac on in its last commit (the owner, 2026-10-23); phase C turns the iPhone on; tvOS stays
    // off in v1.
    #expect(EngineChoice.enabled(on: .mac))
    #expect(!EngineChoice.enabled(on: .phone))
    #expect(!EngineChoice.enabled(on: .tv))
}

@Test func whereTheSwitchIsOffEverythingStaysOnAVPlayer() {
    #expect(EngineChoice.backend(container: "mkv", audio: [], engine: nil, enabled: false) == .avPlayer)
    #expect(EngineChoice.backend(container: "mp4", audio: ["dts_hd_ma"], engine: nil, enabled: false) == .avPlayer)
    #expect(EngineChoice.backend(container: "mkv", audio: [], engine: "aether", enabled: false) == .avPlayer)
    // A refusal there goes to the core as today: no fallback.
    #expect(!EngineChoice.fallsBack(enabled: false, from: .avPlayer, firstFrameShown: false, kind: "decode"))
    #expect(EngineChoice.containers(enabled: false) == ["mp4", "m4v", "mov", "m3u8"])
    #expect(EngineChoice.audioDecoders(enabled: false) == EngineChoice.avPlayerAudio)
}

@Test func whereTheSwitchIsOnTheOpenedFileDecides() {
    func on(_ container: String?, _ audio: [String] = [], engine: String? = nil) -> Backend {
        EngineChoice.backend(container: container, audio: audio, engine: engine, enabled: true)
    }
    #expect(on("mkv") == .aether)
    #expect(on("AVI") == .aether)
    #expect(on("mp4") == .avPlayer)
    #expect(on("m3u8") == .avPlayer)
    // An unknown container plays on AVPlayer first; its refusal falls back.
    #expect(on(nil) == .avPlayer)
    // An MP4 whose only audio AVPlayer cannot decode; Atmos alone says nothing.
    #expect(on("mp4", ["dts_hd_ma"]) == .aether)
    #expect(on("mp4", ["true_hd", "atmos"]) == .aether)
    #expect(on("mp4", ["aac", "dts"]) == .avPlayer)
    #expect(on("mp4", ["atmos"]) == .avPlayer)
    // A version that played on the engine before goes there first.
    #expect(on("mp4", engine: "aether") == .aether)
    #expect(EngineChoice.fallsBack(enabled: true, from: .avPlayer, firstFrameShown: false, kind: "decode"))
    #expect(!EngineChoice.fallsBack(enabled: true, from: .avPlayer, firstFrameShown: true, kind: "decode"))
    #expect(!EngineChoice.fallsBack(enabled: true, from: .aether, firstFrameShown: false, kind: "decode"))
}

@Test func theCapsAreTheCoresNames() throws {
    // The same file the core's versions_params test reads (mq-bridge), so a name it does not know fails both.
    let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "Fixtures/engine-caps.json")
    let fixture = try #require(JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
    #expect(fixture["containers"] as? [String] == EngineChoice.containers(enabled: true))
    #expect(fixture["audioDecoders"] as? [String] == EngineChoice.audioDecoders(enabled: true))
    #expect(fixture["decoders"] as? [String] == EngineChoice.decoders(hardware: ["h264", "h265", "av1"], enabled: true, on: .mac))
    #expect(EngineChoice.decoders(hardware: ["h264", "h265"], enabled: false, on: .mac) == ["h264", "h265"], "off: hardware only")
    #expect(EngineChoice.decoders(hardware: ["h264", "h265"], enabled: true, on: .phone) == ["h264", "h265"], "the iPhone waits for its gate")
    #expect(fixture["tenBitH264"] as? Bool == false)
}

@Test func theEngineOpensWithinWhatTheStartBudgetHasLeft() {
    #expect(EngineChoice.openWithin(budgetEndsAtMs: 50_000, nowMs: 20_000) == .seconds(30))
    #expect(EngineChoice.openWithin(budgetEndsAtMs: 20_000, nowMs: 25_000) == .seconds(1), "spent: a last second")
    #expect(EngineChoice.openWithin(budgetEndsAtMs: nil, nowMs: 25_000) == .seconds(90), "no budget sent")
}

@Test func onlyADecodeOrOtherFailureFallsBack() {
    // Plan 2.3: a source that did not answer goes to the core's next one at once; the engine would wait on it too.
    for kind in ["decode", "other"] {
        #expect(EngineChoice.fallsBack(enabled: true, from: .avPlayer, firstFrameShown: false, kind: kind), "\(kind)")
    }
    for kind in ["network", "timeout", "http", "audio"] {
        #expect(!EngineChoice.fallsBack(enabled: true, from: .avPlayer, firstFrameShown: false, kind: kind), "\(kind)")
    }
}

/// The Mac claims Dolby Vision only after a Profile 5 and a Profile 7 row are walked on it (plan 2.7, bench AS34,
/// AS36): those walks come first, then this line changes with the claim.
@Test func theMacDoesNotClaimDolbyVisionYet() {
    #expect(!DeviceCaps.claimsDolbyVision)
}
