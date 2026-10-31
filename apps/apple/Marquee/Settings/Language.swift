import Foundation

/// The languages offered as chips: ISO 639-1 for audio, the 639-2 codes
/// subtitle addons use, and the name in the user's language.
struct Language {
    let audio: String
    let subtitle: String

    var name: String { Locale.current.localizedString(forLanguageCode: audio)?.capitalized ?? audio }

    /// The eight common languages, then any other the profile chose from the
    /// full list, so each chosen one shows and can be turned off.
    static func items(chosen: [String], subtitle: Bool) -> [(String, String)] {
        let common = all.map { (subtitle ? $0.subtitle : $0.audio, $0.name) }
        let extra = chosen.filter { code in !common.contains { $0.0 == code } }
        return common + extra.map { ($0, Locale.current.localizedString(forIdentifier: $0)?.capitalized ?? $0) }
    }

    /// A stored subtitle language as the core's tag: the older chips' codes
    /// ("eng", "tur") read as the core reads them ("en", "tr").
    static func tag(_ code: String) -> String {
        all.first { $0.subtitle == code.lowercased() }?.audio ?? code
    }

    /// The profile's subtitle languages as tags, each once, in order.
    static func ordered(_ stored: [String]) -> [String] {
        var seen = Set<String>()
        return stored.map(tag).filter { seen.insert($0).inserted }
    }

    /// The subtitle chips: the chosen languages in order, then the other common ones.
    static func subtitleChips(_ stored: [String]) -> [String] {
        let chosen = ordered(stored)
        return chosen + all.map(\.audio).filter { !chosen.contains($0) }
    }

    /// Adds a language at the end, or takes it out in every spelling: the
    /// list written back holds each language once, as its tag.
    static func toggled(_ stored: [String], _ tag: String) -> [String] {
        let chosen = ordered(stored)
        return chosen.contains(tag) ? chosen.filter { $0 != tag } : chosen + [tag]
    }

    /// A tag's name in the user's language.
    static func name(_ tag: String) -> String {
        all.first { $0.audio == tag }?.name ?? Locale.current.localizedString(forIdentifier: tag)?.capitalized ?? tag
    }

    /// Settings' Second line (plans/dual-subtitles.md 5.5): a language in the first subtitle language's primary
    /// language (`first`, as the profile stores it, "eng" or "pt-BR") is the first line's and cannot be picked there.
    static func onFirstLine(_ code: String, first: String?) -> Bool {
        guard let first else { return false }
        let primary = { (c: String) in tag(c).split(separator: "-").first.map { $0.lowercased() } ?? c }
        return primary(code) == primary(first)
    }

    /// What Settings' Second line stores for its "Spoken language" row (`Prefs.secondSubtitle`).
    static let spoken = "audio"

    /// Settings' line under the chips: the language subtitles start in.
    static func firstLine(_ stored: [String]) -> String {
        ordered(stored).first.map { "\(name($0)) first" } ?? "None, subtitles start off"
    }

    static let all = [
        Language(audio: "en", subtitle: "eng"), Language(audio: "tr", subtitle: "tur"),
        Language(audio: "es", subtitle: "spa"), Language(audio: "fr", subtitle: "fre"),
        Language(audio: "de", subtitle: "ger"), Language(audio: "it", subtitle: "ita"),
        Language(audio: "pt", subtitle: "por"), Language(audio: "ja", subtitle: "jpn"),
    ]
}
