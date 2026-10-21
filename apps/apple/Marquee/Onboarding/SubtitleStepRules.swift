import Foundation

/// A first run's subtitles step, after the stream question
/// (plans/m13-subtitle-languages.md 7): the language subtitles start in, one
/// choice. The device's language is listed first and chosen at first, then
/// the common ones and No subtitles; every other language waits behind More
/// languages. Continue writes the choice as the profile's
/// `subtitleLanguages` (none for No subtitles); Skip writes nothing.
enum SubtitleStepRules {
    static let title = "Subtitles"
    static let lead = "Pick the language subtitles start in. They turn on when a film's audio is in another language. Change it later in Settings, under Playback."
    static let device = "This device's language"
    static let none = "No subtitles"
    static let noneLine = "Turn them on while watching"
    static let more = "More languages"
    static let fewer = "Fewer languages"

    /// The device's language, then the common ones, each once, named as the
    /// core names them (by the system before its list is in).
    static func common(_ all: [LanguageChoice], device: String?) -> [LanguageChoice] {
        var seen = Set<String>()
        let codes = ([device].compactMap { $0 }.filter { !$0.isEmpty } + Language.all.map(\.audio)).filter { seen.insert($0).inserted }
        return codes.map { code in all.first { $0.code == code } ?? LanguageChoice(code: code, name: Language.name(code)) }
    }

    /// Every other language, in the core's order (by name).
    static func rest(_ all: [LanguageChoice], device: String?) -> [LanguageChoice] {
        let shown = Set(common(all, device: device).map(\.code))
        return all.filter { !shown.contains($0.code) }
    }

    /// What Continue writes.
    static func patch(_ chosen: String?) -> [String: any Sendable] {
        ["subtitleLanguages": chosen.map { [$0] } ?? [String]()]
    }

    /// Whether the step shows: not for a profile with languages already.
    static func asks(_ languages: [String]?) -> Bool {
        languages?.isEmpty ?? true
    }
}
