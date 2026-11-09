import AetherEngine
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
    // A reroute because AVPlayer waited on bytes (a slow first segment) is not the device's: not remembered.
    #expect(EngineChoice.failureKind(refused: true, waited: true) == "other")
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
    #expect(model.contains("refused: { [weak self] words, waited in\n                guard mine() else { return }\n                self?.fail(PlayerFailure(kind: EngineChoice.failureKind(refused: true, waited: waited)"))
    let load = try body(of: "    func load(", in: session)
    #expect(load.contains("if EngineChoice.stopsBeforeLoad(pictureInPictureActive: engine.pictureInPictureActive) { stop() } else { release() }"))
    // Code r3: the refusal reads the reroute the engine announced in this load.
    #expect(load.contains("Self.recording(engine.softwarePathEscalations) { failure in\n            MainActor.assumeIsolated { if mine() { rerouted = failure } }"))
    #expect(load.contains("let waited = rerouted.map { EngineChoice.waited(domain: $0.underlyingDomain, code: $0.underlyingCode) } ?? false"))
    #expect(load.contains("decode this picture in software\", waited)"))
    #expect(load.contains("rerouted=\\(rerouted == nil ? 0 : 1"))
    #expect(load.contains("reroute=\\(Self.reroute(rerouted)"))
}

/// Code r3: the reroute is classified on the failure the engine absorbed, so AVPlayer waiting on bytes is told apart
/// from AVPlayer refusing the picture, which stays `decode` and is remembered for the device.
@Test func aRerouteIsClassifiedOnWhyAVPlayerGaveUp() {
    #expect(EngineChoice.waited(domain: "CoreMediaErrorDomain", code: -12889), "no media segment in time (AS41)")
    #expect(EngineChoice.waited(domain: "CoreMediaErrorDomain", code: -12888), "a playlist that stopped growing")
    #expect(EngineChoice.waited(domain: "CoreMediaErrorDomain", code: nil), "item death at a frozen position")
    #expect(!EngineChoice.waited(domain: "CoreMediaErrorDomain", code: -12911), "a decode refusal")
    #expect(!EngineChoice.waited(domain: "CoreMediaErrorDomain", code: -11821))
    #expect(!EngineChoice.waited(domain: "AetherEngine.LiveJoin", code: nil), "no entry point: about the media")
    #expect(!EngineChoice.waited(domain: nil, code: nil))
}

/// Code r3: the reroute is recorded the moment the engine announces it, not on a later main-queue turn, so the
/// refusal after it reads what happened.
@MainActor @Test func aRerouteIsRecordedWhenTheEngineAnnouncesIt() {
    let escalations = PassthroughSubject<SoftwarePathEscalationEvent, Never>()
    var got: PlaybackErrorInfo?
    let watch = AetherSession.recording(escalations) { got = $0 }
    let failure = PlaybackErrorInfo(kind: .nativeItemFailed, message: "no response", underlyingDomain: "CoreMediaErrorDomain", underlyingCode: -12889)
    escalations.send(SoftwarePathEscalationEvent(absorbedFailure: failure, positionSeconds: 0, duringStartup: true))
    #expect(got == failure)
    _ = watch
}

/// Code r3 suggestion 3: every engine value goes through `fresh`, so no later subscription replays the previous
/// source's value into an in-place load. A source check: the sites are the code.
@Test func everyEngineValueGoesThroughFresh() throws {
    let dir = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Player")
    let session = try String(contentsOf: dir.appending(path: "AetherSession.swift"), encoding: .utf8)
    let lines = session.split(separator: "\n").filter { $0.contains(/\.\$[a-zA-Z]/) }
    #expect(lines.count >= 8)
    #expect(lines.allSatisfy { $0.contains("Self.fresh(") }, "\(lines.filter { !$0.contains("Self.fresh(") })")
}

