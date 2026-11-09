import XCTest

/// The iPhone walk of the kind chips (plans/plugins-and-marketplace.md 4.3): Sintel's Versions with the torrents the
/// walk install's engine fetches and the direct links of a source on the Mac, then each kind chip. Run only through
/// `apps/apple/scripts/walk-ui.sh <UDID> -only-testing:MarqueeWalkUITests/VersionsKindsWalk`, which builds the app
/// under its own bundle id. Muted (`-MQMute YES`); nothing plays.
@MainActor final class VersionsKindsWalk: XCTestCase {
    static let walkApp = "app.momotus.walk"

    private var app: XCUIApplication { XCUIApplication(bundleIdentifier: Self.walkApp) }

    func testKindChips() throws {
        let named = ProcessInfo.processInfo.environment["MQ_WALK_APP"]
        guard named == Self.walkApp else {
            XCTFail("run through scripts/walk-ui.sh: MQ_WALK_APP is \(named ?? "unset"), not \(Self.walkApp)")
            return
        }
        continueAfterFailure = true
        // A failed step never leaves the phone turned.
        addTeardownBlock { @MainActor in XCUIDevice.shared.orientation = .portrait }
        // The Local Network prompt (the Sintel source sits on this Mac) is allowed whenever it shows.
        addUIInterruptionMonitor(withDescription: "Local Network") { alert in
            let allow = alert.buttons["Allow"]
            guard allow.exists else { return false }
            allow.tap()
            return true
        }
        let app = app
        app.launchArguments = ["-MQMute", "YES", "-MQOpenTitle", "movie:tt1727587"]
        app.launchArguments += (ProcessInfo.processInfo.environment["MQ_WALK_ARGS"] ?? "").split(separator: " ").map(String.init)
        app.launch()
        let allow = XCUIApplication(bundleIdentifier: "com.apple.springboard").buttons["Allow"]
        if allow.waitForExistence(timeout: 15) { allow.tap() }
        if app.passFirstRun(timeout: 10) {
            sleep(5)
            app.terminate()
            app.launch()
        }
        let versions = app.buttons["Versions"]
        // An interruption monitor runs only when the test touches the app.
        app.swipeUp()
        app.swipeDown()
        guard versions.waitForExistence(timeout: 60) else {
            shot("no Versions button")
            XCTFail("the title page never showed Versions")
            return
        }
        versions.tap()
        // Every source answers, then the chips: All and each kind listed.
        let all = app.buttons["All"]
        guard all.waitForExistence(timeout: 90) else {
            shot("no kind chips")
            XCTFail("no kind chips: fewer than two kinds listed")
            return
        }
        sleep(8)
        shot("versions-kinds-all")
        let labels = app.buttons.allElementsBoundByIndex.map(\.label).filter { $0.contains(" version") && $0.contains(", ") }
        print("walk chips: \(labels)")
        for (prefix, kicker, name) in [("Torrents, ", "Best torrent", "torrent"), ("Direct links, ", "Best direct link", "direct")] {
            let chip = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", prefix)).firstMatch
            guard chip.exists else {
                XCTFail("no \(prefix) chip")
                continue
            }
            chip.tap()
            XCTAssertTrue(app.staticTexts[kicker].waitForExistence(timeout: 5), "the card says \(kicker)")
            XCTAssertTrue(chip.isSelected, "\(prefix) is the chip that is on")
            sleep(1)
            shot("versions-kinds-\(name)")
        }
        // Turned with a kind chosen: the chip that is on stays on and in reach, both ways.
        let direct = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", "Direct links, ")).firstMatch
        for orientation in [UIDeviceOrientation.landscapeLeft, .portrait] {
            XCUIDevice.shared.orientation = orientation
            sleep(2)
            // A turned phone is short: the card fills it and the chips sit under the fold, so the page scrolls
            // them up as a viewer would. In reach then means the track itself keeps the chip on in view.
            for _ in 0..<3 where !direct.isHittable { app.swipeUp() }
            XCTAssertTrue(direct.isSelected, "Direct links stays on after turning")
            XCTAssertTrue(direct.isHittable, "Direct links is in reach after turning")
            shot("versions-kinds-turned-\(orientation == .portrait ? "portrait" : "landscape")")
        }
        // Back on the title page every kind is listed again: Versions opens on All.
        let back = app.navigationBars.buttons.element(boundBy: 0)
        if back.exists {
            back.tap()
        } else {
            // The system's edge swipe.
            app.coordinate(withNormalizedOffset: CGVector(dx: 0.01, dy: 0.5))
                .press(forDuration: 0.05, thenDragTo: app.coordinate(withNormalizedOffset: CGVector(dx: 0.8, dy: 0.5)))
        }
        XCTAssertTrue(versions.waitForExistence(timeout: 10), "back on the title page")
        versions.tap()
        XCTAssertTrue(all.waitForExistence(timeout: 30), "the chips again")
        XCTAssertTrue(all.isSelected, "the title page listed every kind again")
        shot("versions-kinds-back-all")
    }

    private func shot(_ name: String) {
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
