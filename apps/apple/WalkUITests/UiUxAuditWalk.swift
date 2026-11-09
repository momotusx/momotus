import XCTest

/// The iPhone walk of plans/2026-10-28-ui-ux-audit.md, Phase I: measured touch targets (I4), the profile editor's
/// dialogs and its one Back (I5), and a shot of each screen the phase touched. Run only through
/// `apps/apple/scripts/walk-ui.sh <UDID> -only-testing:MarqueeWalkUITests/UiUxAuditWalk`, which builds the app as
/// `app.momotus.walk`; nothing else is launched or touched. Muted (`-MQMute YES`); a fresh install skips the first-run
/// flow (`-MQPrefsPatch`). The player tests need a source for Big Buck Bunny (tt1254207) in `MQ_WALK_ARGS`.
/// The tests run in name order and share the install: the profiles test leaves a second profile behind.
@MainActor final class UiUxAuditWalk: XCTestCase {
    static let walkApp = "app.momotus.walk"
    private var app: XCUIApplication { XCUIApplication(bundleIdentifier: Self.walkApp) }

    override func setUp() async throws {
        let named = ProcessInfo.processInfo.environment["MQ_WALK_APP"]
        guard named == Self.walkApp else {
            throw XCTSkip("run through scripts/walk-ui.sh: MQ_WALK_APP is \(named ?? "unset"), not \(Self.walkApp)")
        }
        continueAfterFailure = true
    }

    func test1Screens() {
        launch([])
        tab("Library")
        shot("library-empty")
        tab("Calendar")
        shot("calendar")
        tab("Search")
        shot("search-empty")
        let field = app.textFields["Search titles"].firstMatch
        if field.waitForExistence(timeout: 5) {
            field.tap()
            field.typeText("zzqxqzvbnmw\n")
            sleep(6)
            shot("search-no-results")
        } else {
            XCTFail("no Search field")
        }
    }

    func test2SeriesTitle() {
        launch(["-MQOpenTitle", "series:tt0903747"])
        let season = app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Season'")).firstMatch
        if season.waitForExistence(timeout: 40) {
            shot("title-series")
            measure("season pull-down", season)
        } else {
            shot("title-series-no-season")
            XCTFail("no season pull-down on the title page")
        }
        // A Library item for the Library shot with items.
        let add = app.buttons["Add to Library"].firstMatch
        if add.waitForExistence(timeout: 5) { add.tap() }
        sleep(2)
        app.terminate()
        launch(["-MQTab", "library"])
        sleep(4)
        shot("library-with-items")
    }

    func test3TitleFailure() {
        launch(["-MQOpenTitle", "movie:tt9999999999"])
        sleep(25)
        shot("title-failed")
        let retry = app.buttons["Try again"].firstMatch
        record("title failure shows Try again: \(retry.exists)")
    }

    func test4PlayerTargets() {
        launch(["-MQOpenTitle", "movie:tt1254207", "-MQAutoPlay", "YES"])
        XCUIDevice.shared.orientation = .landscapeLeft
        guard playing() else { return }
        // Paused, so the controls stay up while each frame is read.
        pause()
        shot("player-controls")
        tree("player tree across")
        measure("player Back", app.buttons["Back"].firstMatch)
        measure("Picture in Picture", app.buttons["Picture in Picture"].firstMatch)
        let airPlay = app.descendants(matching: .any)["AirPlay"].firstMatch
        if airPlay.exists { measure("AirPlay", airPlay) } else { record("AirPlay: not shown for this stream") }
        pills("")
        XCUIDevice.shared.orientation = .portrait
        sleep(2)
        showControls()
        shot("player-controls-upright")
        tree("player tree upright")
        pills(" upright")
    }

    private func pause() {
        showControls()
        let pause = app.buttons["Pause"].firstMatch
        if pause.exists { pause.tap() }
        sleep(1)
        showControls()
    }