/// Code r5: the bar's one swarm figure is the start window's bytes as the core counted them over the time between its
/// first and last start snapshots with bytes; a new source starts it over; one snapshot says nothing.
@Test func theStartWindowRateIsTheCoresBytesOverTheirTime() throws {
    let t = ContinuousClock.now
    var window = StartWindow()
    window.saw(0, at: t)
    window.saw(1 << 20, at: t + .seconds(1))
    #expect(window.kibPerS == "n/a")
    window.saw(3 << 20, at: t + .seconds(3))
    #expect(window.kibPerS == "1024")
    // The next source: its count starts lower, so the window starts over.
    window.saw(1 << 19, at: t + .seconds(4))
    #expect(window.kibPerS == "n/a")
    window.saw(1 << 20, at: t + .seconds(6))
    #expect(window.kibPerS == "256")
    // Code r6: the raw ends ride along, milliseconds from the tap.
    #expect(window.ends(from: t) == "start_bytes_first=524288 start_ms_first=4000 start_bytes_last=1048576 start_ms_last=6000")
    #expect(StartWindow().ends(from: t) == "start_bytes_first=n/a start_ms_first=n/a start_bytes_last=n/a start_ms_last=n/a")
    // The iphone-a soak (2026-10-26): a start already whole at its first snapshot gained nothing, so it reads n/a, not 0.
    var whole = StartWindow()
    whole.saw(8 << 20, at: t + .seconds(4))
    whole.saw(8 << 20, at: t + .seconds(5))
    #expect(whole.kibPerS == "n/a")
    // Code r9, the soak's real sequence: the core sends no key during a start, so the ready line ends the window. The
    // first play's line reads its own ends; a version move's start, at the same count, reads only itself.
    var soak = StartWindow()
    soak.saw(2 << 20, at: t + .seconds(1))
    soak.saw(8 << 20, at: t + .seconds(4))
    #expect(soak.line(from: t) == "start_kib_s=2048 start_bytes_first=2097152 start_ms_first=1000 start_bytes_last=8388608 start_ms_last=4000")
    soak.saw(8 << 20, at: t + .seconds(50))
    #expect(soak.line(from: t + .seconds(49)) == "start_kib_s=n/a start_bytes_first=8388608 start_ms_first=1000 start_bytes_last=8388608 start_ms_last=1000")
    // Its call sites: every ready ends the window before the first-play check, and the failed line ends it too.
    let model = try playerModel()
    #expect(model.contains("let startLine = startWindow.line(from: route.tapped)\n            if loadedURL == nil, !rendered {"))
    #expect(model.contains("let startLine = startWindow.line(from: route.tapped)\n                coreLog.named { $0.notice(\"metric playback_failed_ms="))
    // Code r10: a version move before the last start was ready starts the window over.
    #expect(model.contains("// A move before the last start was ready: the new start measures only itself.\n        startWindow = StartWindow()"))
}

/// Code r5: the walk's screen hold is compiled into walk builds only: `isIdleTimerDisabled` appears in the app only in
/// WalkSampler.swift, whose first line is `#if MARQUEE_WALK`. A source check: the guard is the compilation condition.
@Test func theScreenHoldIsInWalkBuildsOnly() throws {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    let files = try #require(FileManager.default.enumerator(at: app, includingPropertiesForKeys: nil))
        .compactMap { $0 as? URL }.filter { $0.pathExtension == "swift" }
    let holding = try files.filter { try String(contentsOf: $0, encoding: .utf8).contains("isIdleTimerDisabled") }
    #expect(holding.map(\.lastPathComponent) == ["WalkSampler.swift"])
    let sampler = try String(contentsOf: app.appending(path: "App/WalkSampler.swift"), encoding: .utf8)
    #expect(sampler.hasPrefix("#if MARQUEE_WALK\n"))
}

/// The walk's read-backs: a seek's landing against its target, and the file's tracks with the one playing.
@Test func theWalkReadsBackTheSeekAndTheTracks() throws {
    #expect(WalkReadBack.seek(target: 90, position: 91.2) == "seek_landed target_s=90.0 position_s=91.2 off_s=1.2 landed=yes")
    #expect(WalkReadBack.seek(target: 162, position: 0.8).hasSuffix("landed=no"), "a seek that started from the top did not land")
    #expect(WalkReadBack.seek(target: 90, position: 86).hasSuffix("landed=yes"), "a keyframe before the target")
    // Code r8: the window is -6 s to +2 s; 4 s past the target skipped it.
    #expect(WalkReadBack.seek(target: 90, position: 94).hasSuffix("landed=no"))
    #expect(WalkReadBack.seek(target: 90, position: 83.5).hasSuffix("landed=no"))
    #expect(WalkReadBack.seek(target: 90, position: 92).hasSuffix("landed=yes"))
    // A seek while playing plays on once two reads inside the window move forward.
    #expect(WalkReadBack.playsOn(target: 90, before: 88.1, after: 88.6))
    #expect(!WalkReadBack.playsOn(target: 90, before: 88.6, after: 88.6), "a frozen position has not played on")
    #expect(!WalkReadBack.playsOn(target: 90, before: 30.0, after: 30.5), "still before the seek")
    #expect(WalkReadBack.tracks(text: ["English", "German"], activeText: 1, audio: ["English, 5.1"], activeAudio: 0)
        == "tracks text=[English | German] text_on=1 audio=[English, 5.1] audio_on=0")
    #expect(WalkReadBack.tracks(text: [], activeText: nil, audio: [], activeAudio: nil) == "tracks text=[] text_on=none audio=[] audio_on=none")
    // Its call site, walk builds only (a source check: the guard is the compilation condition).
    let model = try playerModel()
    #expect(model.contains("#if MARQUEE_WALK\n        readBack()\n        #endif"))
    #expect(model.contains("#if MARQUEE_WALK\n                walkSeekTarget = at\n                #endif"))
}

