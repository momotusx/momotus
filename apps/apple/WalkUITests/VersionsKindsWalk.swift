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
        let app = app
        app.launchArguments = ["-MQMute", "YES", "-MQOpenTitle", "movie:tt1727587"]
        app.launchArguments += (ProcessInfo.processInfo.environment["MQ_WALK_ARGS"] ?? "").split(separator: " ").map(String.init)
        app.launch()
        let allow = XCUIApplication(bundleIdentifier: "com.apple.springboard").buttons["Allow"]
        if allow.waitForExistence(timeout: 15) { allow.tap() }
        let browse = app.buttons["Browse first"]
        if browse.waitForExistence(timeout: 10) {
            browse.tap()
            sleep(5)
            app.terminate()
            app.launch()
        }
        let versions = app.buttons["Versions"]
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
    }

    private func shot(_ name: String) {
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
