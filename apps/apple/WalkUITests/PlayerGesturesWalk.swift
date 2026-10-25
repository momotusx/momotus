import AVFoundation
import XCTest

/// The iPhone walk of plans/player-gestures.md 7.2: real touches on the walk build's player, which only XCUITest can
/// give a physical iPhone. Run only through `apps/apple/scripts/walk-ui.sh <UDID>`, which builds the app under its own
/// bundle id; the target's build refuses any other id, and the test drives `app.marquee.walk` alone, so the owner's app
/// is never installed over or touched. The walk install needs a source for Sintel first. Muted (`-MQMute YES`); upward
/// volume drags raise the level by at most 0.1 and the end stop is checked at 0; the walk build puts the volume it found
/// back when the player closes, and the brightness keeper the brightness. No drag starts in the top or bottom band:
/// those are the system's and would leave the app (test 24 covers them).
@MainActor final class PlayerGesturesWalk: XCTestCase {
    static let walkApp = "app.marquee.walk"

    // Every instance reaches the same running app, so the helpers ask for their own.
    private var app: XCUIApplication { XCUIApplication(bundleIdentifier: Self.walkApp) }
    private var controlsShown: Bool { app.buttons["Back"].exists }
    /// The system brightness, read from the runner (it is the same for every app). The runner has no window scene of
    /// its own to ask, so this is the one place `UIScreen.main` is read.
    private var screenBrightness: CGFloat { UIScreen.main.brightness }

