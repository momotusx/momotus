import Foundation

/// How a platform keeps downloads going once the user leaves (plans/offline-downloads.md 5).
enum KeeperPlatform: Sendable {
    /// iOS 26 and later: one `BGContinuedProcessingTask` per queue, submitted on a user's tap.
    case continued
    /// iOS 18 to 25: downloads run while the app is open; leaving holds them (`app`).
    case foreground
    /// macOS: a user-initiated activity while downloads run, the Dock tile's progress.
    case mac
}

/// What the keeper reads from a `downloads` snapshot.
struct KeeperFacts: Equatable, Sendable {
    /// Running and queued.
    var left: Int
    var bytesDone: Int64
    var bytesTotal: Int64
    var title: String?
    /// Every done download's id: the batch counts those done since it began.
    var done: Set<String>

    static func of(_ snapshot: DownloadsSnapshot) -> KeeperFacts {
        KeeperFacts(
            left: snapshot.active.running + snapshot.active.queued, bytesDone: snapshot.active.bytesDone,
            bytesTotal: snapshot.active.bytesTotal, title: snapshot.active.title,
            done: Set(snapshot.titles.flatMap(\.rows).filter { $0.state == .done }.map(\.id)))
    }

    /// The running downloads' share done, nil with no size yet.
    var fraction: Double? { bytesTotal > 0 ? min(1, max(0, Double(bytesDone) / Double(bytesTotal))) : nil }
}

enum KeeperEvent: Equatable, Sendable {
    case facts(KeeperFacts)
    /// The user tapped Download, Resume or Try again (a download op that starts work).
    case userAction
    /// The system ended the continued task (pressure, or the user cancelled it in its progress UI).
    case expired
    case background
    case foreground
    /// Once a minute while downloads run.
    case minute
    /// A few seconds after a tap: a tap the core never counted (nothing to download) lets the task go.
    case settle
}

/// The system progress item's words and units: 100 units an item.
struct KeeperProgress: Equatable, Sendable {
    let title: String
    let subtitle: String
    let completed: Int64
    let total: Int64
}

enum KeeperEffect: Equatable, Sendable {
    /// Submit the continued task (iOS 26 and later).
    case submit(KeeperProgress)
    /// The running task's progress: a second tap grows its total.
    case progress(KeeperProgress)
    /// End the task: the queue emptied (true) or the system took it (false).
    case end(success: Bool)
    /// `downloadsAllowed`; `reason` "app" when the platform holds them.
    case allowed(Bool, reason: String?)
    /// macOS: `ProcessInfo.beginActivity(.userInitiated)` while downloads run, and its end.
    case beginActivity
    case endActivity
    /// macOS: the Dock tile's progress, nil to clear it.
    case dock(Double?)
}

/// The keeper as a pure reducer, shared by iOS and macOS; the iOS and macOS keepers only perform its
/// effects. One task per queue: a tap while one runs grows its total instead of submitting another.
struct KeeperLogic {
    let platform: KeeperPlatform
    private(set) var visible = true
    private(set) var task = false
    private var facts: KeeperFacts?
    private var activity = false
    private var dockShown = false
    /// What was last sent, so `downloadsAllowed` goes on every change.
    private var sent: (allowed: Bool, left: Int)?
    /// Done ids when the batch began, and taps the core has not counted yet.
    private var doneBefore: Set<String> = []
    private var taps = 0

    init(platform: KeeperPlatform) {
        self.platform = platform
    }

    /// Whether `downloadsAllowed` carries `freeBytes`: only iOS, where `volumeAvailableCapacityForImportantUsage`
    /// counts what the system purges for the app. A Mac never sends it: purgeable space there (local snapshots,
    /// caches) is not purged in time, and the core plans with the disk's own figure (plans/offline-downloads.md 4.9,
    /// review r1 M3).
    static func reportsFreeBytes(_ platform: KeeperPlatform) -> Bool { platform != .mac }

