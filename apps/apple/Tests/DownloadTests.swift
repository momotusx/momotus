import AppKit
import Foundation
import Testing

// Offline downloads on Apple (plans/offline-downloads.md 3.1 to 3.9, 4.13, 5, 6.3): the control's words, equal to
// Android's DownloadCopyTest and the web's download.test.ts; the keeper's reducer; the network report; the view
// as a real core sends it.

private func badge(_ state: DownloadState, _ reason: DownloadReason? = nil, progress: Double? = nil) -> DownloadBadge {
    DownloadBadge(id: "d1", state: state, reason: reason, mobileData: false, progress: progress, bytesTotal: 2_000_000_000)
}

/// What a control shows, as one value: glyph, label, spoken word, value, ring, items, hint.
private struct Shown: Equatable {
    let glyph: DownloadGlyph
    let label: String
    let spoken: String
    let value: String?
    let ring: Double?
    let items: [DownloadItem]
    let hint: String?

    init(_ glyph: DownloadGlyph, _ label: String, _ spoken: String, _ value: String?, _ ring: Double?, _ items: [DownloadItem], _ hint: String?) {
        (self.glyph, self.label, self.spoken, self.value, self.ring, self.items, self.hint) = (glyph, label, spoken, value, ring, items, hint)
    }

    init(_ c: DownloadControl) {
        self.init(c.glyph, c.label, c.spoken, c.value, c.ring, c.items, c.hint)
    }
}

private let hint = "Shows download options"

@Test func everyStateGivesItsGlyphLabelSpokenWordValueRingAndMenu() {
    #expect(Shown(DownloadCopy.control(nil)) == Shown(.download, "Download", "Download", nil, nil, [], nil))
    #expect(Shown(DownloadCopy.control(badge(.queued))) == Shown(.clock, "Queued", "Queued", nil, 0, [.cancel], hint))
    #expect(Shown(DownloadCopy.control(badge(.running, progress: 0.45))) == Shown(.stop, "45%", "Downloading", "45 percent", 0.45, [.pause, .cancel], hint))
    // No size yet: the word, an empty ring, no value.
    #expect(Shown(DownloadCopy.control(badge(.running))) == Shown(.stop, "Downloading", "Downloading", nil, 0, [.pause, .cancel], hint))
    #expect(Shown(DownloadCopy.control(badge(.paused, progress: 0.3))) == Shown(.pause, "Paused", "Paused", "30 percent", 0.3, [.resume, .cancel], hint))
    #expect(Shown(DownloadCopy.control(badge(.done, progress: 1))) == Shown(.check, "Downloaded", "Downloaded", nil, nil, [.play, .delete], hint))
    #expect(Shown(DownloadCopy.control(badge(.failed, .source))) == Shown(.alert, "Couldn't download", "Couldn't download", nil, nil, [.retry, .another, .cancel], hint))
    #expect(Shown(DownloadCopy.control(badge(.missing))) == Shown(.alert, "Download again", "Download again", nil, nil, [.again, .remove], hint))
    // A state from a newer core can still be cancelled.
    #expect(Shown(DownloadCopy.control(badge(.unknown))) == Shown(.clock, "Waiting", "Waiting", nil, 0, [.cancel], hint))
}

@Test func aWaitingDownloadSaysItsReasonAndOnlyWiFiAndLowDataOfferMobileData() {
    let cases: [(DownloadReason, DownloadGlyph, String, [DownloadItem])] = [
        (.wifi, .wifiOff, "Waiting for Wi-Fi", [.mobileData, .cancel]),
        (.lowData, .wifiOff, "Waiting: Low Data Mode is on", [.mobileData, .cancel]),
        (.network, .wifiOff, "Waiting for a connection", [.cancel]),
        (.playing, .clock, "Paused while you watch", [.cancel]),
        (.app, .clock, "Opens again when you return to Momotus", [.cancel]),
    ]
    for (reason, glyph, label, items) in cases {
        let control = DownloadCopy.control(badge(.waiting, reason, progress: 0.2))
        #expect(control.glyph == glyph && control.label == label && control.items == items, "\(reason)")
        #expect(control.spoken == control.label)
        #expect(control.value == "20 percent")
        #expect(control.hint == hint)
    }
}