    func testSwipesTapsAndButtons() throws {
        // The wrapper names the walk app it built; anything else stops here, before a launch.
        let named = ProcessInfo.processInfo.environment["MQ_WALK_APP"]
        guard named == Self.walkApp else {
            XCTFail("run through scripts/walk-ui.sh: MQ_WALK_APP is \(named ?? "unset"), not \(Self.walkApp)")
            return
        }
        continueAfterFailure = true
        let app = app
        app.launchArguments = ["-MQMute", "YES", "-MQOpenTitle", "movie:tt1727587", "-MQAutoPlay", "YES"]
        // The walk install's source (MQ_WALK_ARGS from the wrapper, for example -MQInstallAddons <manifest URLs>).
        app.launchArguments += (ProcessInfo.processInfo.environment["MQ_WALK_ARGS"] ?? "").split(separator: " ").map(String.init)
        app.launch()
        // A fresh walk install asks for the local network (its test source is on the Mac) and opens on the first-run
        // choice: allow the one for this walk app, browse past the other, then launch again so the title opens and plays.
        let allow = XCUIApplication(bundleIdentifier: "com.apple.springboard").buttons["Allow"]
        if allow.waitForExistence(timeout: 15) { allow.tap() }
        let browse = app.buttons["Browse first"]
        if browse.waitForExistence(timeout: 10) {
            browse.tap()
            sleep(5)
            app.terminate()
            app.launch()
        }
        XCUIDevice.shared.orientation = .landscapeLeft
        let picture = app.descendants(matching: .any)["Show controls"]
        // The title page waits on Play when no version was picked by itself (the full film's file): press it.
        if !picture.waitForExistence(timeout: 30), app.buttons["Play"].exists { app.buttons["Play"].tap() }
        // No drag ever lands on anything but the playing picture.
        guard picture.waitForExistence(timeout: 120) else {
            shot("no picture")
            XCTFail("the player never showed the picture")
            return
        }
        // The controls hide by themselves after 4 s.
        sleep(6)
        XCTAssertFalse(controlsShown, "the controls hid")
        let height = app.frame.height
        // What the phone showed before the walk's first swipe: the brightness the player puts back, the volume the walk
        // build puts back on close.
        let walkStart = (brightness: screenBrightness, volume: AVAudioSession.sharedInstance().outputVolume)
        // Every drag is measured against the app's frame, across: anything else and no drag is sent.
        guard app.frame.width > app.frame.height, height > 200 else {
            shot("no frame across")
            XCTFail("the app's frame is not across: \(app.frame)")
            return
        }

        // Volume: up by at most 0.1 of the range (80 percent of the height is all of it), then down to the end stop.
        drag("volume up", at: CGPoint(x: 0.75, y: 0.6), by: CGVector(dx: 0, dy: -0.08 * height))
        drag("volume down to the end", at: CGPoint(x: 0.75, y: 0.15), by: CGVector(dx: 0, dy: 0.7 * height))
        // Brightness, both ways (the keeper puts it back on close).
        drag("brightness up", at: CGPoint(x: 0.25, y: 0.7), by: CGVector(dx: 0, dy: -0.4 * height))
        drag("brightness down", at: CGPoint(x: 0.25, y: 0.3), by: CGVector(dx: 0, dy: 0.3 * height))
        // From the side band, and diagonally: neither changes a level. A diagonal drag stays a tap on the picture.
        drag("left band", at: CGPoint(x: 10 / app.frame.width, y: 0.5), by: CGVector(dx: 0, dy: -0.3 * height))
        drag("diagonal", at: CGPoint(x: 0.75, y: 0.5), by: CGVector(dx: 60, dy: -60), toggles: true)
        if controlsShown { point(CGPoint(x: 0.5, y: 0.4)).tap() }
        sleep(1)

        // Double taps on either half seek 10 s and leave the controls as they were; a single tap shows them after the
        // double tap's window.
        point(CGPoint(x: 0.25, y: 0.5)).doubleTap()
        shot("double tap left")
        sleep(1)
        point(CGPoint(x: 0.75, y: 0.5)).doubleTap()
        shot("double tap right")
        sleep(1)
        XCTAssertFalse(controlsShown, "a double tap does not show the controls")
        point(CGPoint(x: 0.5, y: 0.4)).tap()
        sleep(1)
        shot("one tap")
        XCTAssertTrue(controlsShown, "a tap shows the controls")
        // With the controls up a drag on the position bar is the bar's: the controls stay.
        let bar = app.descendants(matching: .any)["Position"].firstMatch
        if bar.waitForExistence(timeout: 3) {
            let on = bar.coordinate(withNormalizedOffset: CGVector(dx: 0.75, dy: 0.5))
            on.press(forDuration: 0.05, thenDragTo: on.withOffset(CGVector(dx: 0, dy: -80)), withVelocity: 300, thenHoldForDuration: 0.2)
            shot("on the position bar")
            // Read once the scrub has settled: right after it the Back button can be missing from the tree for a moment.
            XCTAssertTrue(app.buttons["Back"].waitForExistence(timeout: 2), "a drag on the bar leaves the controls up")
        } else {
            XCTFail("no position bar with the controls up")
        }

        // The hardware buttons: our indicator on the right, no system HUD. The two presses cancel out.
        XCUIDevice.shared.press(.volumeDown)
        shot("volume button down")
        XCUIDevice.shared.press(.volumeUp)
        shot("volume button up")

        // The brightness return from the background (code review R2-3): swipe it up, press Home, read it (it should be
        // back), return to the app, read it again. Last, since Home may take the film to Picture in Picture.
        sleep(6)
        let before = screenBrightness
        drag("brightness up before Home", at: CGPoint(x: 0.25, y: 0.7), by: CGVector(dx: 0, dy: -0.3 * height))
        let swiped = screenBrightness
        XCUIDevice.shared.press(.home)
        sleep(3)
        let away = screenBrightness
        app.activate()
        sleep(3)
        let back = screenBrightness
        let found = "brightness at the walk's start \(walkStart.brightness), before the last swipe \(before), swiped \(swiped), at Home \(away), back in the app \(back)"
        let note = XCTAttachment(string: found)
        note.name = "brightness return"
        note.lifetime = .keepAlways
        add(note)
        XCTAssertNotEqual(swiped, before, accuracy: 0.02, "the swipe moved the brightness: \(found)")
        // The player puts back what it found before its first brightness swipe: the brightness at the walk's start.
        XCTAssertEqual(away, walkStart.brightness, accuracy: 0.02, "Home put the walk's first brightness back: \(found)")
        XCTAssertEqual(back, walkStart.brightness, accuracy: 0.02, "and coming back left it there: \(found)")

        sleep(2)
        if !controlsShown { point(CGPoint(x: 0.5, y: 0.4)).tap() }
        if app.buttons["Back"].waitForExistence(timeout: 3) { app.buttons["Back"].tap() }
        sleep(2)
        // The walk build put back on close the volume it found at its first volume swipe: the walk's start.
        let end = AVAudioSession.sharedInstance().outputVolume
        let levels = XCTAttachment(string: "volume at the walk's start \(walkStart.volume), after close \(end)")
        levels.name = "volume return"
        levels.lifetime = .keepAlways
        add(levels)
        XCTAssertEqual(end, walkStart.volume, accuracy: 0.01, "the walk left the volume as it found it")
    }

    private func point(_ at: CGPoint) -> XCUICoordinate {
        let frame = app.frame
        return app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: at.x * frame.width, dy: at.y * frame.height))
    }

    /// A drag at a set speed, held at its end, then a shot while the indicator still shows (it stays 800 ms). A swipe
    /// never toggles the controls; a drag that never commits stays a tap and does.
    private func drag(_ name: String, at: CGPoint, by: CGVector, toggles: Bool = false) {
        let before = controlsShown
        let start = point(at)
        start.press(forDuration: 0.05, thenDragTo: start.withOffset(by), withVelocity: 400, thenHoldForDuration: 0.3)
        shot(name)
        sleep(2)
        if !toggles { XCTAssertEqual(controlsShown, before, "\(name): the controls stay as they were") }
    }

    private func shot(_ name: String) {
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        shot.name = name
        shot.lifetime = .keepAlways
        add(shot)
    }
}
