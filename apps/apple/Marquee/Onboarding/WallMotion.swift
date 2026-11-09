import Foundation
import Observation
import SwiftUI

/// When the poster wall drifts (plans/first-screens.md 3.1, D4): on Welcome only, never under a sheet, never with
/// Reduce Motion (or the walk's `-MQReduceMotion`), never while the scene is not active, never after Pause motion.
enum WallMotion {
    static func runs(step: FirstRunStep, reduceMotion: Bool, scenePhase: ScenePhase, paused: Bool) -> Bool {
        step == .welcome && !reduceMotion && scenePhase == .active && !paused
    }

    /// Pause motion's choice, kept on this device (a local setting, never synced).
    static let pausedKey = "firstRunWallPaused"

    /// The toggle is named by what pressing it does.
    static func toggleLabel(paused: Bool, words: FirstRunWords = .shared) -> String {
        paused ? words.playMotion : words.pauseMotion
    }
}

/// The drift's clock for the app run: it stops while the wall holds still and goes on from there, so Back to
/// Welcome resumes where it stopped.
@MainActor final class WallClock {
    static let shared = WallClock()
    private var kept: TimeInterval = 0
    private var since: Date?

    func elapsed(at now: Date) -> TimeInterval {
        kept + (since.map { now.timeIntervalSince($0) } ?? 0)
    }

    func set(running: Bool, at now: Date) {
        if running, since == nil { since = now }
        if !running, let started = since {
            kept += now.timeIntervalSince(started)
            since = nil
        }
    }
}

/// The wall's posters (`firstRunWall`): a list, once one came, is kept for the app run; an empty answer (offline, past
/// the core's budget) or an ask that failed is asked again at the next step, so the art comes late instead of never.
@MainActor @Observable final class WallOnce {
    static let shared = WallOnce()
    private(set) var posters: [String] = []
    @ObservationIgnored private var asked: Task<[String], any Error>?

    @discardableResult
    func get(_ fetch: @escaping @Sendable () async throws -> [String]) async -> [String] {
        if !posters.isEmpty { return posters }
        let task = asked ?? Task { try await fetch() }
        asked = task
        let list = (try? await task.value) ?? []
        if list.isEmpty {
            if asked == task { asked = nil }
        } else {
            posters = list
        }
        return list
    }
}

/// The wall's geometry (round2.html:171 to :176): columns of 2:3 tiles 14 apart in a box 28 percent wider than the
/// screen on each side and 18 percent taller, turned -9 degrees about the screen's centre; each column holds its list
/// twice, odd columns (from 1) run down and start 160 higher, 70, 84 or 98 s a loop.
struct WallLayout: Equatable {
    static let gap: CGFloat = 14
    static let radius: CGFloat = 12
    static let degrees: Double = -9
    static let lift: CGFloat = 160

    let screen: CGSize
    let columns: Int
    let count: Int

    var wall: CGSize { CGSize(width: screen.width * 1.56, height: screen.height * 1.36) }
    var origin: CGPoint { CGPoint(x: -screen.width * 0.28, y: -screen.height * 0.18) }
    var tile: CGSize {
        let width = max((wall.width - Self.gap * CGFloat(columns - 1)) / CGFloat(max(columns, 1)), 1)
        return CGSize(width: width, height: width * 1.5)
    }

    /// Tiles in one list: enough to cover the wall's height and the lift, so the loop never shows a gap.
    var rows: Int { max(Int(((wall.height + Self.lift) / (tile.height + Self.gap)).rounded(.up)), 1) }
    /// One list's height: the drift's loop.
    var loop: CGFloat { CGFloat(rows) * (tile.height + Self.gap) }

    static func seconds(_ column: Int) -> Double { 70 + Double(column % 3) * 14 }

    /// Welcome's black ramp from the top down, (share of the height, opacity) (round2.html:179, :181).
    static let welcomePhone: [(at: Double, alpha: Double)] = [(0, 0.25), (0.35, 0.45), (0.66, 0.92), (0.82, 1)]
    static let welcomeWide: [(at: Double, alpha: Double)] = [(0, 0.2), (0.3, 0.5), (0.54, 0.9), (0.72, 1)]

    /// The ramp's opacity at `at`, a share of the height.
    static func shade(_ stops: [(at: Double, alpha: Double)], at: Double) -> Double {
        guard let next = stops.firstIndex(where: { $0.at >= at }) else { return stops.last?.alpha ?? 0 }
        guard next > 0 else { return stops[0].alpha }
        let (a, b) = (stops[next - 1], stops[next])
        return a.alpha + (b.alpha - a.alpha) * (at - a.at) / (b.at - a.at)
    }

    /// The column's poster for its `row` (of the list shown twice): each column starts further along the list.
    func poster(_ column: Int, _ row: Int, in posters: [String]) -> String? {
        guard !posters.isEmpty else { return nil }
        return posters[(column * rows + row % rows) % posters.count]
    }

    /// The column's offset at `t` seconds of drift.
    func offset(_ column: Int, at t: TimeInterval) -> CGFloat {
        let share = CGFloat((t / Self.seconds(column)).truncatingRemainder(dividingBy: 1))
        return column % 2 == 0 ? -share * loop : -(1 - share) * loop - Self.lift
    }

    /// The tiles whose centres are on screen at `t`, as (column, row): what a walk counts as visible.
    func visible(at t: TimeInterval) -> [(column: Int, row: Int)] {
        let angle = Self.degrees * .pi / 180
        let centre = CGPoint(x: screen.width / 2, y: screen.height / 2)
        var shown: [(Int, Int)] = []
        for column in 0..<columns {
            let x = origin.x + CGFloat(column) * (tile.width + Self.gap) + tile.width / 2
            for row in 0..<(rows * 2) {
                let y = origin.y + offset(column, at: t) + CGFloat(row) * (tile.height + Self.gap) + tile.height / 2
                let dx = Double(x - centre.x), dy = Double(y - centre.y)
                let px = Double(centre.x) + dx * cos(angle) - dy * sin(angle)
                let py = Double(centre.y) + dx * sin(angle) + dy * cos(angle)
                if px >= 0, px <= Double(screen.width), py >= 0, py <= Double(screen.height) { shown.append((column, row)) }
            }
        }
        return shown
    }
}

/// `first-wall poster <ms> tiles6s <n>/<visible>` (plans/first-screens.md 3.1, 9): the bench's line, once per app run;
/// PosterWall writes it in walk and debug builds.
@MainActor final class WallLog {
    static let shared = WallLog()
    private var began: ContinuousClock.Instant?
    private var firstMs: Int?
    private var done = false

    /// The wall's first frame; false once this run has logged. A size change in the first 6 s (a rotation, a window
    /// resize) starts the 6 s again and keeps the first poster's time.
    func start() -> Bool {
        guard !done else { return false }
        if began == nil { began = .now }
        return true
    }

    func drew() {
        guard firstMs == nil, let began else { return }
        let elapsed = ContinuousClock.now - began
        firstMs = Int(elapsed.components.seconds * 1000 + elapsed.components.attoseconds / 1_000_000_000_000_000)
    }

    /// The line, the first time only.
    func settled(filled: Int, visible: Int) -> String? {
        guard !done else { return nil }
        done = true
        return "first-wall poster \(firstMs.map(String.init) ?? "none") tiles6s \(filled)/\(visible)"
    }
}
