import XCTest

/// The iPhone walk of plans/player-gestures.md 7.2: real touches on the walk build's player, which only XCUITest can
/// give a physical iPhone. Run from its own scheme (`MarqueeWalkUITests`), never part of an app's:
///   xcodebuild test -scheme MarqueeWalkUITests -destination id=<UDID> MARQUEE_APP_ID=app.marquee.walk
/// The walk install needs a source for Sintel first. Muted (`-MQMute YES`); upward volume drags raise the level by
/// at most 0.1 and the end stop is checked at 0; the walk build puts the volume it found back when the player closes,
/// and the brightness keeper the brightness. No drag starts in the top or bottom band: those are the system's and
/// would leave the app (test 24 covers them).
@MainActor final class PlayerGesturesWalk: XCTestCase {
    // The app under test; every instance reaches the same running app, so the helpers ask for their own.
    private var app: XCUIApplication { XCUIApplication() }

    func testSwipesTapsAndButtons() {
        continueAfterFailure = true
        let app = XCUIApplication()
        app.launchArguments = ["-MQMute", "YES", "-MQOpenTitle", "movie:tt1727587", "-MQAutoPlay", "YES"]
        app.launch()
        XCUIDevice.shared.orientation = .landscapeLeft
        let picture = app.descendants(matching: .any)["Show controls"]
        XCTAssertTrue(picture.waitForExistence(timeout: 90), "the player shows the picture")
        sleep(3)
        let height = app.windows.firstMatch.frame.height

        // Volume: up by at most 0.1 of the range (80 percent of the height is all of it), then down to the end stop.
        drag("volume up", at: CGPoint(x: 0.75, y: 0.6), by: CGVector(dx: 0, dy: -0.08 * height))
        drag("volume down to the end", at: CGPoint(x: 0.75, y: 0.15), by: CGVector(dx: 0, dy: 0.7 * height))
        // Brightness, both ways (the keeper puts it back on close).
        drag("brightness up", at: CGPoint(x: 0.25, y: 0.7), by: CGVector(dx: 0, dy: -0.4 * height))
        drag("brightness down", at: CGPoint(x: 0.25, y: 0.3), by: CGVector(dx: 0, dy: 0.3 * height))
        // From the side band, and diagonally: neither changes a level.
        drag("left band", at: CGPoint(x: 10 / app.windows.firstMatch.frame.width, y: 0.5), by: CGVector(dx: 0, dy: -0.3 * height))
        drag("diagonal", at: CGPoint(x: 0.75, y: 0.5), by: CGVector(dx: 60, dy: -60))

        // Double taps on either half seek 10 s; a single tap shows the controls after the double tap's window.
        point(CGPoint(x: 0.25, y: 0.5)).doubleTap()
        shot("double tap left")
        sleep(1)
        point(CGPoint(x: 0.75, y: 0.5)).doubleTap()
        shot("double tap right")
        sleep(1)
        point(CGPoint(x: 0.5, y: 0.4)).tap()
        sleep(1)
        shot("one tap")
        // With the controls up a drag on the position bar is the bar's.
        let bar = app.descendants(matching: .any)["Position"].firstMatch
        if bar.waitForExistence(timeout: 3) {
            let on = bar.coordinate(withNormalizedOffset: CGVector(dx: 0.75, dy: 0.5))
            on.press(forDuration: 0.05, thenDragTo: on.withOffset(CGVector(dx: 0, dy: -80)), withVelocity: 300, thenHoldForDuration: 0.2)
            shot("on the position bar")
        }

        // The hardware buttons: our indicator on the right, no system HUD. The two presses cancel out.
        XCUIDevice.shared.press(.volumeDown)
        shot("volume button down")
        XCUIDevice.shared.press(.volumeUp)
        shot("volume button up")

        sleep(2)
        if !app.buttons["Back"].exists { point(CGPoint(x: 0.5, y: 0.4)).tap() }
        if app.buttons["Back"].waitForExistence(timeout: 3) { app.buttons["Back"].tap() }
        sleep(2)
    }

    private func point(_ at: CGPoint) -> XCUICoordinate {
        let frame = app.windows.firstMatch.frame
        return app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: at.x * frame.width, dy: at.y * frame.height))
    }

    /// A drag at a set speed, held at its end, then a shot while the indicator still shows (it stays 800 ms).
    private func drag(_ name: String, at: CGPoint, by: CGVector) {
        let start = point(at)
        start.press(forDuration: 0.05, thenDragTo: start.withOffset(by), withVelocity: 400, thenHoldForDuration: 0.3)
        shot(name)
        sleep(2)
    }

    private func shot(_ name: String) {
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        shot.name = name
        shot.lifetime = .keepAlways
        add(shot)
    }
}
