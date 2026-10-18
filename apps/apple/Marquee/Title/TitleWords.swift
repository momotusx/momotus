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
