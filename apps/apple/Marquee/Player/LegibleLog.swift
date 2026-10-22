import Foundation

/// The item's legible text as AVPlayerItemLegibleOutput delivers it (plans/apple-file-subtitle-timing.md): facts keyed by
/// item time, each "from here this shows" (an empty text: nothing). What shows at a time is the last entry at or before
/// it, so a timing either way reads the same log: an earlier one in what arrived ahead (up to the output's advance over
/// HTTP), a later one in what came before.
struct LegibleLog {
    private var entries: [(time: Double, text: String)] = []
    /// The landing of the last seek, written as an empty entry until a delivery covers it.
    private var marker: Double?

    /// A delivery. Two cues at one time come as two deliveries, so a second text at an existing time is added as a line;
    /// a redelivery of a line already there adds nothing.
    mutating func deliver(_ text: String, at time: Double) {
        guard time.isFinite else { return }
        // A delivery at or before a seek's landing (MP4: at it; HLS: the cue's own start) tells what shows there: the
        // marker goes. Deliveries after the landing leave it: until one comes, nothing showed at the landing.
        if let landing = marker, time <= landing {
            marker = nil
            entries.removeAll { $0.time == landing && $0.text.isEmpty }
        }
        let at = entries.firstIndex { $0.time >= time } ?? entries.endIndex
        guard at < entries.endIndex, entries[at].time == time else {
            entries.insert((time, text), at: at)
            return
        }
        let old = entries[at].text
        if old.isEmpty || text.isEmpty {
            entries[at].text = text
        } else if !old.split(separator: "\n").contains(Substring(text)) {
            entries[at].text = old + "\n" + text
        }
    }

    /// What shows at `time`: nil for nothing.
    func text(at time: Double) -> String? {
        guard let last = entries.last(where: { $0.time <= time }), !last.text.isEmpty else { return nil }
        return last.text
    }

    /// A seek: nothing delivered before tells what shows at the landing (a cue whose end was never delivered would
    /// show through the whole jump), so the landing reads empty until the output says otherwise.
    mutating func seek(to landing: Double) {
        guard landing.isFinite else { return }
        entries.removeAll { $0.time == landing && $0.text.isEmpty }
        deliver("", at: landing)
        marker = landing
    }

    /// A new item or another track: nothing delivered before holds.
    mutating func reset() {
        entries = []
        marker = nil
    }

    /// The times the shown text changes, shifted by `delay`, from `from` and up to `ahead` seconds later: for the
    /// boundary observer, which takes finite times only.
    func edges(delay: Double, from: Double, ahead: Double) -> [Double] {
        entries.map { $0.time + delay }.filter { $0.isFinite && $0 >= max(0, from) && $0 <= from + ahead }
    }
}
