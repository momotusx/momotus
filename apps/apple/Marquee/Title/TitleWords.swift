import Foundation

/// "21:00" or "1:02:03".
func clock(ms: Int64) -> String {
    let total = Int(ms / 1000)
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60)
    return h > 0 ? String(format: "%d:%02d:%02d", h, m, s) : String(format: "%d:%02d", m, s)
}

/// Minutes from a runtime like "57 min", "2h 9min" or "1h".
func minutes(_ runtime: String?) -> Int? {
    guard let runtime else { return nil }
    let hours = runtime.firstMatch(of: /(\d+)\s*h/).flatMap { Int($0.1) } ?? 0
    let mins = runtime.firstMatch(of: /(\d+)\s*m/).flatMap { Int($0.1) } ?? 0
    let total = hours * 60 + mins
    return total > 0 ? total : nil
}

/// A core date ("2013-08-11") as a date at the start of that day, UTC.
func releaseDate(_ value: String?) -> Date? {
    guard let value else { return nil }
    return try? Date(value, strategy: .iso8601.year().month().day())
}

/// A calendar day's heading: "Today", "Tomorrow", "Yesterday", else
/// "Wednesday, October 21". Both are core days at UTC midnight, so they are
/// counted and named in UTC, never in the viewer's zone (`viewer`, there so tests can
/// be a viewer anywhere): read there, a day west of UTC was the day before,
/// and the night the clocks change counted as no day at all.
func dayHeading(_ day: String, today: String, viewer: TimeZone = .current, locale: Locale = .current) -> String {
    guard let date = releaseDate(day), let now = releaseDate(today) else { return day }
    let utc = TimeZone(identifier: "UTC") ?? TimeZone(secondsFromGMT: 0) ?? viewer
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = utc
    switch calendar.dateComponents([.day], from: now, to: date).day {
    case 0: return "Today"
    case 1: return "Tomorrow"
    case -1: return "Yesterday"
    default:
        var style = Date.FormatStyle.dateTime.weekday(.wide).day().month(.wide)
        style.timeZone = utc
        style.locale = locale
        return date.formatted(style)
    }
}

/// A long text in blocks of whole sentences of about `size` characters,
/// for a TV reader whose blocks take focus one after another (a remote
/// scrolls a page only by moving focus). A sentence longer than `size`
/// is a block of its own; nothing is cut or dropped.
func readingBlocks(_ text: String, size: Int = 240) -> [String] {
    var sentences: [String] = []
    text.enumerateSubstrings(in: text.startIndex..., options: .bySentences) { sentence, _, _, _ in
        if let sentence = sentence?.trimmingCharacters(in: .whitespacesAndNewlines), !sentence.isEmpty {
            sentences.append(sentence)
        }
    }
    // A sentence longer than a block goes in pieces of whole words, so no
    // block outgrows the screen.
    let pieces = sentences.flatMap { sentence in
        sentence.count <= size ? [sentence] : join(sentence.split(separator: " ").map(String.init), size: size)
    }
    return join(pieces, size: size)
}

/// Parts joined with spaces into runs of at most `size` characters; a part
/// longer than that stands alone.
private func join(_ parts: [String], size: Int) -> [String] {
    var runs: [String] = []
    for part in parts {
        if let last = runs.last, last.count + 1 + part.count <= size {
            runs[runs.count - 1] = last + " " + part
        } else {
            runs.append(part)
        }
    }
    return runs
}

/// What Resume says after its label: "36 min left" from what the file has
/// left (Continue Watching's figure), else from the runtime when it is known,
/// else the position ("from 21:00").
func resumeText(resumeMs: Int64, leftMs: Int64? = nil, runtime: String?) -> String {
    if let leftMs, leftMs > 0 { return minutesLeft(ms: leftMs) }
    guard let total = minutes(runtime) else { return "from \(clock(ms: resumeMs))" }
    let left = Int((Double(total) - Double(resumeMs) / 60_000).rounded())
    return "\(max(1, left)) min left"
}

/// Whole minutes rounded up, as the core words Continue Watching.
func minutesLeft(ms: Int64) -> String {
    "\(max(1, (ms + 59_999) / 60_000)) min left"
}
