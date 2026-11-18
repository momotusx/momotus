import XCTest

/// The Zap walk of plans/zap.md 3 on the owner's iPhone, muted: Zap from Home's hero, the name hidden until What is
/// this?, Zap again, and the dial. Run through scripts/walk-ui.sh with -only-testing:MarqueeWalkUITests/ZapWalk.
@MainActor final class ZapWalk: XCTestCase {
    static let walkApp = "app.momotus.walk"

    private var app: XCUIApplication { XCUIApplication(bundleIdentifier: Self.walkApp) }

    func testZap() throws {
        let named = ProcessInfo.processInfo.environment["MQ_WALK_APP"]
        guard named == Self.walkApp else {
            XCTFail("run through scripts/walk-ui.sh: MQ_WALK_APP is \(named ?? "unset"), not \(Self.walkApp)")
            return
        }
        continueAfterFailure = true
        addTeardownBlock { @MainActor in XCUIDevice.shared.orientation = .portrait }
        addUIInterruptionMonitor(withDescription: "Local Network") { alert in
            let allow = alert.buttons["Allow"]
            guard allow.exists else { return false }
            allow.tap()
            return true
        }
        let app = app
        app.launchArguments = ["-MQMute", "YES"]
        app.launch()
        let allow = XCUIApplication(bundleIdentifier: "com.apple.springboard").buttons["Allow"]
        if allow.waitForExistence(timeout: 15) { allow.tap() }
        app.passFirstRun(timeout: 10)
        let zap = app.buttons["Zap"].firstMatch
        app.swipeDown()
        guard zap.waitForExistence(timeout: 60) else {
            shot("no Zap on Home")
            XCTFail("Home never showed Zap")
            return
        }
        shot("home")
        for round in 1...3 {
            let pressed = Date()
            if round == 1 { zap.tap() } else {
                let again = app.buttons["Zap again"].firstMatch
                showControls(for: again)
                guard again.waitForExistence(timeout: 10) else {
                    shot("no Zap again \(round)")
                    XCTFail("the zap play showed no Zap again")
                    return
                }
                again.tap()
            }
            let what = app.buttons["What is this?"].firstMatch
            guard what.waitForExistence(timeout: 150) else {
                shot("zap \(round) never played")
                XCTFail("zap \(round): no What is this? in 150 s")
                continue
            }
            print("zap \(round): What is this? after \(Int(Date().timeIntervalSince(pressed))) s")
            sleep(8)
            shot("zap \(round) playing")
            // The top line reads Zap, not the title.
            showControls(for: app.staticTexts["Zap"].firstMatch)
            XCTAssertTrue(app.staticTexts["Zap"].exists, "zap \(round): the top line is not Zap")
            if round == 1 {
                showControls(for: what)
                what.tap()
                sleep(2)
                shot("zap 1 revealed")
                XCTAssertTrue(app.buttons["Start from the beginning"].firstMatch.isHittable, "Start from the beginning is not reachable")
                XCTAssertTrue(app.buttons["Open title"].firstMatch.isHittable, "Open title is not reachable")
            }
        }
        app.terminate()
    }

    /// A tap on the picture toggles the controls: only when `control` is not reachable, so it never hides them.
    private func showControls(for control: XCUIElement) {
        if control.exists && control.isHittable { return }
        // Hidden controls leave one full-screen "Show controls" button; shown, it is not there.
        let show = app.buttons["Show controls"].firstMatch
        if show.exists { show.tap() }
        _ = control.waitForExistence(timeout: 5)
    }

    private func shot(_ name: String) {
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
