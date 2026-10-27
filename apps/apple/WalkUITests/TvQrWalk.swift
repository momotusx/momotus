import XCTest

/// The iPhone walk of plans/tv-qr-login.md section 8: a screen's pairing code reaches the walk build by
/// `momotus://pair#...` (what the link host's Open in Momotus hands over, as the system camera's link would), and one
/// tap signs that screen in. Run only through `plans/tv-qr-walk-ios.sh`, which starts a relay on the Mac, a CLI screen
/// showing a code (MQ_WALK_CODE) and builds the app as `app.momotus.walk`; the owner's app is never touched. Muted.
@MainActor final class TvQrWalk: XCTestCase {
    static let walkApp = "app.momotus.walk"

    private var app: XCUIApplication { XCUIApplication(bundleIdentifier: Self.walkApp) }

    func testALinkedCodeSignsTheScreenInWithOneTap() throws {
        let env = ProcessInfo.processInfo.environment
        guard env["MQ_WALK_APP"] == Self.walkApp, let server = env["MQ_WALK_SERVER"], let code = env["MQ_WALK_CODE"],
              let url = URL(string: code.replacingOccurrences(of: "https://momotus.normalisdead.mom/pair#", with: "momotus://pair#"))
        else {
            XCTFail("run through plans/tv-qr-walk-ios.sh")
            return
        }
        continueAfterFailure = false
        let app = app
        app.launchArguments = ["-MQMute", "YES", "-MQSyncServer", server]
        app.launch()
        // The relay of this walk is on the Mac: the walk app alone asks for the local network.
        let allow = XCUIApplication(bundleIdentifier: "com.apple.springboard").buttons["Allow"]
        if allow.waitForExistence(timeout: 10) { allow.tap() }
        // First run on a sync service with no provider: Create at once (plans/tv-qr-login.md 5.4).
        let create = app.buttons["Create your Momotus account"]
        XCTAssertTrue(create.waitForExistence(timeout: 30), "the first run offers Create")
        shot("first run")
        create.tap()
        // No passkey on a build without associated domains: the second-way step, then Later.
        let later = app.buttons["Later"]
        if later.waitForExistence(timeout: 30) { later.tap() }
        sleep(3)
        // The code by link: the scene delegate takes it, the sheet asks once.
        let start = Date()
        app.open(url)
        let ask = app.staticTexts["Sign in this device?"]
        XCTAssertTrue(ask.waitForExistence(timeout: 20), "the sheet asks to sign the screen in")
        let toSheet = Date().timeIntervalSince(start)
        shot("sheet")
        app.buttons["Sign in"].tap()
        let done = app.staticTexts["The other device is signed in"]
        XCTAssertTrue(done.waitForExistence(timeout: 60), "the screen joined")
        print("walk: link to sheet \(Int(toSheet * 1000)) ms")
        shot("done")
    }

    private func shot(_ name: String) {
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
