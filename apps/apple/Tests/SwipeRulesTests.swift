import CoreGraphics
import Foundation
import Testing

// The level swipes' rules on iPhone and iPad (plans/player-gestures.md, tests 24 to 28).

private let size = CGSize(width: 1000, height: 400)
private let bands = SwipeRules.Bands(left: 24, top: 32, right: 24, bottom: 32)

@Test func theStartPointPicksTheSideAndTheEdgesAreTheSystems() {
    #expect(SwipeRules.side(start: CGPoint(x: 490, y: 200), size: size, bands: bands) == .brightness)
    #expect(SwipeRules.side(start: CGPoint(x: 500, y: 200), size: size, bands: bands) == .volume)
    #expect(SwipeRules.side(start: CGPoint(x: 20, y: 200), size: size, bands: bands) == nil)
    #expect(SwipeRules.side(start: CGPoint(x: 980, y: 200), size: size, bands: bands) == nil)
    #expect(SwipeRules.side(start: CGPoint(x: 300, y: 20), size: size, bands: bands) == nil)
    #expect(SwipeRules.side(start: CGPoint(x: 700, y: 380), size: size, bands: bands) == nil)
}

@Test func theBandsTakeTheSafeArea() {
    #expect(SwipeRules.bands(safeTop: 0, safeBottom: 21) == SwipeRules.Bands(left: 24, top: 32, right: 24, bottom: 32))
    #expect(SwipeRules.bands(safeTop: 47, safeBottom: 34) == SwipeRules.Bands(left: 24, top: 47, right: 24, bottom: 42))
}

@Test func aTouchCommitsOnlyWhenItGoesUpOrDownTwiceAsFarAsAcross() {
    #expect(SwipeRules.commits(dx: 3, dy: 12, threshold: 12) == true)
    #expect(SwipeRules.commits(dx: 7, dy: 12, threshold: 12) == nil)
    #expect(SwipeRules.commits(dx: 12, dy: 5, threshold: 12) == false)
    #expect(SwipeRules.commits(dx: 2, dy: 11, threshold: 12) == nil)
    // iOS asks at the pan's own slop: only the 2:1 decides.
    #expect(SwipeRules.commits(dx: 4, dy: -9, threshold: 0) == true)
    #expect(SwipeRules.commits(dx: 6, dy: 9, threshold: 0) == false)
}

@Test func eightyPercentOfTheHeightSweepsTheWholeRange() {
    #expect(SwipeRules.level(start: 0.2, dy: -0.8 * 400, height: 400) == 1)
    #expect(SwipeRules.level(start: 0.5, dy: 0.4 * 400, height: 400) == 0)
    #expect(abs(SwipeRules.level(start: 0.5, dy: -0.2 * 400, height: 400) - 0.75) < 1e-9)
}

@Test func theTickComesOnceAtEitherEnd() {
    #expect(SwipeRules.reachedEnd(before: 0.98, after: 1))
    #expect(!SwipeRules.reachedEnd(before: 1, after: 1))
    #expect(SwipeRules.reachedEnd(before: 0.02, after: 0))
    #expect(!SwipeRules.reachedEnd(before: 0.4, after: 0.5))
}

@Test func theBrightnessGoesBackOnlyOverTheValueThePlayerSet() {
    #expect(SwipeRules.shouldRestore(current: 0.41, set: 0.40))
    #expect(!SwipeRules.shouldRestore(current: 0.70, set: 0.40))
    // Found 0.50, swiped to 0.80, Control Center to 0.30: the next swipe keeps the viewer's 0.30 as the one to put back.
    #expect(SwipeRules.found(current: 0.30, set: 0.80, found: 0.50) == 0.30)
    #expect(SwipeRules.found(current: 0.79, set: 0.80, found: 0.50) == 0.50)
    #expect(SwipeRules.found(current: 0.60, set: nil, found: nil) == 0.60)
}

@Test func theSwipesTakeThePictureOnlyWhenNothingElseIsOverIt() {
    func active(
        phone: Bool = true, across: Bool = true, status: PlayerStatus = .none, endCard: Bool = false,
        picker: Bool = false, versions: Bool = false, floating: Bool = false, assistive: Bool = false
    ) -> Bool {
        SwipeRules.active(
            phone: phone, across: across, status: status, endCard: endCard, pickerOpen: picker, versions: versions,
            floating: floating, assistive: assistive)
    }
    #expect(active())
    #expect(!active(across: false), "upright on an iPhone the picture is a small box")
    #expect(active(phone: false, across: false), "an iPad takes them either way")
    #expect(active(status: .waiting(words: false)), "a stall mid-play keeps them")
    for status in [PlayerStatus.failed, .noPick, .cannotOpen, .waiting(words: true)] {
        #expect(!active(status: status), "\(status)")
    }
    #expect(!active(endCard: true))
    #expect(!active(picker: true))
    #expect(!active(versions: true))
    #expect(!active(floating: true))
    #expect(!active(assistive: true))
}