/// Heat on the iPhone (IM-W3): at serious, an engine play above 1080p moves to the best listed version at 1080p or under;
/// with none it pauses; anything else keeps playing.
@Test func aHotPhoneMovesA4KEnginePlayToItsScreenOrPauses() throws {
    let rows = [(key: "a", quality: "4K"), (key: "b", quality: "4K"), (key: "c", quality: "1080p"), (key: "d", quality: "720p")]
    #expect(EngineChoice.heatStep(hot: true, on: .phone, engine: true, playing: "a", rows: rows) == .move("c"))
    #expect(EngineChoice.heatStep(hot: true, on: .phone, engine: true, playing: "a", rows: Array(rows.prefix(2))) == .pause)
    #expect(EngineChoice.heatStep(hot: false, on: .phone, engine: true, playing: "a", rows: rows) == .keep)
    #expect(EngineChoice.heatStep(hot: true, on: .mac, engine: true, playing: "a", rows: rows) == .keep)
    #expect(EngineChoice.heatStep(hot: true, on: .phone, engine: false, playing: "a", rows: rows) == .keep, "AVPlayer decodes out of process")
    #expect(EngineChoice.heatStep(hot: true, on: .phone, engine: true, playing: "c", rows: rows) == .keep, "1080p is the screen's class")
    #expect(EngineChoice.heatStep(hot: true, on: .phone, engine: true, playing: "x", rows: rows) == .keep, "an unlisted play")
    #expect(EngineChoice.heatStep(hot: true, on: .phone, engine: true, playing: "a", rows: [(key: "a", quality: "4K"), (key: "w", quality: "WEB-DL")]) == .pause, "a version of unknown height is not cooler")
    // Code r10: a 1440p play is above the screen's class too.
    #expect(EngineChoice.heatStep(hot: true, on: .phone, engine: true, playing: "q", rows: [(key: "q", quality: "1440p"), (key: "c", quality: "1080p")]) == .move("c"))
    // The first cooler version in the list's order, not the lowest.
    #expect(EngineChoice.heatStep(hot: true, on: .phone, engine: true, playing: "a", rows: [(key: "a", quality: "4K"), (key: "d", quality: "720p"), (key: "c", quality: "1080p")]) == .move("d"))
    // Code r10, the action through its seam: a mid-play pause is a real pause of a play that is playing or waiting
    // (never a toggle, which would start a waiting play); at a source's start the load is held instead.
    #expect(EngineChoice.heatAction(.pause, atStart: false, playing: false, waiting: true) == .pause)
    #expect(EngineChoice.heatAction(.pause, atStart: false, playing: true, waiting: false) == .pause)
    #expect(EngineChoice.heatAction(.pause, atStart: false, playing: false, waiting: false) == .none, "a paused play is not paused again")
    #expect(EngineChoice.heatAction(.pause, atStart: true, playing: false, waiting: true) == .holdStart)
    #expect(EngineChoice.heatAction(.move("c"), atStart: true, playing: false, waiting: true) == .move("c"))
    #expect(EngineChoice.heatAction(.keep, atStart: false, playing: true, waiting: false) == .none)
    // Its wiring (a source check): every thermal change runs it mid-play; every ready runs it before the load, and a
    // move takes the load's place; the pause is a real pause, not togglePlay; the next episode gets its own step.
    let model = try playerModel()
    #expect(model.contains("self.heatCheck(atStart: false, engine: self.backend == .aether)"))
    #expect(model.contains("if heatCheck(atStart: true, engine: pick == .aether) { return }\n            #endif\n            // Another source: a stall on the last one ends with it.\n            sendStall(stall.interrupted())\n            load(url, key: snapshot.key, attempt: snapshot.attempt, at: start)"))
    #expect(model.contains("if backend == .aether, let aether { aether.pause() } else { player.pause() }"))
    #expect(try body(of: "    private func heatCheck(", in: model).contains("togglePlay") == false)
    #expect(model.contains("heatHandled = false\n        selectSubtitle(nil)"))
    // The heat pause's line stays until the viewer plays again.
    #expect(model.contains("notices.show(words, seconds: untilPlay ? nil : seconds, priority: priority)"))
    #expect(model.contains("if !playing { notices.clear(Self.heatPaused) }"))
    #expect(model.contains(".filter { $0.key == playingKey || $0.key != route.key }"))
}

