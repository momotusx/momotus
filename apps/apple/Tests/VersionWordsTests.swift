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

@Test func thePickHasItsFactsInTheWebsOrder() {
    let bare = row()
    #expect(VersionWords.facts(bare).map(\.key) == ["Size", "Status"])
    #expect(VersionWords.facts(bare)[0].value == "Unknown")
    let full = row(features: ["HDR10", "Atmos"], size: 1_500_000_000, languages: ["eng", "tur"])
    #expect(VersionWords.facts(full).map(\.key) == ["Size", "Status", "Features", "Languages"])
    #expect(VersionWords.facts(full)[3].value == "English, Turkish")
    // One feature is in the heading, not repeated as a fact.
    #expect(VersionWords.facts(row(features: ["HDR10"])).map(\.key) == ["Size", "Status"])
    #expect(VersionWords.pickHeading(row(quality: "4K", features: ["Dolby Vision", "Atmos"])) == "4K Dolby Vision")
}

@Test func reasonsAndFailoverSayWhatTheWebSays() {
    // Never the service's name.
    #expect(VersionWords.reason("readyNow", status: .ready(service: "Real-Debrid")) == "Ready now")
    #expect(VersionWords.reason("readyNow", status: .direct) == "Ready now")
    #expect(VersionWords.reason("smallerFaster") == "Smaller and faster to start")
    #expect(VersionWords.failover(row()) == "One source for this release.")
    #expect(VersionWords.failover(row(failover: 1)) == "2 sources. If one fails, the next starts on its own.")
    #expect(VersionWords.failover(row(failover: 2)) == "3 sources. If one fails, the next starts on its own.")
    #expect(VersionWords.status(.needsDownload(service: "TorBox")) == "Needs download")
    #expect(VersionWords.status(.ready(service: "TorBox")) == "Ready now")
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

@Test func oneSeedIsSingular() {
    #expect(VersionWords.status(.torrent(seeds: 1)) == "Torrent, 1 seed")
    #expect(VersionWords.status(.torrent(seeds: 153)) == "Torrent, 153 seeds")
    #expect(VersionWords.status(.torrent(seeds: nil)) == "Torrent")
}

@Test func wideLayoutsNeedTheRoomTheyWereMadeFor() {
    // iPad Air in portrait (820 pt, 40 pt gutters): the pick above the table, one column per row.
    let ipadPortrait: CGFloat = 820 - 80
    #expect(!WideLayout.pickBesideTable(page: ipadPortrait))
    #expect(WideLayout.tableColumns(table: ipadPortrait))
    #expect(!WideLayout.creditsBesideEpisodes(page: ipadPortrait))
    // iPad mini in portrait (744 pt): rows read in two lines.
    #expect(!WideLayout.tableColumns(table: 744 - 80))
    // iPad Air in landscape (1180 pt): side by side, columns, credits beside the episodes.
    let ipadLandscape: CGFloat = 1180 - 80
    #expect(WideLayout.pickBesideTable(page: ipadLandscape))
    #expect(WideLayout.tableColumns(table: ipadLandscape - 360 - 40))
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
