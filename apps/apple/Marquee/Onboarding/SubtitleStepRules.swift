import Foundation

/// The device's language for first run's "Subtitles start in" chips (plans/first-screens.md 3.5, D11;
/// `FirstRunRules.chips` orders them).
enum SubtitleStepRules {
    /// The device's language as the core lists it, the one it marks ("nb"
    /// reads as Norwegian, "no"): a code it does not list would be a
    /// preference no track ever matches.
    static func listed(_ all: [LanguageChoice]) -> String? {
        all.first { $0.device == true }?.code
    }
}
