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

/// plans/iphone-mkv.md: the iPhone's caps while its switch is off (MKV hidden as today), and what it sends once the
/// last commit of that plan turns it on: every engine container and audio, hardware video only.
@Test func theIPhonesCapsWithItsSwitchOffAndOn() {
    let phone = EngineChoice.enabled(on: .phone)
    #expect(EngineChoice.containers(enabled: phone) == ["mp4", "m4v", "mov", "m3u8"])
    #expect(EngineChoice.audioDecoders(enabled: phone) == EngineChoice.avPlayerAudio)
    #expect(EngineChoice.containers(enabled: true).contains("mkv"))
    #expect(EngineChoice.audioDecoders(enabled: true).contains("true_hd"))
    #expect(EngineChoice.decoders(hardware: ["h264", "h265"], enabled: true, on: .phone) == ["h264", "h265"])
}

/// plans/iphone-mkv.md: Picture in Picture on the second engine, the native path's layer before the software
/// path's, nothing on AVPlayer (its layer attaches itself) or before the engine shows a surface.
@Test func pictureInPictureTakesTheEnginesSurface() throws {
    #expect(PlayerLayout.pipSurface(engine: true, native: true, software: true) == .engineNative)
    #expect(PlayerLayout.pipSurface(engine: true, native: false, software: true) == .engineSoftware)
    #expect(PlayerLayout.pipSurface(engine: true, native: false, software: false) == nil)
    #expect(PlayerLayout.pipSurface(engine: false, native: true, software: true) == nil)
    // Wiring: the screen attaches the engine's surface once it shows, and tells the engine the window is up.
    let dir = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Player")
    let screen = try String(contentsOf: dir.appending(path: "PlayerScreen.swift"), encoding: .utf8)
    #expect(screen.contains(".onChange(of: model.firstFrameMs) { attachEngine() }"))
    let float = try body(of: "private func float()", in: screen)
    #expect(float.contains("model.pictureInPicture(active: true)"))
    #expect(float.contains("model.pictureInPicture(active: false)"))
    // A film brought back from the window plays on, on whichever engine.
    let pip = try String(contentsOf: dir.appending(path: "PictureInPicture.swift"), encoding: .utf8)
    #expect(try body(of: "func stopped(_ model: PlayerModel)", in: pip).contains("model.resume()"))
    #expect(!pip.contains("model.player."))
}

/// plans/iphone-mkv.md, review r1 finding 2: the iPhone's switch counts in own builds only; a store build keeps MKV
/// hidden whatever the switch says.
@Test func theIPhoneSwitchCountsInOwnBuildsOnly() {
    #expect(EngineChoice.phone(switchOn: true, build: "own"))
    #expect(!EngineChoice.phone(switchOn: true, build: "store"))
    #expect(!EngineChoice.phone(switchOn: false, build: "own"))
    #expect(EngineChoice.enabled(on: .phone) == EngineChoice.phone(switchOn: EngineChoice.phoneSwitch, build: CoreBuild.field))
}

/// Review r1 finding 1: software video only where admitted, with the path named in the start log; finding 5: a
/// loading engine session runs no picture clock; the AirPlay button goes when the engine opens.
@MainActor @Test func theEngineKeepsToWhatThePlatformAdmits() throws {
    #expect(EngineChoice.allowsSoftwareVideo(on: .mac))
    #expect(!EngineChoice.allowsSoftwareVideo(on: .phone))
    #expect(!EngineChoice.allowsSoftwareVideo(on: .tv))
    #expect(AetherSession.path(native: true, software: false) == "native")
    #expect(AetherSession.path(native: false, software: true) == "software")
    #expect(AetherSession.path(native: false, software: false) == "other")
    let dir = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Player")
    let session = try String(contentsOf: dir.appending(path: "AetherSession.swift"), encoding: .utf8)
    #expect(session.contains("guard mine(), source != nil, !EngineChoice.allowsSoftwareVideo(on: EngineChoice.platform) else { return }"))
    #expect(session.contains("path=\\(Self.path(native: host.nativePlayerLayer != nil, software: host.softwarePiPSource != nil)"))
    // The segment 0 wait reports loading: not playing, so the watchdog's clock does not run.
    #expect(!AetherSession.playState(.loading, buffering: false).playing)
    let model = try String(contentsOf: dir.appending(path: "PlayerModel.swift"), encoding: .utf8)
    #expect(try body(of: "private func engineState(", in: model).contains("playingSince = playing ? playingSince ?? .now : nil"))
    #expect(try body(of: "private func openOnEngine(", in: model).contains("airPlayable = false"))
}
