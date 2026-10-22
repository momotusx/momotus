import Foundation

/// A first run's subtitles step, after the stream question
/// (plans/m13-subtitle-languages.md 7): the language subtitles start in, one
/// choice. The device's language is listed first and chosen at first, unless
/// it is English (No subtitles then: most titles are in English, and English
/// audio a file does not tag would get English subtitles over it); then the
/// common ones and No subtitles; every other language waits behind More
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
    static let listFailed = "Could not load the languages"

    /// The device's language as the core lists it, the one it marks ("nb"
    /// reads as Norwegian, "no"): a code it does not list would be a
    /// preference no track ever matches.
    static func listed(_ all: [LanguageChoice]) -> String? {
        all.first { $0.device == true }?.code
    }

    /// The device's language (`listed`), then the common ones, each once,
    /// named as the core names them (by the system before its list is in).
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

    /// The row chosen at first: the device's language, No subtitles (nil)
    /// on an English device.
    static func initial(_ device: String?) -> String? {
        device == "en" ? nil : device
    }

    /// The choice once the core's list is in: one the viewer made before it
    /// stays (No subtitles too), else the device's language as the core
    /// lists it (`initial`).
    static func loaded(_ chosen: String?, touched: Bool, all: [LanguageChoice]) -> String? {
        touched ? chosen : initial(listed(all))
    }

    /// Where a first run goes after the question, and after this step.
    enum Next: Equatable { case subtitles, check, home }

    /// This step, unless the profile has languages; then on as after it.
    static func afterQuestion(_ languages: [String]?, thenHome: Bool) -> Next {
        asks(languages) ? .subtitles : afterSubtitles(thenHome: thenHome)
    }

    /// Browse first goes Home; Start fresh and an import go to the setup check.
    static func afterSubtitles(thenHome: Bool) -> Next {
        thenHome ? .home : .check
    }
}
