import XCTest

/// The offline downloads walk (plans/offline-downloads.md 7) on a connected iPhone: a season download that goes on
/// with the app in the background, the offline banner over Home, a title page, a pushed page and Downloads, and the
/// two delete confirms (Delete Season on the title page, Delete all on a series' Downloads page). Run only through
/// `apps/apple/scripts/walk-ui.sh <UDID> -only-testing:MarqueeWalkUITests/DownloadsWalk` with `MQ_WALK_ARGS` naming
/// the walk's source for Breaking Bad season 1 (`-MQInstallAddons <manifest URL>`). Muted (`-MQMute YES`); offline is
/// the walk hook `-MQOffline YES`, never the phone's airplane mode.
@MainActor final class DownloadsWalk: XCTestCase {
    private var app: XCUIApplication { XCUIApplication(bundleIdentifier: PlayerGesturesWalk.walkApp) }
    private let base = ["-MQMute", "YES"]
    private let title = ["-MQOpenTitle", "series:tt0903747"]

    func testDownloadsWalk() throws {
        let named = ProcessInfo.processInfo.environment["MQ_WALK_APP"]
        guard named == PlayerGesturesWalk.walkApp else {
            XCTFail("run through scripts/walk-ui.sh: MQ_WALK_APP is \(named ?? "unset")")
            return
        }
        continueAfterFailure = true
        let source = (ProcessInfo.processInfo.environment["MQ_WALK_ARGS"] ?? "").split(separator: " ").map(String.init)

        // 1. The season, started from the title page with a real tap.
        allowPrompts(source)
        guard let control = header("Download", timeout: 90) else {
            shot("1 no Download control")
            XCTFail("the title page showed no Download control")
            return
        }
        shot("1 title online")
        control.tap()
        let season = app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Download Season 1'")).firstMatch
        guard season.waitForExistence(timeout: 10) else {
            shot("1 no season item")
            openDownloads()
            shot("1 Downloads when the menu had no season")
            XCTFail("the Download menu had no Download Season 1")
            return
        }
        note("season item: \(season.label)")
        season.tap()
        sleep(4)
        shot("2 season started")

        // 2. The app leaves the front: the continued task holds the downloads, and the system shows its item.
        XCUIDevice.shared.press(.home)
        sleep(3)
        shot("2 home with the system item")
        sleep(40)
        shot("2 home after 40 s")
        sleep(40)
        shot("2 home after 80 s")
        app.activate()
        sleep(2)
        openDownloads()
        shot("2 Downloads back in front")
        // The title's row on Downloads once all seven are done, and nothing left under Downloading.
        let whole = app.buttons.matching(NSPredicate(format: "label CONTAINS 'Breaking Bad' AND label CONTAINS '7 episodes'")).firstMatch
        XCTAssertTrue(whole.waitForExistence(timeout: 240), "all seven episodes downloaded")
        let downloading = app.staticTexts["Downloading"]
        let done = Date()
        while downloading.exists, Date().timeIntervalSince(done) < 120 { sleep(2) }
        XCTAssertFalse(downloading.exists, "nothing still downloading")
        shot("2 Downloads, the season done")

        // 3. Offline (the walk hook): the shell's banner over Home, the title page, a pushed page and Downloads.
        launch(base + ["-MQOffline", "YES", "-MQTab", "home"])
        sleep(8)
        banner("3 offline Home")
        launch(base + ["-MQOffline", "YES"] + title)
        sleep(8)
        banner("3 offline title page")
        openDownloads()
        banner("3 offline Downloads (pushed, with its bar)")
        clear("3 offline Downloads")
        let row = app.buttons.matching(NSPredicate(format: "label CONTAINS 'Breaking Bad' AND label CONTAINS 'episodes'")).firstMatch
        if row.waitForExistence(timeout: 10) {
            row.tap()
            sleep(2)
            banner("3 offline series page (pushed)")
            clear("3 offline series page")
        }

        // 4. Delete Season on the title page: its confirm's Delete removes the season's rows (review r3 N6).
        launch(base + title)
        let downloaded = header("Downloaded", timeout: 60)
        XCTAssertNotNil(downloaded, "the title says Downloaded")
        downloaded?.tap()
        let delete = app.buttons["Delete Season 1 downloads"]
        XCTAssertTrue(delete.waitForExistence(timeout: 10), "the menu has Delete Season 1 downloads")
        delete.tap()
        confirm("4 Delete Season confirm")
        shot("4 title just after Delete Season")
        let again = header("Download", timeout: 30)
        XCTAssertNotNil(again, "the season is gone: the title says Download again")
        shot("4 title after Delete Season")
        openDownloads()
        XCTAssertTrue(app.staticTexts["Nothing downloaded yet"].waitForExistence(timeout: 15), "Downloads is empty")
        shot("4 Downloads empty after Delete Season")

        app.terminate()
    }

    /// A fresh install asks for notifications and for the local network (the source is on this Mac): allow each
    /// prompt as it comes, then launch again so the source installs with the network allowed, on the title page.
    private func allowPrompts(_ source: [String]) {
        launch(base + source + title)
        let allow = XCUIApplication(bundleIdentifier: "com.apple.springboard").buttons["Allow"]
        let asked = Date()
        while Date().timeIntervalSince(asked) < 25 {
            if allow.waitForExistence(timeout: 2) {
                note("allowed: \(XCUIApplication(bundleIdentifier: "com.apple.springboard").alerts.firstMatch.label)")
                allow.tap()
            }
        }
        let browse = app.buttons["Browse first"]
        if browse.waitForExistence(timeout: 5) { browse.tap() }
        sleep(2)
        launch(base + source + title)
        if allow.waitForExistence(timeout: 8) { allow.tap() }
    }