// The owner's rule: never a storage warning. A download waiting for room reads "Waiting" with the clock and no
// detail line (no "Not enough space", no "Needs X more").
@Test func aDownloadWaitingForSpaceReadsWaitingWithTheClockAndNoDetail() {
    let control = DownloadCopy.control(badge(.waiting, .space, progress: 0.2))
    #expect(control.label == "Waiting")
    #expect(control.spoken == "Waiting")
    #expect(control.glyph == .clock)
    #expect(control.detail == nil)
    #expect(control.items == [.cancel])
    #expect(DownloadCopy.waiting(.space) == "Waiting")
}

@Test func theReasonInFullSitsOverTheMenu() {
    #expect(DownloadCopy.control(badge(.waiting, .wifi)).detail == "Download on Wi-Fi only is on, and this network uses mobile data.")
    #expect(DownloadCopy.control(badge(.waiting, .network)).detail == "It goes on by itself when the connection comes back.")
    #expect(DownloadCopy.control(badge(.waiting, .playing)).detail == "It goes on by itself when playback ends.")
    #expect(DownloadCopy.control(badge(.missing)).detail == "The downloaded file is no longer on this device.")
    let failures = [DownloadReason.noVersion, .source, .retries, .notVideo].map { DownloadCopy.control(badge(.failed, $0)).detail }
    #expect(failures == [
        "No version of this one can be downloaded", "The source stopped answering",
        "The download was interrupted too many times", "The file wasn't a video",
    ])
}

@Test func eachItemSendsItsOpAndMobileDataReleasesAWaitingOne() {
    func op(_ item: DownloadItem) -> String? { DownloadCopy.op(item).map { "\($0.op.rawValue)\($0.mobileData ? "+mobile" : "")" } }
    #expect(op(.pause) == "downloadPause")
    #expect(op(.resume) == "downloadResume")
    for gone in [DownloadItem.cancel, .delete, .remove] { #expect(op(gone) == "downloadRemove") }
    #expect(op(.retry) == "downloadRetry")
    #expect(op(.again) == "downloadRetry")
    #expect(op(.mobileData) == "downloadRetry+mobile")
    for elsewhere in [DownloadItem.start, .choose, .another, .play] { #expect(op(elsewhere) == nil) }
    #expect(DownloadItem.allCases.map(\.label) == [
        "Download", "Choose a version to download", "Pause download", "Resume download", "Cancel download", "Delete download",
        "Play", "Try again", "Choose another version", "Download now on mobile data", "Download again", "Remove",
    ])
}

@Test func theSeriesMenuCountsAiredEpisodesAndSaysWhereTheSeasonStands() {
    let series = GroupDownload(seasons: 3, episodes: 28, downloaded: 0, active: 0)
    #expect(DownloadCopy.seriesMenu(season: 1, group: GroupDownload(seasons: 1, episodes: 12, downloaded: 0, active: 0), series: series) == [
        GroupItem(label: "Download Season 1 (12 episodes)", act: .downloadSeason),
        GroupItem(label: "Download every season (3 seasons, 28 episodes)", act: .downloadSeries),
    ])
    // Some of the season already kept: the rest, and its delete.
    #expect(DownloadCopy.seriesMenu(season: 1, group: GroupDownload(seasons: 1, episodes: 12, downloaded: 11, active: 0),
                                    series: GroupDownload(seasons: 3, episodes: 28, downloaded: 11, active: 0)) == [
        GroupItem(label: "Download Season 1 (1 episode)", act: .downloadSeason),
        GroupItem(label: "Delete Season 1 downloads", act: .deleteSeason),
        GroupItem(label: "Download every season (3 seasons, 28 episodes)", act: .downloadSeries),
    ])
    #expect(DownloadCopy.seriesMenu(season: 2, group: GroupDownload(seasons: 1, episodes: 8, downloaded: 8, active: 3), series: series).first
        == GroupItem(label: "Downloading Season 2", act: .status))
    #expect(DownloadCopy.seriesMenu(season: 2, group: GroupDownload(seasons: 1, episodes: 8, downloaded: 8, active: 0), series: series).first
        == GroupItem(label: "Season 2 downloaded", act: .status))
    // One season, or everything kept: no "every season".
    let one = GroupDownload(seasons: 1, episodes: 8, downloaded: 0, active: 0)
    #expect(DownloadCopy.seriesMenu(season: 1, group: one, series: one) == [GroupItem(label: "Download Season 1 (8 episodes)", act: .downloadSeason)])
    // Deleting a season asks first, in a series delete's words; nothing else in the menu does (review r1 M5).
    #expect(DownloadCopy.asksFirst(.deleteSeason))
    #expect(!DownloadCopy.asksFirst(.downloadSeason) && !DownloadCopy.asksFirst(.downloadSeries) && !DownloadCopy.asksFirst(.status))
    #expect(DownloadCopy.deleteSeries(8, "Severance") == "Delete 8 downloaded episodes of Severance?")
    // The action-row control always opens the menu.
    #expect(DownloadCopy.seriesControl(nil).label == "Download" && DownloadCopy.seriesControl(nil).hint == hint)
    #expect(DownloadCopy.seriesControl(GroupDownload(seasons: 3, episodes: 28, downloaded: 4, active: 2)).label == "Downloading")
    #expect(DownloadCopy.seriesControl(GroupDownload(seasons: 3, episodes: 28, downloaded: 4, active: 0)).glyph == .check)
}

