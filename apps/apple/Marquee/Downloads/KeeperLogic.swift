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

extension DownloadsSnapshot {
    /// This profile's downloads that move, Library's count: `active` counts the device's, which a kids profile
    /// does not list (review r3 N4).
    var moving: Int { downloading.count(where: { $0.state == .running || $0.state == .queued }) }
}

/// What the keeper reads from a `downloads` snapshot.
struct KeeperFacts: Equatable, Sendable {
    /// Running, queued and waiting: a download waiting for Wi-Fi is still left to do, so the keeper holds on.
    var left: Int
    var bytesDone: Int64
    var bytesTotal: Int64
    var title: String?
    /// Every done download's id: the batch counts those done since it began.
    var done: Set<String>
    /// Running and queued alone: what moves. The Dock bar, the Quit question and the system item's
    /// "Downloading" follow it, so a download waiting for Wi-Fi or room never reads as in progress (review r2).
    var running = 0
    /// A locked session's counts-only snapshot: it lists nothing done, so its `done` says nothing.
    var locked = false
    /// The titles the done set was read from (each title's id and row count), so an unchanged list is not
    /// read again, and a list of other titles of the same size (another profile's) is (review r3 N7).
    var titles = 0

    init(left: Int, bytesDone: Int64, bytesTotal: Int64, title: String?, done: Set<String>, running: Int? = nil, locked: Bool = false, titles: Int = 0) {
        (self.left, self.bytesDone, self.bytesTotal, self.title, self.done) = (left, bytesDone, bytesTotal, title, done)
        self.running = running ?? left
        self.locked = locked
        self.titles = titles
    }

    /// The titles' ids and row counts, hashed: cheap next to reading every row.
    static func titlesKey(_ snapshot: DownloadsSnapshot) -> Int {
        var hasher = Hasher()
        for title in snapshot.titles {
            hasher.combine(title.itemId)
            hasher.combine(title.rows.count)
        }
        return hasher.finalize()
    }