    /// Whether quitting asks first (macOS): "Downloads are in progress."
    static func asksBeforeQuit(_ facts: KeeperFacts?) -> Bool { (facts?.left ?? 0) > 0 }

    var current: KeeperFacts? { facts }

    /// A view opened again (a new core, another profile): whatever was sent went to a core that may be
    /// gone, so the next event says `downloadsAllowed` again.
    mutating func resend() {
        sent = nil
    }

    mutating func on(_ event: KeeperEvent) -> [KeeperEffect] {
        var out: [KeeperEffect] = []
        var force = false
        switch event {
        case .facts(let now):
            let before = facts
            facts = now
            // A snapshot already on its way when the user tapped still says nothing runs: the tap stays
            // counted, and the task stays, until the core counts it (or `settle`) (review r1 m1).
            if now.left > 0 { taps = 0 }
            if now.left > 0, (before?.left ?? 0) == 0, !task { doneBefore = before?.done ?? now.done }
            if task {
                if now.left == 0, taps == 0 {
                    task = false
                    out.append(.end(success: true))
                } else {
                    out.append(.progress(progress()))
                }
            }
            mac(&out)
        case .userAction:
            guard platform == .continued, visible else { break }
            taps += 1
            if task {
                out.append(.progress(progress()))
            } else {
                task = true
                doneBefore = facts?.done ?? []
                out.append(.submit(progress()))
            }
        case .expired:
            // The decision for an expiry in the foreground (the user cancelling the system's progress item while
            // the app is open, review r1 m2): the task ends, and the downloads go on while the app is open, as
            // they do below iOS 26; leaving the app then holds them (`app`). In the background it holds them now.
            if task {
                task = false
                out.append(.end(success: false))
            }
        case .background:
            visible = false
        case .foreground:
            visible = true
            force = true
        case .minute:
            force = (facts?.left ?? 0) > 0
        case .settle:
            if task, (facts?.left ?? 0) == 0 {
                taps = 0
                task = false
                out.append(.end(success: true))
            }
        }
        allowed(&out, force: force)
        return out
    }

    /// Whether downloads may run now: in front, or held by the system's task; always on the Mac.
    private var wantAllowed: Bool {
        switch platform {
        case .mac: true
        case .continued: visible || task
        case .foreground: visible
        }
    }

    private mutating func allowed(_ out: inout [KeeperEffect], force: Bool) {
        let want = wantAllowed
        let left = facts?.left ?? 0
        guard force || sent == nil || sent?.allowed != want || sent?.left != left else { return }
        sent = (want, left)
        out.append(.allowed(want, reason: want ? nil : "app"))
    }

    private mutating func mac(_ out: inout [KeeperEffect]) {
        guard platform == .mac, let facts else { return }
        if facts.left > 0 {
            if !activity { activity = true; out.append(.beginActivity) }
            dockShown = true
            out.append(.dock(facts.fraction ?? 0))
        } else {
            if activity { activity = false; out.append(.endActivity) }
            if dockShown { dockShown = false; out.append(.dock(nil)) }
        }
    }

    /// "Downloading Severance", "3 of 10 · 45%": items done in this batch, then the ones left, with the
    /// taps the core has not counted yet.
    private func progress() -> KeeperProgress {
        let now = facts ?? KeeperFacts(left: 0, bytesDone: 0, bytesTotal: 0, title: nil, done: [])
        let finished = now.done.subtracting(doneBefore).count
        let left = max(now.left, 0) + taps
        let total = max(1, finished + left)
        let fraction = now.fraction
        let subtitle = [total > 1 ? "\(min(finished + 1, total)) of \(total)" : nil, fraction.map { "\(Int(($0 * 100).rounded()))%" }]
            .compactMap { $0 }.joined(separator: " \u{B7} ")
        let running = Int64(((fraction ?? 0) * Double(max(now.left, 0)) * 100).rounded())
        return KeeperProgress(
            title: now.title.map { "Downloading \($0)" } ?? "Downloading", subtitle: subtitle,
            completed: Int64(finished) * 100 + running, total: Int64(total) * 100)
    }
}
