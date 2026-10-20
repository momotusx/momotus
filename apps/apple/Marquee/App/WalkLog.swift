#if DEBUG
import AVFoundation
import Combine
import os

/// `-MQWalkLog YES`: once a second while a playback plays or waits, one
/// `walk {...}` line on stderr and in the core log, the row the Android walk
/// log writes too, so a phone walk reads like the Mac's CLI rows:
/// - `stalls`, `stall_ms`: waits for data after the first play (as the CLI
///   counts mpv's paused-for-cache; a seek's wait counts too);
/// - `player_stalls`, `observed_bitrate` (bits/s), `startup_ms`: the player's
///   own access log, null where it has none;
/// - `free_bytes`: the volume's free space for important use (null on tvOS).
@MainActor
final class WalkLog {
    static var on: Bool { UserDefaults.standard.bool(forKey: "MQWalkLog") }

    private weak var player: AVPlayer?
    private let opened = ContinuousClock.now
    private var status = AVPlayer.TimeControlStatus.paused
    private var played = false
    private var stalls = 0
    private var waitedFrom: ContinuousClock.Instant?
    private var waited: Duration = .zero
    private var watch: AnyCancellable?

    /// Logs until the player's model lets go of it.
    init(player: AVPlayer) {
        self.player = player
        watch = player.publisher(for: \.timeControlStatus)
            .receive(on: DispatchQueue.main)
            .sink { [weak self] status in MainActor.assumeIsolated { self?.changed(status) } }
        Task { [weak self] in
            while true {
                try? await Task.sleep(for: .seconds(1))
                guard let self else { return }
                self.log()
            }
        }
    }

    private func changed(_ status: AVPlayer.TimeControlStatus) {
        self.status = status
        let now = ContinuousClock.now
        if status == .waitingToPlayAtSpecifiedRate {
            if played, waitedFrom == nil {
                stalls += 1
                waitedFrom = now
            }
        } else if let from = waitedFrom {
            waited += now - from
            waitedFrom = nil
        }
        played = played || status == .playing
    }

    private func log() {
        guard status != .paused, let player else { return }
        let now = ContinuousClock.now
        let events = player.currentItem?.accessLog()?.events ?? []
        let counted = events.map(\.numberOfStalls).filter { $0 >= 0 }
        let position = player.currentTime().seconds
        let row: [String: Any] = [
            "kind": "playback",
            "t_ms": ms(now - opened),
            "position_ms": position.isFinite ? Int(position * 1000) : 0,
            "stalls": stalls,
            "stall_ms": ms(waited + (waitedFrom.map { now - $0 } ?? .zero)),
            "player_stalls": counted.isEmpty ? NSNull() : counted.reduce(0, +),
            "observed_bitrate": events.last.flatMap { $0.observedBitrate >= 0 ? Int($0.observedBitrate) : nil } ?? NSNull(),
            "startup_ms": events.first.flatMap { $0.startupTime >= 0 ? Int($0.startupTime * 1000) : nil } ?? NSNull(),
            "free_bytes": freeBytes() ?? NSNull(),
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: row, options: .sortedKeys),
              let json = String(data: data, encoding: .utf8) else { return }
        FileHandle.standardError.write(Data("walk \(json)\n".utf8))
        coreLog.notice("walk \(json, privacy: .public)")
    }

    private func ms(_ duration: Duration) -> Int {
        Int(duration.components.seconds * 1000 + duration.components.attoseconds / 1_000_000_000_000_000)
    }

    private func freeBytes() -> Int64? {
        #if os(tvOS)
        nil
        #else
        try? URL.homeDirectory.resourceValues(forKeys: [.volumeAvailableCapacityForImportantUsageKey])
            .volumeAvailableCapacityForImportantUsage
        #endif
    }
}
#endif