@Test func theDownloadsScreensLines() {
    let gb: Int64 = 1 << 30
    #expect(DownloadCopy.storage(DownloadStorage(downloadsBytes: Int64(12.4 * Double(gb)), freeBytes: 48 * gb, neededBytes: nil))
        == "Downloads use 12.4 GB \u{B7} 48 GB free on this device")
    #expect(DownloadCopy.storage(DownloadStorage(downloadsBytes: 300 << 20, freeBytes: nil, neededBytes: nil)) == "Downloads use 300 MB")
    // A download waiting for room never turns the line into a warning.
    #expect(DownloadCopy.storage(DownloadStorage(downloadsBytes: 300 << 20, freeBytes: 1 << 30, neededBytes: 2 * gb))
        == "Downloads use 300 MB \u{B7} 1 GB free on this device")
    #expect(DownloadCopy.ofTotal(Int64(1.2 * Double(gb)), Int64(2.1 * Double(gb))) == "1.2 of 2.1 GB")
    #expect(DownloadCopy.ofTotal(300 << 20, Int64(2.1 * Double(gb))) == "300 MB of 2.1 GB")
    #expect(DownloadCopy.left(330) == "6 min left")
    #expect(DownloadCopy.left(3_900) == "1 h 5 min left")
    #expect(DownloadCopy.left(20) == "Less than a minute left")
    let row = DownloadRow(
        id: "d1", kind: "series", itemId: "tt1", videoId: "tt1:1:3", season: 1, episode: 3, name: "Severance", episodeTitle: "In Perpetuity",
        poster: nil, still: nil, runtime: "52 min", released: nil, state: .running, reason: nil, mobileData: false,
        bytesDone: Int64(1.2 * Double(gb)), bytesTotal: Int64(2.1 * Double(gb)), speedBps: nil, etaSecs: 330, quality: "1080p")
    #expect(DownloadCopy.progressLine(row) == "1.2 of 2.1 GB \u{B7} 6 min left")
    let waiting = DownloadRow(
        id: "d1", kind: "series", itemId: "tt1", videoId: "tt1:1:3", season: 1, episode: 3, name: "Severance", episodeTitle: "In Perpetuity",
        poster: nil, still: nil, runtime: "52 min", released: nil, state: .waiting, reason: .wifi, mobileData: false,
        bytesDone: Int64(1.2 * Double(gb)), bytesTotal: Int64(2.1 * Double(gb)), speedBps: nil, etaSecs: 330, quality: "1080p")
    #expect(DownloadCopy.progressLine(waiting) == "1.2 of 2.1 GB \u{B7} Waiting for Wi-Fi")
    #expect(DownloadCopy.episodeName(row) == "E3 In Perpetuity")
    #expect(DownloadCopy.episodeLine(row) == "52 min \u{B7} 2.1 GB")
    let film = DownloadTitle(itemId: "tt2", kind: "movie", name: "Sintel", poster: nil, count: 1, bytes: Int64(2.1 * Double(gb)), rows: [row])
    #expect(DownloadCopy.titleLine(film) == "1080p \u{B7} 2.1 GB")
    let show = DownloadTitle(itemId: "tt2", kind: "series", name: "Severance", poster: nil, count: 4, bytes: Int64(2.1 * Double(gb)), rows: [row])
    #expect(DownloadCopy.titleLine(show) == "4 episodes \u{B7} 2.1 GB")
    #expect(DownloadCopy.deleteSeries(4, "Severance") == "Delete 4 downloaded episodes of Severance?")
    #expect(DownloadCopy.deleteSeries(1, "Severance") == "Delete 1 downloaded episode of Severance?")
    #expect(DownloadCopy.deleteAll(12 * gb) == "Delete all downloads on this device? 12 GB")
    // The subtitles kept with it, by English names, once each.
    #expect(DownloadCopy.subtitlesLine(["en", "tr", "en", "pt-BR"]) == "Subtitles: English, Turkish, Portuguese (Brazil)")
    #expect(DownloadCopy.subtitlesLine([]) == nil)
    #expect(DownloadCopy.subtitlesLine(nil) == nil)
    // A Downloads row's control names what it is for (review r1 m8).
    #expect(DownloadCopy.spoken(DownloadCopy.control(badge(.done)), for: "Severance E3 In Perpetuity") == "Downloaded, Severance E3 In Perpetuity")
    #expect(DownloadCopy.spoken(DownloadCopy.control(badge(.done)), for: nil) == "Downloaded")
    // An episode card's spoken value gains the state.
    #expect(DownloadCopy.spokenState(badge(.done)) == "Downloaded")
    #expect(DownloadCopy.spokenState(badge(.running, progress: 0.45)) == "Downloading, 45 percent")
    #expect(DownloadCopy.spokenState(nil) == nil)
}

