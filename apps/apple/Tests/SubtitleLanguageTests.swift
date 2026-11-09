import Foundation
import Testing

// First run's "Subtitles start in" chips (plans/first-screens.md 3.5, D11) and Settings' Subtitle language row
// (plans/m13-subtitle-languages.md 7).

private let known = [
    LanguageChoice(code: "de", name: "German"), LanguageChoice(code: "en", name: "English"),
    LanguageChoice(code: "pl", name: "Polish"), LanguageChoice(code: "pt-BR", name: "Portuguese (Brazil)"),
    LanguageChoice(code: "tr", name: "Turkish"),
]

@Test func theDevicesLanguageIsTheOneTheCoreMarks() {
    // Norway's Bokmål: the core lists Norwegian as "no" and marks it, and the setup's first chip is that.
    let norwegian = known + [LanguageChoice(code: "no", name: "Norwegian", device: true)]
    #expect(SubtitleStepRules.listed(norwegian) == "no")
    #expect(FirstRunRules.chips(device: SubtitleStepRules.listed(norwegian), chosen: []).chips == ["no", "en", "none", "more"])
    // A code the core does not list counts as absent: English is chosen (D11), never No subtitles.
    #expect(SubtitleStepRules.listed(known) == nil)
    #expect(FirstRunRules.chips(device: SubtitleStepRules.listed(known), chosen: []).selected == "en")
}

@Test func startWatchingWritesTheShownChipAndBestQualityOnlyWhenChosen() throws {
    #expect(FirstRunRules.setupPatch(chip: "tr", best: nil)["subtitleLanguages"] as? [String] == ["tr"])
    // No subtitles is written too, as none.
    #expect(FirstRunRules.setupPatch(chip: FirstRunRules.none, best: nil)["subtitleLanguages"] as? [String] == [])
    #expect(FirstRunRules.setupPatch(chip: "en", best: nil)["streams"] == nil, "Start fast writes no stream patch")
    let best = StreamChoice.patch(StreamChoice.words.question.best)
    let streams = try #require(FirstRunRules.setupPatch(chip: "en", best: best)["streams"] as? [String: any Sendable])
    #expect(streams["minSeeders"] as? Int == StreamChoice.words.question.best.patch.minSeeders)
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

@Test func theSecondLineCannotTakeTheFirstSubtitleLanguageInAnySpelling() {
    #expect(Language.onFirstLine("en", first: "eng"))
    #expect(Language.onFirstLine("en", first: "en"))
    #expect(Language.onFirstLine("pt", first: "pt-BR"))
    #expect(Language.onFirstLine("pt-BR", first: "por"))
    #expect(!Language.onFirstLine("tr", first: "eng"))
    #expect(!Language.onFirstLine("en", first: nil))
}
