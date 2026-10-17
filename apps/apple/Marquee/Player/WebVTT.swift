import Foundation

/// Cues from the core's subtitle URLs (always UTF-8 WebVTT). AVPlayer cannot
/// side-load WebVTT for a progressive file, so the player draws cues itself.
enum WebVTT {
    struct Cue: Equatable, Sendable {
        let start: Double
        let end: Double
        let text: String
    }

    /// Parses cues in file order; styling tags are dropped, line breaks kept.
    static func parse(_ text: String) -> [Cue] {
        var cues: [Cue] = []
        let blocks = text.replacingOccurrences(of: "\r\n", with: "\n").components(separatedBy: "\n\n")
        for block in blocks {
            let lines = block.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
            guard let timing = lines.firstIndex(where: { $0.contains("-->") }) else { continue }
            let parts = lines[timing].components(separatedBy: "-->")
            guard parts.count == 2,
                  let start = seconds(parts[0]),
                  let end = seconds(parts[1].split(separator: " ", omittingEmptySubsequences: true).first.map(String.init) ?? "")
            else { continue }
            let body = lines[(timing + 1)...].joined(separator: "\n")
                .replacing(/<[^>]*>/, with: "")
                .replacingOccurrences(of: "&amp;", with: "&")
                .replacingOccurrences(of: "&lt;", with: "<")
                .replacingOccurrences(of: "&gt;", with: ">")
                .trimmingCharacters(in: .whitespacesAndNewlines)
            if !body.isEmpty { cues.append(Cue(start: start, end: end, text: body)) }
        }
        return cues.sorted { $0.start < $1.start }
    }

    /// "01:02:03.456" or "02:03.456" to seconds.
    static func seconds(_ stamp: String) -> Double? {
        let fields = stamp.trimmingCharacters(in: .whitespaces).split(separator: ":")
        guard (2...3).contains(fields.count) else { return nil }
        var total = 0.0
        for field in fields {
            guard let value = Double(field.replacingOccurrences(of: ",", with: ".")) else { return nil }
            total = total * 60 + value
        }
        return total
    }

    /// The text showing at `time` (overlapping cues joined), or nil.
    static func text(at time: Double, in cues: [Cue]) -> String? {
        let showing = cues.filter { $0.start <= time && time < $0.end }.map(\.text)
        return showing.isEmpty ? nil : showing.joined(separator: "\n")
    }
}