@Test func theOtherWordsAreTheSharedOnes() {
    #expect(DownloadCopy.emptyTitle == "Nothing downloaded yet")
    #expect(DownloadCopy.emptyBody == "Download movies and episodes to watch them without a connection.")
    #expect(DownloadCopy.offline == "You're offline. Your downloads still play.")
    #expect(DownloadCopy.offlineAction == "Go to Downloads")
    #expect(DownloadCopy.titleOffline == "Showing what's downloaded")
    #expect(DownloadCopy.smartLine == "When you finish a downloaded episode, the next one downloads and the one you watched is deleted.")
    #expect(DownloadCopy.pick == "Pick the version to keep on this device")
    #expect(DownloadCopy.pickAction == "Download this version")
    // Every glyph and item icon is in the catalog.
    let catalog = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Resources/Icons.xcassets")
    let glyphs: [DownloadGlyph] = [.download, .stop, .pause, .wifiOff, .clock, .check, .alert]
    for name in glyphs.map(\.asset) + DownloadItem.allCases.map(\.glyph) {
        #expect(FileManager.default.fileExists(atPath: catalog.appending(path: "\(name).imageset").path), "\(name)")
    }
}

// TV forms show none of it; a kids profile sees and plays, with no control and no items.
@Test func tvFormsShowNoDownloadsAndKidsGetNoControl() {
    #expect(!DownloadGate.shows(tv: true))
    #expect(!DownloadGate.controls(tv: true, kids: false))
    #expect(DownloadGate.shows(tv: false))
    #expect(DownloadGate.controls(tv: false, kids: false))
    #expect(!DownloadGate.controls(tv: false, kids: true))
    // This build is a Mac: the gate opens here.
    #expect(!DownloadGate.tv)
}

@Test func tvOSCompilesNoDownloadUI() throws {
    // Every screen file of downloads is inside `#if !os(tvOS)`, as the keeper is.
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    for file in ["Downloads/DownloadKeeper.swift", "Downloads/DownloadControlView.swift", "Downloads/DownloadsScreen.swift", "Settings/DownloadsSettings.swift"] {
        let text = try String(contentsOf: app.appending(path: file), encoding: .utf8)
        let first = text.split(separator: "\n").first { !$0.hasPrefix("//") && !$0.isEmpty }
        #expect(first == "#if !os(tvOS)", "\(file)")
    }
}

// MARK: The keeper (plans/offline-downloads.md 5)

private func facts(left: Int, done: Set<String> = [], bytes: (Int64, Int64) = (0, 0), title: String? = "Severance") -> KeeperFacts {
    KeeperFacts(left: left, bytesDone: bytes.0, bytesTotal: bytes.1, title: title, done: done)
}

private func submits(_ effects: [KeeperEffect]) -> Int {
    effects.filter { if case .submit = $0 { true } else { false } }.count
}

