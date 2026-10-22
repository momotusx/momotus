import Foundation

/// The item's legible text as AVPlayerItemLegibleOutput delivers it (plans/apple-file-subtitle-timing.md): facts keyed by
/// item time, each "from here these cues show" (none: nothing). What shows at a time is the last entry at or before it, so
/// a timing either way reads the same log: an earlier one in what arrived ahead (up to the output's advance over HTTP), a
/// later one in what came before.
struct LegibleLog {
    /// Each delivery's strings as they came: one string per cue, a two-line cue being one string with its line break.
    private var entries: [(time: Double, cues: [String])] = []

    /// A delivery. Two cues at one time can come as two deliveries, so cues at an existing time are added; one already
    /// there (a redelivery after a seek) adds nothing. An empty delivery never erases cues at its own time: the same
    /// instant cannot be both a cue and nothing, and the output may tell an abutting cue's start and the last one's end
    /// in either order.
    mutating func deliver(_ cues: [String], at time: Double) {
        guard time.isFinite else { return }
        let cues = cues.filter { !$0.isEmpty }
        let at = entries.firstIndex { $0.time >= time } ?? entries.endIndex
        guard at < entries.endIndex, entries[at].time == time else {
            entries.insert((time, cues), at: at)
            return
        }
        entries[at].cues += cues.filter { !entries[at].cues.contains($0) }
    }

    /// What shows at `time`: nil for nothing.
    func text(at time: Double) -> String? {
        guard let last = entries.last(where: { $0.time <= time }), !last.cues.isEmpty else { return nil }
        return last.cues.joined(separator: "\n")
    }

    /// A seek. Every entry but the last is ended by the next one, so what the log says stays true across a jump; only the
    /// last, if it shows cues, is an open frontier whose end never came: it goes, or a seek past it would show its line
    /// through the jump (at any timing). The output delivers it again if it still applies (MP4 at the landing, HLS at
    /// the cue's own start, both measured).
    mutating func seek() {
        if let last = entries.last, !last.cues.isEmpty { entries.removeLast() }
    }

    /// A new item or another track: nothing delivered before holds.
    mutating func reset() {
        entries = []
    }

    /// The times the shown text changes, shifted by `delay`, from `from` and up to `ahead` seconds later: for the
    /// boundary observer, which takes finite times only.
    func edges(delay: Double, from: Double, ahead: Double) -> [Double] {
        entries.map { $0.time + delay }.filter { $0.isFinite && $0 >= max(0, from) && $0 <= from + ahead }
    }
}