    /// `previous`: the facts read last, whose done set is kept when the titles and their done count did not
    /// change (review r2 m-r2-4).
    static func of(_ snapshot: DownloadsSnapshot, previous: KeeperFacts? = nil) -> KeeperFacts {
        let active = snapshot.active
        let key = titlesKey(snapshot)
        let doneCount = snapshot.titles.reduce(0) { $0 + $1.rows.reduce(0) { $0 + ($1.state == .done ? 1 : 0) } }
        let done = if let previous, previous.titles == key, previous.done.count == doneCount {
            previous.done
        } else {
            Set(snapshot.titles.flatMap(\.rows).filter { $0.state == .done }.map(\.id))
        }
        return KeeperFacts(
            left: active.running + active.queued + (active.waiting ?? 0), bytesDone: active.bytesDone, bytesTotal: active.bytesTotal,
            title: active.title, done: done, running: active.running + active.queued, locked: snapshot.locked == true,
            titles: key)
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
    /// A tap's op answered, a second on so the board's snapshot lands first: once every tap is answered, a
    /// tap the core never counted (nothing to download, an error) lets the task go (review r3 N2).
    case answered
    /// A minute after a tap with no answer (a core that went away): the task goes if nothing is left.
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
    /// The fraction the Dock tile shows, nil with no bar.
    private var dockShown: Double?
    /// What was last sent, so `downloadsAllowed` goes on every change.
    private var sent: (allowed: Bool, left: Int)?
    /// Done ids when the batch began, and taps the core has not counted yet.
    private var doneBefore: Set<String> = []
    private var taps = 0
    /// Taps whose op the core has not answered yet: the task waits for them, however slow the add is.
    private var asking = 0

    init(platform: KeeperPlatform) {
        self.platform = platform
    }

    /// Whether `downloadsAllowed` carries `freeBytes`: only iOS, where `volumeAvailableCapacityForImportantUsage`
    /// counts what the system purges for the app. A Mac never sends it: purgeable space there (local snapshots,
    /// caches) is not purged in time, and the core plans with the disk's own figure (plans/offline-downloads.md 4.9,
    /// review r1 M3).
    static func reportsFreeBytes(_ platform: KeeperPlatform) -> Bool { platform != .mac }

    /// A window went to the background: the app left only when no other window is in front (an iPad shows
    /// several), so a second window going away never holds the downloads of the one in use.
    static func appLeft(windowsInFront: Int) -> Bool { windowsInFront == 0 }

    /// The continued task's one identifier, under the `<bundle>.downloads.*` wildcard Info.plist permits:
    /// registered once, every submit reuses it.
    static func taskIdentifier(bundle: String) -> String { "\(bundle).downloads.queue" }

    /// Whether quitting asks first (macOS): "Downloads are in progress."
    /// Only while something moves: a download waiting for Wi-Fi or room loses nothing on Quit (review r2).
    static func asksBeforeQuit(_ facts: KeeperFacts?) -> Bool { (facts?.running ?? 0) > 0 }

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
        case .facts(var now):
            let before = facts
            // A locked session's snapshot lists nothing done: the last unlocked set stays, so the batch's
            // count never jumps and nothing reads as newly finished after an unlock (review r2).
            if now.locked, let before { now.done = before.done }
            facts = now
            // A snapshot already on its way when the user tapped still says nothing runs: the tap stays
            // counted, and the task stays, until the core counts it (or `settle`) (review r1 m1).
            if now.left > 0 { taps = 0 }
            if now.left > 0, (before?.left ?? 0) == 0, !task { doneBefore = before?.done ?? now.done }
            if task {
                // A tap still waiting for its answer holds the task too (review r4 N11).
                if now.left == 0, taps == 0, asking == 0 {
                    finish(&out)
                } else {
                    out.append(.progress(progress()))
                }
            }
            mac(&out)
        case .userAction:
            asking += 1
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
            // A refused or ended task forgets its taps: the next one counts from none (review r3 N8).
            taps = 0
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
        case .answered:
            asking = max(0, asking - 1)
            if asking == 0 { settle(&out) }
        case .settle:
            asking = 0
            settle(&out)
        }
        allowed(&out, force: force)
        return out
    }

    private mutating func settle(_ out: inout [KeeperEffect]) {
        if task, (facts?.left ?? 0) == 0 {
            taps = 0
            finish(&out)
        }
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
        // The activity follows what is left to do; the Dock bar what moves (review r2 m-r2-2).
        if facts.left > 0, !activity { activity = true; out.append(.beginActivity) }
        if facts.left == 0, activity { activity = false; out.append(.endActivity) }
        if facts.running > 0 {
            // The tile redraws for a move of 1 percent or more, not for every snapshot (review r1 m9).
            let fraction = facts.fraction ?? 0
            if dockShown.map({ abs($0 - fraction) >= 0.01 }) ?? true {
                dockShown = fraction
                out.append(.dock(fraction))
            }
        } else if dockShown != nil {
            dockShown = nil
            out.append(.dock(nil))
        }
    }

    /// The batch is over: the item says so, whole, before the task ends, so the system never keeps its last
    /// "7 of 7 · 99%" (the iphone-a walk, 2026-10-28).
    private mutating func finish(_ out: inout [KeeperEffect]) {
        task = false
        let last = progress()
        if last.completed > 0 { out.append(.progress(last)) }
        out.append(.end(success: true))
    }

    /// "Downloading Severance", "3 of 10 · 45%": items done in this batch, then the ones left, with the
    /// taps the core has not counted yet; "Downloads finished" once nothing is left.
    private func progress() -> KeeperProgress {
        let now = facts ?? KeeperFacts(left: 0, bytesDone: 0, bytesTotal: 0, title: nil, done: [])
        let finished = now.done.subtracting(doneBefore).count
        let left = max(now.left, 0) + taps
        let total = max(1, finished + left)
        let fraction = now.fraction
        let subtitle = [total > 1 ? "\(min(finished + 1, total)) of \(total)" : nil, fraction.map { "\(Int(($0 * 100).rounded()))%" }]
            .compactMap { $0 }.joined(separator: " \u{B7} ")
        let running = Int64(((fraction ?? 0) * Double(max(now.left, 0)) * 100).rounded())
        // Nothing moves (all waiting for Wi-Fi, a connection, room) and no tap is on its way: never "Downloading".
        let title = left == 0 ? DownloadCopy.finished
            : now.running == 0 && taps == 0 ? "Waiting" : now.title.map { "Downloading \($0)" } ?? "Downloading"
        return KeeperProgress(
            title: title, subtitle: subtitle,
            completed: Int64(finished) * 100 + running, total: Int64(total) * 100)
    }
}