@Test func oneTaskPerQueueAndASecondTapGrowsItsTotal() throws {
    var keeper = KeeperLogic(platform: .continued)
    _ = keeper.on(.facts(facts(left: 0)))
    let first = keeper.on(.userAction)
    #expect(submits(first) == 1)
    guard case .submit(let started)? = first.first(where: { if case .submit = $0 { true } else { false } }) else { Issue.record("no submit"); return }
    _ = keeper.on(.facts(facts(left: 1, bytes: (0, 100))))
    // A second tap while it runs: no second task, a bigger total.
    let second = keeper.on(.userAction)
    #expect(submits(second) == 0)
    let grown = second.compactMap { if case .progress(let p) = $0 { p } else { nil } }
    #expect(grown.count == 1)
    #expect((grown.first?.total ?? 0) > started.total)
    #expect(grown.first?.subtitle.hasPrefix("1 of 2") == true)
    // The core counts it: the system sees both, then the first done.
    let counted = keeper.on(.facts(facts(left: 1, done: ["a"], bytes: (50, 100))))
    let shown = counted.compactMap { if case .progress(let p) = $0 { p } else { nil } }.first
    #expect(shown?.title == "Downloading Severance")
    #expect(shown?.subtitle == "2 of 2 \u{B7} 50%")
    // The queue empties: the task ends, once.
    #expect(keeper.on(.facts(facts(left: 0, done: ["a", "b"]))).contains(.end(success: true)))
    #expect(!keeper.on(.facts(facts(left: 0, done: ["a", "b"]))).contains(.end(success: true)))
    // A tap after that starts a new one.
    #expect(submits(keeper.on(.userAction)) == 1)
}

// A snapshot already on its way when the user tapped (nothing running yet) never ends the task the tap
// submitted; the core counting the add does, and so does `settle` when the core counted nothing (review r1 m1).
@Test func aStaleSnapshotAroundATapKeepsTheTask() {
    var keeper = KeeperLogic(platform: .continued)
    _ = keeper.on(.facts(facts(left: 0)))
    #expect(submits(keeper.on(.userAction)) == 1)
    #expect(!keeper.on(.facts(facts(left: 0))).contains(.end(success: true)))
    #expect(keeper.task)
    let counted = keeper.on(.facts(facts(left: 1)))
    #expect(counted.contains { if case .progress = $0 { true } else { false } })
    #expect(keeper.on(.facts(facts(left: 0, done: ["a"]))).contains(.end(success: true)))
    // A tap that adds nothing: the task goes at `settle`.
    #expect(submits(keeper.on(.userAction)) == 1)
    #expect(!keeper.on(.facts(facts(left: 0, done: ["a"]))).contains(.end(success: true)))
    #expect(keeper.on(.settle).contains(.end(success: true)))
    #expect(!keeper.task)
    // With work running, `settle` leaves the task alone.
    _ = keeper.on(.userAction)
    _ = keeper.on(.facts(facts(left: 1)))
    #expect(!keeper.on(.settle).contains(.end(success: true)))
}

@Test func expirationSendsDownloadsAllowedFalse() {
    var keeper = KeeperLogic(platform: .continued)
    _ = keeper.on(.facts(facts(left: 2)))
    _ = keeper.on(.userAction)
    // Held by the task, the downloads go on in the background.
    #expect(!keeper.on(.background).contains(.allowed(false, reason: "app")))
    let expired = keeper.on(.expired)
    #expect(expired.contains(.end(success: false)))
    #expect(expired.contains(.allowed(false, reason: "app")))
    // Back in front, they may run again.
    #expect(keeper.on(.foreground).contains(.allowed(true, reason: nil)))
}

// An expiry with the app in front (the user cancels the system's progress item): the task ends and the
// downloads go on while the app is open; leaving the app holds them (review r1 m2).
@Test func anExpiryInTheForegroundEndsTheTaskAndDownloadsGoOn() {
    var keeper = KeeperLogic(platform: .continued)
    _ = keeper.on(.facts(facts(left: 2)))
    _ = keeper.on(.userAction)
    let expired = keeper.on(.expired)
    #expect(expired.contains(.end(success: false)))
    #expect(!expired.contains(.allowed(false, reason: "app")))
    #expect(keeper.on(.background).contains(.allowed(false, reason: "app")))
}