    /// A whole series deleted from Downloads (the row swiped): its confirm's Delete removes every row (review r3 N6).
    /// Its own test, so a run can walk it alone.
    func testDeleteASeriesFromDownloads() throws {
        guard ProcessInfo.processInfo.environment["MQ_WALK_APP"] == PlayerGesturesWalk.walkApp else {
            XCTFail("run through scripts/walk-ui.sh")
            return
        }
        continueAfterFailure = true
        allowPrompts((ProcessInfo.processInfo.environment["MQ_WALK_ARGS"] ?? "").split(separator: " ").map(String.init))
        guard let control = header("Download", timeout: 90) else {
            shot("6 no Download control")
            XCTFail("the title page showed no Download control")
            return
        }
        control.tap()
        let season = app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Download Season 1'")).firstMatch
        XCTAssertTrue(season.waitForExistence(timeout: 10))
        season.tap()
        sleep(2)
        openDownloads()
        let whole = app.buttons.matching(NSPredicate(format: "label CONTAINS 'Breaking Bad' AND label CONTAINS '7 episodes'")).firstMatch
        XCTAssertTrue(whole.waitForExistence(timeout: 300), "the season downloaded")
        let downloading = app.staticTexts["Downloading"]
        let done = Date()
        while downloading.exists, Date().timeIntervalSince(done) < 120 { sleep(2) }
        sleep(2)
        shot("6 Downloads, the season done")
        // The series page's More, then Delete all: it asks first for the whole series (review r3 N6).
        whole.tap()
        sleep(2)
        let more = app.buttons["downloads-series-more"]
        XCTAssertTrue(more.waitForExistence(timeout: 10), "the series page has More")
        more.tap()
        let all = app.buttons["Delete all"]
        XCTAssertTrue(all.waitForExistence(timeout: 5), "More has Delete all")
        shot("6 series page More")
        all.tap()
        sleep(1)
        confirm("6 Delete all confirm")
        XCTAssertTrue(app.staticTexts["Nothing downloaded yet"].waitForExistence(timeout: 20), "the series delete left nothing")
        shot("6 after the series delete")
        app.terminate()
    }

    private func launch(_ arguments: [String]) {
        let app = app
        app.terminate()
        app.launchArguments = arguments
        app.launch()
    }

    /// The title's own control: of the buttons named `label` (episode rows have theirs), the one highest up.
    private func header(_ label: String, timeout: TimeInterval) -> XCUIElement? {
        let query = app.buttons.matching(NSPredicate(format: "label == %@", label))
        guard query.firstMatch.waitForExistence(timeout: timeout) else { return nil }
        let top = { query.allElementsBoundByIndex.min { $0.frame.minY < $1.frame.minY } }
        // Clear of the tab bar first: the pill sits low under a tall hero.
        if let found = top(), found.frame.maxY > app.frame.height - 260 {
            let from = app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.7))
            from.press(forDuration: 0.05, thenDragTo: app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.4)))
            sleep(1)
        }
        return top()
    }

    /// Library, then its Downloads row.
    private func openDownloads() {
        let tab = app.tabBars.buttons["Library"].exists ? app.tabBars.buttons["Library"] : app.buttons["Library"].firstMatch
        if tab.waitForExistence(timeout: 10) { tab.tap() }
        let row = app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Downloads'")).firstMatch
        if row.waitForExistence(timeout: 10) { row.tap() }
        sleep(2)
    }

    /// The confirm's Delete, with a shot of the dialog first.
    private func confirm(_ name: String) {
        let delete = app.buttons.matching(NSPredicate(format: "label == 'Delete'")).firstMatch
        XCTAssertTrue(delete.waitForExistence(timeout: 10), "\(name): the confirm shows Delete")
        shot(name)
        delete.tap()
        sleep(2)
    }

    /// The page's own top is clear of the banner: a navigation bar's Back, or the title page's back button, can be
    /// tapped where it shows (review r5 M1).
    private func clear(_ name: String) {
        let back = app.navigationBars.buttons.firstMatch
        let banner = app.staticTexts.matching(NSPredicate(format: "label BEGINSWITH \"You're offline.\"")).firstMatch
        guard back.waitForExistence(timeout: 5), banner.exists else {
            note("\(name): no back to check")
            return
        }
        note("\(name): back at \(back.frame.minY), banner ends \(banner.frame.maxY)")
        XCTAssertTrue(back.isHittable, "\(name): Back can be tapped")
    }

    /// A shot that also says whether the banner's constant words are on screen.
    private func banner(_ name: String) {
        let shown = app.staticTexts.matching(NSPredicate(format: "label BEGINSWITH \"You're offline.\"")).firstMatch.waitForExistence(timeout: 10)
        note("\(name): banner \(shown ? "shown" : "missing")")
        XCTAssertTrue(shown, "\(name): the offline banner")
        shot(name)
    }

    private func note(_ text: String) {
        let line = XCTAttachment(string: text)
        line.name = text
        line.lifetime = .keepAlways
        add(line)
    }

    private func shot(_ name: String) {
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        shot.name = name
        shot.lifetime = .keepAlways
        add(shot)
    }
}