    private func pills(_ form: String) {
        for pill in ["Audio", "Subtitles", "Versions", "Speed"] {
            showControls()
            measure("\(pill) pill\(form)", app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", pill)).firstMatch)
        }
    }

    private func tree(_ name: String) {
        let note = XCTAttachment(string: app.debugDescription)
        note.name = name
        note.lifetime = .keepAlways
        add(note)
    }

    func test5EndCard() {
        launch(["-MQOpenTitle", "movie:tt1254207", "-MQAutoPlay", "YES", "-MQSeekTo", "585"])
        XCUIDevice.shared.orientation = .landscapeLeft
        guard playing() else { return }
        let back = app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Back to'")).firstMatch
        if back.waitForExistence(timeout: 60) {
            sleep(1)
            shot("player-end-card")
        } else {
            shot("player-no-end-card")
            XCTFail("no end card within 60 s")
        }
        XCUIDevice.shared.orientation = .portrait
    }

    func test6Profiles() {
        launch(["-MQTab", "settings"])
        // The owner's name, from Settings' profile row ("<name>, <detail>").
        let row = app.buttons.matching(NSPredicate(format: "label CONTAINS 'switch' OR label CONTAINS 'Switch'")).firstMatch
        guard row.waitForExistence(timeout: 20) else { return fail("no profile row in Settings") }
        let owner = String(row.label.split(separator: ",").first ?? "")
        record("owner profile: \(owner)")

        // A second profile, Walk B.
        tapLabel("Manage profiles")
        shot("profiles-manage")
        tapLabel("Add profile")
        type("Walk B", into: app.textFields["Name"].firstMatch)
        let add = app.buttons.matching(NSPredicate(format: "label == 'Add profile'")).allElementsBoundByIndex.last
        add?.tap()
        sleep(3)
        shot("profiles-manage-two")
        back()
        // Switch to Walk B and give it a PIN from its own editor.
        switchTo("Walk B")
        tapLabel("Edit profile")
        tapLabel("Set a PIN")
        for _ in 0..<2 {
            for digit in ["3", "7", "1", "9"] { app.buttons[digit].firstMatch.tap() }
            sleep(2)
        }
        sleep(2)
        shot("profiles-pin-set")
        back()
        switchTo(owner)

        // The owner removes Walk B's PIN: the dialog asks first, in the key blue, Cancel the default.
        tapLabel("Manage profiles")
        tapLabel("Walk B")
        sleep(2)
        shot("profiles-editor")
        tapLabel("Remove Walk B's PIN")
        let sheet = app.buttons["Cancel"].firstMatch
        if sheet.waitForExistence(timeout: 5) {
            sleep(1)
            shot("pin-removal-dialog")
            // Return from a hardware keyboard: the default action must be Cancel, so the PIN stays.
            app.typeKey(XCUIKeyboardKey.return, modifierFlags: [])
            sleep(2)
            let open = sheet.exists
            let pinKept = app.buttons["Remove Walk B's PIN"].exists && !app.staticTexts["Walk B's PIN was removed."].exists
            record("PIN dialog after Return: dialog open \(open), PIN kept \(pinKept)")
            shot("pin-removal-after-return")
            if open { sheet.tap() }
        } else {
            fail("no PIN removal dialog")
        }

        // An edit makes the page dirty: one Back in the bar, and it asks.
        let barBefore = navButtons()
        record("editor bar before an edit: \(barBefore)")
        let name = app.textFields["Name"].firstMatch
        type("x", into: name)
        sleep(1)
        let barAfter = navButtons()
        record("editor bar with an edit: \(barAfter)")
        XCTAssertEqual(barAfter.filter { $0.hasPrefix("Back") }.count, 1, "one Back on the dirty editor: \(barAfter)")
        shot("profiles-editor-dirty")
        app.navigationBars.buttons["Back"].firstMatch.tap()
        let ask = app.staticTexts["You have unsaved changes"].firstMatch
        if ask.waitForExistence(timeout: 5) {
            sleep(1)
            shot("leave-ask-dialog")
            record("leave ask after one Back: shown")
            app.buttons["Discard changes"].firstMatch.tap()
        } else {
            shot("leave-ask-missing")
            fail("no leave ask after Back on the dirty editor")
        }
        sleep(2)
        shot("profiles-after-discard")
    }

    // MARK: helpers

    private var walkArgs: [String] {
        (ProcessInfo.processInfo.environment["MQ_WALK_ARGS"] ?? "").split(separator: " ").map(String.init)
    }

    private func launch(_ arguments: [String]) {
        let app = app
        app.launchArguments = ["-MQMute", "YES", "-MQPrefsPatch", #"{"defaultsSeen":1000}"#] + arguments + walkArgs
        app.launch()
        // The walk install's first network call asks for the local network (its source is on the Mac).
        let allow = XCUIApplication(bundleIdentifier: "com.apple.springboard").buttons["Allow"]
        if allow.waitForExistence(timeout: 4) { allow.tap() }
        if app.passFirstRun(timeout: 3) { sleep(3) }
        sleep(3)
    }

    private func tab(_ title: String) {
        let button = app.buttons[title].firstMatch
        guard button.waitForExistence(timeout: 10) else { return fail("no \(title) tab") }
        button.tap()
        sleep(4)
    }

    private func tapLabel(_ label: String) {
        let element = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", label)).firstMatch
        guard element.waitForExistence(timeout: 15) else { return fail("no button \(label)") }
        element.tap()
        sleep(2)
    }

    private func type(_ text: String, into field: XCUIElement) {
        guard field.waitForExistence(timeout: 10) else { return fail("no field for \(text)") }
        field.tap()
        field.typeText(text)
    }

    /// The system or the page's own back in the navigation bar.
    private func back() {
        let bar = app.navigationBars.buttons.firstMatch
        if bar.waitForExistence(timeout: 5) { bar.tap() }
        sleep(2)
    }

    private func navButtons() -> [String] {
        app.navigationBars.buttons.allElementsBoundByIndex.map { "\($0.label) \(Int($0.frame.width))x\(Int($0.frame.height))" }
    }

    /// From Settings: back to its root, the picker, the profile's tile, back to Settings' root.
    private func switchTo(_ name: String) {
        let row = toSettingsRoot()
        guard row.exists else { return fail("no switch row in Settings") }
        row.tap()
        sleep(2)
        let tile = app.buttons.matching(NSPredicate(format: "label == %@ OR label BEGINSWITH %@", name, name + ",")).firstMatch
        guard tile.waitForExistence(timeout: 10) else { return fail("no tile for \(name)") }
        tile.tap()
        sleep(4)
        _ = toSettingsRoot()
        record("switched to \(name)")
    }

    /// Pops pushed pages until Settings' profile row is on screen.
    @discardableResult private func toSettingsRoot() -> XCUIElement {
        let row = app.buttons.matching(NSPredicate(format: "label CONTAINS 'witch'")).firstMatch
        for _ in 0..<4 where !(row.exists && row.isHittable) { back() }
        return row
    }

    private func playing() -> Bool {
        let picture = app.descendants(matching: .any)["Show controls"]
        if !picture.waitForExistence(timeout: 30), app.buttons["Play"].exists { app.buttons["Play"].tap() }
        guard picture.waitForExistence(timeout: 120) else {
            shot("player-no-picture")
            XCTFail("the player never showed the picture")
            return false
        }
        return true
    }

    private func showControls() {
        if app.buttons["Back"].exists { return }
        let frame = app.frame
        app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: frame.width / 2, dy: frame.height * 0.4)).tap()
        _ = app.buttons["Back"].waitForExistence(timeout: 3)
    }

    /// A touch target's frame in points, recorded and held to 44 x 44.
    private func measure(_ name: String, _ element: XCUIElement) {
        guard element.waitForExistence(timeout: 5) else {
            tree("tree at \(name)")
            return fail("\(name): not found")
        }
        let frame = element.frame
        record("frame \(name): \(frame.width) x \(frame.height) at (\(frame.minX), \(frame.minY))")
        XCTAssertGreaterThanOrEqual(frame.width, 44, "\(name) width \(frame.width)")
        XCTAssertGreaterThanOrEqual(frame.height, 44, "\(name) height \(frame.height)")
    }

    private func record(_ line: String) {
        let note = XCTAttachment(string: line)
        note.name = "note: \(line.prefix(60))"
        note.lifetime = .keepAlways
        add(note)
        print("WALK \(line)")
    }

    private func fail(_ message: String) {
        shot("fail-\(message)")
        XCTFail(message)
    }

    private func shot(_ name: String) {
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