// An iPad window going away while another is in front does not hold the downloads (review r1 m7).
@Test func onlyTheLastWindowLeavingHoldsDownloads() throws {
    #expect(KeeperLogic.appLeft(windowsInFront: 0))
    #expect(!KeeperLogic.appLeft(windowsInFront: 1))
    #expect(!KeeperLogic.appLeft(windowsInFront: 2))
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    let events = try String(contentsOf: app.appending(path: "App/SystemEvents.swift"), encoding: .utf8)
    #expect(events.contains("if KeeperLogic.appLeft(windowsInFront: Self.windowsInFront) { downloads?.handle(.background) }"))
}

@Test func belowIOS26TheBackgroundHoldsDownloads() {
    var keeper = KeeperLogic(platform: .foreground)
    _ = keeper.on(.facts(facts(left: 1)))
    // No continued task there: a tap submits nothing.
    #expect(submits(keeper.on(.userAction)) == 0)
    #expect(keeper.on(.background).contains(.allowed(false, reason: "app")))
    #expect(keeper.on(.foreground).contains(.allowed(true, reason: nil)))
    // Without a task the background holds them on iOS 26 too.
    var later = KeeperLogic(platform: .continued)
    _ = later.on(.facts(facts(left: 1)))
    #expect(later.on(.background).contains(.allowed(false, reason: "app")))
}

@Test func theMacActivityBeginsAndEndsWithActiveDownloads() {
    var keeper = KeeperLogic(platform: .mac)
    #expect(!keeper.on(.facts(facts(left: 0))).contains(.beginActivity))
    let started = keeper.on(.facts(facts(left: 2, bytes: (25, 100))))
    #expect(started.contains(.beginActivity))
    #expect(started.contains(.dock(0.25)))
    // Once, while they run.
    #expect(!keeper.on(.facts(facts(left: 1, bytes: (50, 100)))).contains(.beginActivity))
    let ended = keeper.on(.facts(facts(left: 0)))
    #expect(ended.contains(.endActivity))
    #expect(ended.contains(.dock(nil)))
    // The Mac never holds them, not even in the background, and Quit asks only while they run.
    #expect(!keeper.on(.background).contains(.allowed(false, reason: "app")))
    #expect(submits(keeper.on(.userAction)) == 0)
    #expect(!KeeperLogic.asksBeforeQuit(facts(left: 0)))
    #expect(KeeperLogic.asksBeforeQuit(facts(left: 1)))
}

// The Dock tile redraws when the bar moves a percent or more, not on every snapshot (review r1 m9).
@Test func theDockRedrawsOnlyForAVisibleMove() {
    var keeper = KeeperLogic(platform: .mac)
    #expect(keeper.on(.facts(facts(left: 1, bytes: (100, 1000)))).contains(.dock(0.1)))
    #expect(!keeper.on(.facts(facts(left: 1, bytes: (105, 1000)))).contains { if case .dock = $0 { true } else { false } })
    #expect(keeper.on(.facts(facts(left: 1, bytes: (120, 1000)))).contains(.dock(0.12)))
    #expect(keeper.on(.facts(facts(left: 0))).contains(.dock(nil)))
}

@Test func downloadsAllowedGoesOnEveryChangeEveryMinuteAndAtForeground() {
    var keeper = KeeperLogic(platform: .continued)
    #expect(keeper.on(.facts(facts(left: 1))) == [.allowed(true, reason: nil)])
    // The same again: nothing new to say.
    #expect(keeper.on(.facts(facts(left: 1, bytes: (5, 10)))) == [])
    #expect(keeper.on(.facts(facts(left: 2))) == [.allowed(true, reason: nil)])
    #expect(keeper.on(.minute) == [.allowed(true, reason: nil)])
    #expect(keeper.on(.foreground) == [.allowed(true, reason: nil)])
    _ = keeper.on(.facts(facts(left: 0)))
    // Nothing runs: the minute says nothing.
    #expect(keeper.on(.minute) == [])
}

// A core opened again knows neither the platform's hold nor the network: the next event says the hold again
// (review r1 M1).
@Test func aViewOpenedAgainHearsDownloadsAllowedAgain() {
    var keeper = KeeperLogic(platform: .foreground)
    _ = keeper.on(.facts(facts(left: 1)))
    _ = keeper.on(.background)
    // Nothing changed: nothing to say.
    #expect(keeper.on(.facts(facts(left: 1))) == [])
    keeper.resend()
    #expect(keeper.on(.facts(facts(left: 1))) == [.allowed(false, reason: "app")])
}

