import Foundation
import Testing

// The Versions words match the web's lib/words.ts and Android's VersionsCopy.

private func row(
    quality: String = "1080p", features: [String] = [], size: Int64? = nil, status: VersionStatus = .direct,
    languages: [String] = [], reasons: [String] = [], failover: Int = 0
) -> VersionRow {
    VersionRow(
        key: "k", quality: quality, features: features, sizeBytes: size, status: status, via: "Torrentio",
        releaseName: "Name.mp4", languages: languages, reasons: reasons, group: "best", failoverCount: failover,
        line: nil, sourceClass: nil, bitrateMbps: nil)
}

@Test func languageCodesBecomeEnglishNames() {
    #expect(VersionWords.languageName("eng") == "English")
    #expect(VersionWords.languageName("fr") == "French")
    #expect(VersionWords.languageName("qqq") == "QQQ")
}

@Test func anOpenedRowSaysItsLanguagesAndSources() {
    #expect(VersionWords.detail(row(languages: ["en"], failover: 1)) == "English, 2 sources")
    #expect(VersionWords.detail(row(languages: ["en", "fr"])) == "English, French")
    #expect(VersionWords.detail(row(failover: 2)) == "3 sources")
    #expect(VersionWords.detail(row()) == "Language not stated")
}

@Test func wideLayoutsNeedTheRoomTheyWereMadeFor() {
    // iPad Air in portrait (820 pt, 40 pt gutters): the pick above the table, one column per row.
    let ipadPortrait: CGFloat = 820 - 80
    #expect(!WideLayout.pickBesideTable(page: ipadPortrait))
    #expect(!WideLayout.creditsBesideEpisodes(page: ipadPortrait))
    // iPad Air in landscape (1180 pt): side by side, columns, credits beside the episodes.
    let ipadLandscape: CGFloat = 1180 - 80
    #expect(WideLayout.pickBesideTable(page: ipadLandscape))
    #expect(WideLayout.creditsBesideEpisodes(page: ipadLandscape))
}

@Test func theWideTitleStacksOnAShortWindow() {
    // iPad and Mac windows: the words over the art.
    #expect(WideLayout.titleOverArt(wide: true, window: 820))
    #expect(WideLayout.titleOverArt(wide: true, window: 500))
    // A Pro Max in landscape (440 pt): stacked under the art, as a phone.
    #expect(!WideLayout.titleOverArt(wide: true, window: 440))
    // Not measured yet: the wide layout; a phone always stacks.
    #expect(WideLayout.titleOverArt(wide: true, window: 0))
    #expect(!WideLayout.titleOverArt(wide: false, window: 900))
}

@Test func theMacTitleHeroIsSixteenByNineHeldToTheWindowNeverUnder400() {
    #expect(WideLayout.titleHeroHeight(width: 1440, height: 900) == 810)
    // A short window keeps the first shelf's title on screen, down to the 400 floor.
    #expect(WideLayout.titleHeroHeight(width: 1440, height: 700) == 610)
    #expect(WideLayout.titleHeroHeight(width: 720, height: 400) == 400)
    #expect(WideLayout.titleHeroHeight(width: 0, height: 0) == 506)
}

@Test func starringShowsOnlyWhereItClearsTheActionRow() {
    // A short row: from 1240, where the glass check reads Starring.
    #expect(!WideLayout.starringFits(window: 1239, row: 500))
    #expect(WideLayout.starringFits(window: 1240, row: 500))
    #expect(!WideLayout.starringFits(window: 720, row: 500))
    // A movie's 875 pt row needs 1355 (review P2-1); an unmeasured row hides it.
    #expect(!WideLayout.starringFits(window: 1354, row: 875))
    #expect(WideLayout.starringFits(window: 1355, row: 875))
    #expect(!WideLayout.starringFits(window: 1440, row: .infinity))
}

@Test func thePanelCoversTheInfoOnlyBelowAbout1020() {
    // Beside the open panel the column narrows to the window less 540; under 480 the panel covers it.
    #expect(WideLayout.panelCoversInfo(window: 1000))
    #expect(!WideLayout.panelCoversInfo(window: 1100))
    #expect(WideLayout.panelColumnWidth(window: 1100) == 560)
    #expect(!WideLayout.panelCoversInfo(window: 1440))
    #expect(WideLayout.panelColumnWidth(window: 1440) == 900)
}

@Test func theMacTitleHeroGivesTheVersionNoteItsRoom() {
    // A two-line note with its gap (52 pt) comes off the hero, so Season N stays on the first screen.
    #expect(WideLayout.titleHeroHeight(width: 1440, height: 863, note: 52) == 721)
    #expect(WideLayout.titleHeroHeight(width: 1440, height: 863, note: 0) == 773)
    // Never under the 400 floor.
    #expect(WideLayout.titleHeroHeight(width: 720, height: 500, note: 52) == 400)
}