@Test func theVolumeSwipeChecksThatItTookAndIgnoresItsOwnEcho() {
    #expect(SwipeRules.volumeTook(before: 0.5, set: 0.7, after: 0.69))
    #expect(!SwipeRules.volumeTook(before: 0.5, set: 0.7, after: 0.5))
    #expect(SwipeRules.isEcho(value: 0.705, lastSet: 0.70))
    #expect(!SwipeRules.isEcho(value: 0.75, lastSet: 0.70))
    #expect(!SwipeRules.isEcho(value: 0.5, lastSet: nil))
}

@Test func thePlayerWiresTheSwipesTheirOffRulesAndTheBrightnessReturn() throws {
    let dir = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Player")
    let screen = try String(contentsOf: dir.appending(path: "PlayerScreen.swift"), encoding: .utf8)
    let swipe = try String(contentsOf: dir.appending(path: "PlayerSwipe.swift"), encoding: .utf8)
    let view = try body(of: "var body: some View", in: screen)
    #expect(view.contains(".gesture(levelSwipe)"))
    #expect(try body(of: "private var swipesOn: Bool", in: screen).contains("SwipeRules.active("))
    #expect(try body(of: "private var levelSwipe: LevelSwipe", in: screen).contains("enabled: swipesOn"))
    // The system brightness goes back when the player leaves and when the app goes to the background.
    #expect(try body(of: ".onDisappear", in: screen).contains("keeper.restore()"))
    #expect(view.contains("if phase == .background || (phase == .active && keeper.pending) { keeper.restore() }"))
    // The return uses the screen the swipe held, and a return with no screen keeps its record.
    let keeper = try body(of: "@MainActor final class BrightnessKeeper", in: swipe)
    #expect(try body(of: "private var screen: UIScreen?", in: keeper).contains("if let held { return held }"))
    #expect(try body(of: "func start() -> Double?", in: keeper).contains("held = screen"))
    let restore = try body(of: "func restore()", in: keeper)
    #expect(restore.contains("SwipeRules.brightnessReturn("))
    #expect(restore.contains("SwipeRules.took(brightness:"))
    #expect(!restore.contains("defer"))
    // The volume swipe that turned off hides its indicator.
    #expect(try body(of: "private var levelSwipe: LevelSwipe", in: screen).contains("if volume.off {\n                        levels.hideNow()"))
    #expect(view.contains("if off, levels.side == .volume { levels.hideNow() }"))
    #expect(try body(of: "func set(_ value: Float)", in: swipe).contains("SwipeRules.checksVolume(before: before, set: value)"))
    // Under VoiceOver or Switch Control no volume view, so the system's HUD stays.
    #expect(view.contains("if !assistive {\n                    VolumeControl(volume: volume)"))
    // Direct touches only, and a second touch keeps the pan from beginning.
    #expect(try body(of: "func makeUIGestureRecognizer(", in: swipe).contains("allowedTouchTypes = [NSNumber(value: UITouch.TouchType.direct.rawValue)]"))
    let begin = try body(of: "func gestureRecognizerShouldBegin(", in: swipe)
    #expect(begin.contains("pan.numberOfTouches == 1"))
    #expect(begin.contains("SwipeRules.commits("))
}

@Test func aBrightnessReturnWithNoScreenKeepsItsRecord() {
    // Home pressed after a swipe: by the background phase no scene is active. With the swipe's screen held the put
    // goes through; with none at all the record stays for the next try, never cleared with nothing written.
    #expect(SwipeRules.brightnessReturn(found: 0.50, set: 0.90, current: nil) == .keep)
    #expect(SwipeRules.brightnessReturn(found: 0.50, set: 0.90, current: 0.90) == .put(0.50))
    #expect(SwipeRules.brightnessReturn(found: 0.50, set: 0.90, current: 0.30) == .forget)
    #expect(SwipeRules.brightnessReturn(found: nil, set: nil, current: 0.70) == .forget)
    // The record goes only once the screen shows the put: a write the system ignored from the background reads back
    // as the old value and is tried again.
    #expect(SwipeRules.took(brightness: 0.50, put: 0.50))
    #expect(!SwipeRules.took(brightness: 0.90, put: 0.50))
}

@Test func aSmallFirstVolumeSetIsNotTheReadBack() {
    // A first set a few thousandths from the start can round to the start; it must not turn the swipe off.
    #expect(!SwipeRules.checksVolume(before: 0.5, set: 0.503))
    #expect(SwipeRules.checksVolume(before: 0.5, set: 0.5 + 1.0 / 16))
    #expect(SwipeRules.checksVolume(before: 0.5, set: 0.4))
}
