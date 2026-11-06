// Plays one URL in AVPlayer, muted, in this process (never an app), and
// prints what it saw: `swift bench/dash/avplayer-check.swift <url>`. Ready,
// a decoded frame and its size, 10 s of play, then a seek to 600 s: where it
// lands and that it plays on. Exits 1 when any of them fails.
import AVFoundation
import Foundation

let url = URL(string: CommandLine.arguments[1])!
let item = AVPlayerItem(url: url)
let output = AVPlayerItemVideoOutput(pixelBufferAttributes: nil)
item.add(output)
let player = AVPlayer(playerItem: item)
player.isMuted = true
player.volume = 0

func wait(_ seconds: Double, until done: () -> Bool) -> Bool {
    let end = Date().addingTimeInterval(seconds)
    while Date() < end {
        if done() { return true }
        RunLoop.main.run(until: Date().addingTimeInterval(0.1))
    }
    return done()
}

func fail(_ what: String) -> Never {
    let log = item.errorLog()?.events.map { "\($0.errorStatusCode) \($0.errorComment ?? "")" } ?? []
    print("FAIL \(what) error=\(item.error.map { "\($0)" } ?? "none") log=\(log)")
    exit(1)
}

let started = Date()
player.play()
guard wait(30, until: { item.status != .unknown }), item.status == .readyToPlay else { fail("not ready") }
print("ready_ms=\(Int(Date().timeIntervalSince(started) * 1000))")
var frame = ""
let decoded = wait(20) {
    let now = item.currentTime()
    guard output.hasNewPixelBuffer(forItemTime: now),
          let buffer = output.copyPixelBuffer(forItemTime: now, itemTimeForDisplay: nil) else { return false }
    frame = "\(CVPixelBufferGetWidth(buffer))x\(CVPixelBufferGetHeight(buffer))"
    return true
}
guard decoded else { fail("no decoded frame") }
print("first_frame_ms=\(Int(Date().timeIntervalSince(started) * 1000)) frame=\(frame)")
guard wait(30, until: { player.currentTime().seconds >= 10 }) else { fail("did not play 10 s") }
print("played_to=\(String(format: "%.1f", player.currentTime().seconds))")
let kinds = item.tracks.compactMap { $0.assetTrack?.mediaType.rawValue }
print("tracks=\(kinds)")
var landed = -1.0
player.seek(to: CMTime(seconds: 600, preferredTimescale: 1000)) { _ in landed = player.currentTime().seconds }
guard wait(30, until: { landed >= 0 }) else { fail("the seek did not end") }
guard wait(30, until: { player.currentTime().seconds > landed + 2 }) else { fail("no play after the seek") }
print("seek_to=600 landed=\(String(format: "%.2f", landed)) now=\(String(format: "%.1f", player.currentTime().seconds))")
guard abs(landed - 600) <= 5 else { fail("the seek landed off") }
print("PASS")