@Test func aCoreOpenedAgainHearsTheNetworkAndTheHold() throws {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    let events = try String(contentsOf: app.appending(path: "App/SystemEvents.swift"), encoding: .utf8)
    let shell = try String(contentsOf: app.appending(path: "App/MarqueeApp.swift"), encoding: .utf8)
    let keeper = try String(contentsOf: app.appending(path: "Downloads/DownloadKeeper.swift"), encoding: .utf8)
    #expect(events.contains("if let last { core.lifecycle(\"networkChanged\", last.fields) }"))
    #expect(shell.contains(".task(id: core.generation) { system.resend() }"))
    #expect(keeper.contains("logic.resend()"))
}

// Only iOS sends its free space; the Mac's purgeable space would admit what the disk cannot hold (review r1 M3).
@MainActor @Test func onlyIOSSendsFreeBytes() throws {
    #expect(!KeeperLogic.reportsFreeBytes(.mac))
    #expect(KeeperLogic.reportsFreeBytes(.continued))
    #expect(KeeperLogic.reportsFreeBytes(.foreground))
    // This build is a Mac: its keeper never puts the field in.
    #expect(!KeeperLogic.reportsFreeBytes(DownloadKeeper.platform))
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    let keeper = try String(contentsOf: app.appending(path: "Downloads/DownloadKeeper.swift"), encoding: .utf8)
    #expect(keeper.components(separatedBy: "fields[\"freeBytes\"]").count - 1 == 1)
    #expect(keeper.contains("if KeeperLogic.reportsFreeBytes(Self.platform), let free = freeBytes() { fields[\"freeBytes\"] = free }"))
}

// One continued-task identifier for the process, under the wildcard Info.plist permits, registered once (review r1 m5).
@Test func theContinuedTaskHasOneIdentifierRegisteredOnce() throws {
    let id = KeeperLogic.taskIdentifier(bundle: "app.marquee.dev")
    #expect(id == KeeperLogic.taskIdentifier(bundle: "app.marquee.dev"))
    #expect(id == "app.marquee.dev.downloads.queue")
    let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "..")
    let spec = try String(contentsOf: root.appending(path: "project.yml"), encoding: .utf8)
    #expect(spec.contains(#"BGTaskSchedulerPermittedIdentifiers: ["$(PRODUCT_BUNDLE_IDENTIFIER).downloads.*"]"#))
    let keeper = try String(contentsOf: root.appending(path: "Marquee/Downloads/DownloadKeeper.swift"), encoding: .utf8)
    #expect(keeper.contains("if !handlerRegistered {\n            handlerRegistered = BGTaskScheduler.shared.register(forTaskWithIdentifier: id"))
    #expect(!keeper.contains("UUID()"))
}

@Test func theKeeperOnlyPerformsTheReducersEffects() throws {
    // Read from the sources: the iOS keeper submits, ends and sends only what `KeeperLogic` decides.
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    let text = try String(contentsOf: app.appending(path: "Downloads/DownloadKeeper.swift"), encoding: .utf8)
    let count = { (needle: String) in text.components(separatedBy: needle).count - 1 }
    #expect(count("BGTaskScheduler.shared.submit(") == 1)
    #expect(count("submit(progress)") == 1)
    #expect(count("case .submit(let progress): submit(progress)") == 1)
    #expect(count("for effect in logic.on(event) { perform(effect) }") == 1)
    #expect(count("ProcessInfo.processInfo.beginActivity(") == 1)
    #expect(count("case .beginActivity:") == 1)
}

// The view closes with the profile (a lock, the picker): nothing reads as running any more (review r1 M2).
@MainActor @Test func aClosedViewUnwindsTheKeeper() async throws {
    let core = RealCore()
    #expect(await core.ready())
    let keeper = DownloadKeeper(core: core.client)
    keeper.begin(open: true)
    keeper.handle(.facts(facts(left: 2, bytes: (10, 100))))
    #expect(keeper.asksBeforeQuit)
    keeper.begin(open: false)
    #expect(keeper.view == nil)
    #expect(!keeper.asksBeforeQuit)
}

