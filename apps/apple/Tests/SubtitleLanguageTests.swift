import Foundation
import Testing

// The first run's subtitles step and Settings' Subtitle language row
// (plans/m13-subtitle-languages.md 7).

private let known = [
    LanguageChoice(code: "de", name: "German"), LanguageChoice(code: "en", name: "English"),
    LanguageChoice(code: "pl", name: "Polish"), LanguageChoice(code: "pt-BR", name: "Portuguese (Brazil)"),
    LanguageChoice(code: "tr", name: "Turkish"),
]

@Test func theStepListsTheDevicesLanguageFirstThenTheCommonOnes() {
    let common = SubtitleStepRules.common(known, device: "tr")
    #expect(common.map(\.code) == ["tr", "en", "es", "fr", "de", "it", "pt", "ja"])
    #expect(common.first?.name == "Turkish")
    // The rest in the core's order, none listed twice.
    #expect(SubtitleStepRules.rest(known, device: "tr").map(\.code) == ["pl", "pt-BR"])
    // A language off the common ones leads when it is the device's.
    #expect(SubtitleStepRules.common(known, device: "pl").map(\.code).prefix(2) == ["pl", "en"])
    #expect(SubtitleStepRules.common(known, device: nil).map(\.code).first == "en")
}

@Test func continueWritesTheChoiceAndNoSubtitlesWritesNone() {
    #expect(SubtitleStepRules.patch("tr")["subtitleLanguages"] as? [String] == ["tr"])
    #expect(SubtitleStepRules.patch(nil)["subtitleLanguages"] as? [String] == [])
    // A profile with languages already passes the step.
    #expect(SubtitleStepRules.asks(nil))
    #expect(SubtitleStepRules.asks([]))
    #expect(!SubtitleStepRules.asks(["tr"]))
}

@Test func settingsReadsEverySpellingAsOneLanguageInOrder() {
    // "tr" from a pick while watching, "eng" from an older chip.
    let stored = ["tr", "eng", "tur"]
    #expect(Language.ordered(stored) == ["tr", "en"])
    #expect(Language.subtitleChips(stored) == ["tr", "en", "es", "fr", "de", "it", "pt", "ja"])
    #expect(Language.subtitleChips(["pt-BR"]).first == "pt-BR")
    // Off takes out every spelling; on adds the tag at the end.
    #expect(Language.toggled(stored, "tr") == ["eng"])
    #expect(Language.toggled(["eng"], "de") == ["eng", "de"])
    #expect(Language.firstLine(stored) == "\(Language.name("tr")) first")
    #expect(Language.firstLine([]) == "None, subtitles start off")
}
