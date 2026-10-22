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

@Test func anEnglishDeviceStartsOnNoSubtitles() {
    #expect(SubtitleStepRules.initial("en") == nil)
    #expect(SubtitleStepRules.initial("tr") == "tr")
    #expect(SubtitleStepRules.initial(nil) == nil)
    // English still leads the list on an English device.
    #expect(SubtitleStepRules.common(known, device: "en").first?.code == "en")
}

@Test func theDevicesLanguageIsTheOneTheCoreMarks() {
    // Norway's Bokmål: the core lists Norwegian as "no" and marks it.
    let norwegian = known + [LanguageChoice(code: "no", name: "Norwegian", device: true)]
    #expect(SubtitleStepRules.listed(norwegian) == "no")
    #expect(SubtitleStepRules.loaded(nil, all: norwegian) == "no")
    #expect(SubtitleStepRules.common(norwegian, device: SubtitleStepRules.listed(norwegian)).first?.name == "Norwegian")
    // A code the core does not list: nothing marked, No subtitles.
    #expect(SubtitleStepRules.listed(known) == nil)
    #expect(SubtitleStepRules.loaded(nil, all: known) == nil)
    // English stays on No subtitles; a choice made stays.
    let english = [LanguageChoice(code: "en", name: "English", device: true)]
    #expect(SubtitleStepRules.loaded(nil, all: english) == nil)
    #expect(SubtitleStepRules.loaded("de", all: norwegian) == "de")
}

@Test func theQuestionLeadsToTheStepUnlessTheProfileHasLanguages() {
    #expect(SubtitleStepRules.afterQuestion([], thenHome: false) == .subtitles)
    #expect(SubtitleStepRules.afterQuestion(nil, thenHome: true) == .subtitles)
    #expect(SubtitleStepRules.afterQuestion(["tr"], thenHome: false) == .check)
    #expect(SubtitleStepRules.afterQuestion(["tr"], thenHome: true) == .home)
    // Skip and Continue go on alike.
    #expect(SubtitleStepRules.afterSubtitles(thenHome: false) == .check)
    #expect(SubtitleStepRules.afterSubtitles(thenHome: true) == .home)
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
    // Off takes out every spelling; on adds the tag at the end; each
    // language is written once, as its tag.
    #expect(Language.toggled(stored, "tr") == ["en"])
    #expect(Language.toggled(["eng"], "de") == ["en", "de"])
    #expect(Language.toggled(["tr", "tur"], "de") == ["tr", "de"])
    #expect(Language.firstLine(stored) == "\(Language.name("tr")) first")
    #expect(Language.firstLine([]) == "None, subtitles start off")
}