// Logout, restart and shutdown hold the downloads without asking; a user's Quit asks (review r1 m4).
@Test func aSystemQuitDoesNotAsk() {
    for reason in [kAELogOut, kAEReallyLogOut, kAEShowRestartDialog, kAERestart, kAEShowShutdownDialog, kAEShutDown] {
        #expect(!MacAppDelegate.asks(quitReason: OSType(reason)), "\(reason)")
    }
    #expect(MacAppDelegate.asks(quitReason: nil))
    #expect(MacAppDelegate.asks(quitReason: OSType(kAEQuitApplication)))
}

// A download keeps App Nap off, not the Mac awake (review r1 m3).
@MainActor @Test func theMacActivityLetsTheMacSleep() {
    #expect(DownloadKeeper.activityOptions == .userInitiatedAllowingIdleSystemSleep)
    #expect(!DownloadKeeper.activityOptions.contains(.idleSystemSleepDisabled))
}

// Quit waits for the core to checkpoint and hold the downloads before the app goes (review r1 M4).
@MainActor @Test func quitWaitsForTheCoresAnswer() async throws {
    let core = RealCore()
    #expect(await core.ready())
    let keeper = DownloadKeeper(core: core.client)
    var answered = 0
    keeper.holdForQuit { answered += 1 }
    // Not before the core has answered.
    #expect(answered == 0)
    #expect(await eventually { answered == 1 })
    // Once, though the time limit comes after the answer.
    try await Task.sleep(for: .milliseconds(2300))
    #expect(answered == 1)
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    let text = try String(contentsOf: app.appending(path: "Downloads/DownloadKeeper.swift"), encoding: .utf8)
    #expect(text.contains("keeper.holdForQuit { sender.reply(toApplicationShouldTerminate: true) }\n            return .terminateLater"))
}

// MARK: The network (plans/offline-downloads.md 4.13)

@Test func thePathMapsToNoneMeteredAndLowData() {
    func report(_ satisfied: Bool, wifi: Bool = false, cellular: Bool = false, expensive: Bool = false, constrained: Bool = false, desktop: Bool = false) -> NetworkReport {
        NetworkReport.of(satisfied: satisfied, wifi: wifi, ethernet: false, cellular: cellular, expensive: expensive, constrained: constrained, desktop: desktop)
    }
    #expect(report(false, wifi: true, expensive: true) == NetworkReport(network: "none", metered: false, lowData: false))
    #expect(report(true, wifi: true) == NetworkReport(network: "wifi", metered: false, lowData: false))
    // A phone's hotspot: Wi-Fi, but the cost flag says metered.
    #expect(report(true, wifi: true, expensive: true) == NetworkReport(network: "wifi", metered: true, lowData: false))
    #expect(report(true, cellular: true, expensive: true, constrained: true) == NetworkReport(network: "cellular", metered: true, lowData: true))
    // A Mac never holds downloads for cost (its switch is hidden).
    #expect(report(true, wifi: true, expensive: true, constrained: true, desktop: true) == NetworkReport(network: "wifi", metered: false, lowData: false))
    let fields = report(true, wifi: true, expensive: true).fields
    #expect(fields["network"] as? String == "wifi" && fields["metered"] as? Bool == true && fields["lowData"] as? Bool == false)
}

// MARK: A real core

@MainActor @Test func theDownloadsViewDecodesAndItsOpsAnswer() async throws {
    let core = RealCore()
    #expect(await core.ready())
    let downloads = LiveRecord<DownloadsSnapshot>("downloads", core: core.client)
    downloads.start()
    #expect(await eventually { downloads.snapshot != nil })
    #expect(downloads.failure == nil)
    #expect(downloads.snapshot?.settings == DownloadSettings(wifiOnly: true, smart: true))
    #expect(downloads.snapshot?.downloading.isEmpty == true && downloads.snapshot?.titles.isEmpty == true)
    downloads.performNow("setDownloadSettings", ["smart": false])
    #expect(await eventually { downloads.snapshot?.settings.smart == false })
    try await core.client.perform("downloadsAllowed", ["allowed": false, "reason": "app", "freeBytes": Int64(10) << 30])
    core.client.lifecycle("networkChanged", NetworkReport(network: "none", metered: false, lowData: false).fields)
    try await core.client.perform("downloadsAllowed", ["allowed": true])
    #expect(downloads.failure == nil)
}
