import Combine
import Foundation
import Testing

// plans/apple-second-engine.md 2.2 and 2.5: one switch per platform for the caps, the routing and the fallback;
// the routing by what the core says about the opened file; the caps in the core's own names.

@Test func theSecondEngineIsOnWhereItsPhaseTurnedItOn() {
    // Phase B turned the Mac on in its last commit (the owner, 2026-10-23); phase C turns the iPhone on; tvOS stays
    // off in v1.
    #expect(EngineChoice.switchOn(.mac))
    #expect(!EngineChoice.switchOn(.phone))
    #expect(!EngineChoice.switchOn(.tv))
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
    let phone = EngineChoice.switchOn(.phone)
    #expect(EngineChoice.containers(enabled: phone) == ["mp4", "m4v", "mov", "m3u8"])
    #expect(EngineChoice.audioDecoders(enabled: phone) == EngineChoice.avPlayerAudio)
    #expect(EngineChoice.containers(enabled: true).contains("mkv"))
    #expect(EngineChoice.audioDecoders(enabled: true).contains("true_hd"))
    #expect(EngineChoice.decoders(hardware: ["h264", "h265"], enabled: true, on: .phone) == ["h264", "h265"])
}

/// Review r1 (code) finding 4: the engine is on in own builds only, on every platform, and a missing or empty build
/// field is not an own build.
@Test func theEngineIsOnInOwnBuildsOnly() {
    #expect(EngineChoice.ownBuild("own"))
    for field: Any? in ["store", "", nil, 1] {
        #expect(!EngineChoice.ownBuild(field), "\(String(describing: field))")
    }
    #expect(EngineChoice.enabled(on: .mac, build: "own"))
    #expect(!EngineChoice.enabled(on: .mac, build: "store"))
    #expect(!EngineChoice.enabled(on: .mac, build: nil))
    #expect(!EngineChoice.enabled(on: .phone, build: "own"), "the iPhone's switch is still off")
    #expect(!EngineChoice.enabled(on: .tv, build: "own"))
}

/// Code r1 finding 1: a session on the engine's software host is refused where the platform admits no software
/// video, and reaches the core as `decode`, which skips the version's other sources and is remembered for the device.
@MainActor @Test func aRefusedSoftwareSessionReachesTheCoreAsADecodeFailure() {
    #expect(EngineChoice.refuses(softwareHost: true, on: .phone))
    #expect(EngineChoice.refuses(softwareHost: true, on: .tv))
    #expect(!EngineChoice.refuses(softwareHost: true, on: .mac))
    #expect(!EngineChoice.refuses(softwareHost: false, on: .phone))
    #expect(EngineChoice.failureKind(refused: true) == "decode")
    #expect(EngineChoice.failureKind(refused: false) == "other")
    // A reroute after a native session (AVPlayer gave up on a slow first segment) is not the device's: not remembered.
    #expect(EngineChoice.failureKind(refused: true, afterNative: true) == "other")
    // The start log names the path.
    #expect(AetherSession.path(native: true, software: false) == "native")
    #expect(AetherSession.path(native: false, software: true) == "software")
    #expect(AetherSession.path(native: false, software: false) == "other")
}

/// Code r1 finding 2: a new source while the window is up loads in place, so the window and the film go on.
@Test func aSourceChangeWithTheWindowUpLoadsInPlace() {
    #expect(!EngineChoice.stopsBeforeLoad(pictureInPictureActive: true))
    #expect(EngineChoice.stopsBeforeLoad(pictureInPictureActive: false))
}

/// The AirPlay button: AVPlayer's loopback rule, and never on the second engine, so none survives a fallback.
@Test func theAirPlayButtonFollowsTheBackend() throws {
    let direct = try #require(URL(string: "https://cdn.example.com/film.mp4"))
    let loopback = try #require(URL(string: "http://127.0.0.1:11470/film.mkv"))
    #expect(PlayerLayout.airPlayable(on: .avPlayer, direct))
    #expect(!PlayerLayout.airPlayable(on: .avPlayer, loopback))
    #expect(!PlayerLayout.airPlayable(on: .aether, direct))
}

/// Plan finding 5 (r1): the segment 0 wait reports loading, which is not playing, so the picture watchdog's clock
/// does not run on the engine; and the window attaches to each engine session's first picture (one wiring check).
@MainActor @Test func theEngineSessionsPictureDrivesTheWindowNotTheWatchdog() throws {
    #expect(!AetherSession.playState(.loading, buffering: false).playing)
    let dir = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Player")
    let screen = try String(contentsOf: dir.appending(path: "PlayerScreen.swift"), encoding: .utf8)
    #expect(screen.contains(".onChange(of: model.enginePictures) { attachEngine() }"))
}

/// The lead's call on plan r2: the 6 GB and no-iPad claim is a gate in code. A 6 GB iPhone is admitted, an iPad or a
/// 4 GB phone is not.
@Test func thePhoneSwitchCoversTheDevicesItsGateMeasured() {
    #expect(EngineChoice.admits(memoryBytes: 5_700_000_000, model: "iPhone14,2"))
    #expect(!EngineChoice.admits(memoryBytes: 3_800_000_000, model: "iPhone14,7"))
    #expect(!EngineChoice.admits(memoryBytes: 8_000_000_000, model: "iPad13,4"))
    // 6 GB or more: an 8 GB iPhone passes (the lead, 2026-10-25); a model that does not read as an iPhone closes it.
    #expect(EngineChoice.admits(memoryBytes: 7_700_000_000, model: "iPhone17,1"))
    #expect(!EngineChoice.admits(memoryBytes: 8_000_000_000, model: ""))
    #expect(!EngineChoice.admits(memoryBytes: 8_000_000_000, model: "arm64"))
}

/// Code r2 finding 1: a load with the window up keeps the engine, so each engine value it subscribes to still holds
/// the previous source's (its duration, first-frame flag, position, state). The session takes values for this load
/// only: none of the old ones, every new one.
@MainActor @Test func aLoadInTheWindowGetsItsOwnValuesNotThePreviousSources() async {
    let duration = CurrentValueSubject<Double, Never>(596)  // the episode before
    let shown = CurrentValueSubject<Bool, Never>(true)
    var got: [Double] = []
    var frames: [Bool] = []
    let a = AetherSession.fresh(duration).sink { got.append($0) }
    let b = AetherSession.fresh(shown).sink { frames.append($0) }
    duration.send(1320)  // this episode
    shown.send(false)
    shown.send(true)
    for _ in 0..<20 { await Task.yield() }
    try? await Task.sleep(for: .milliseconds(50))
    #expect(got == [1320])
    #expect(frames == [false, true])
    _ = (a, b)
}

/// Code r2 finding 5: the call sites of the refusal and the in-place load.
@Test func theRefusalAndTheInPlaceLoadAreWired() throws {
    let dir = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Player")
    let model = try String(contentsOf: dir.appending(path: "PlayerModel.swift"), encoding: .utf8)
    let session = try String(contentsOf: dir.appending(path: "AetherSession.swift"), encoding: .utf8)
    #expect(model.contains("refused: { [weak self] words, afterNative in\n                guard mine() else { return }\n                self?.fail(PlayerFailure(kind: EngineChoice.failureKind(refused: true, afterNative: afterNative)"))
    #expect(try body(of: "    func load(", in: session).contains("if EngineChoice.stopsBeforeLoad(pictureInPictureActive: engine.pictureInPictureActive) { stop() } else { release() }"))
}
