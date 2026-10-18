import CoreGraphics

/// Where the phone player puts its parts, as Android settled after three
/// review rounds (plans/android-design-v2.md, slices 14 to 16). Heights are
/// the real window's, bars included, since the player hides them.
enum PlayerLayout {
    /// Below this the transport joins the actions' row, so the controls stay
    /// clear of the title and the status line; from it the transport keeps its
    /// centered place above the scrubber.
    static let shortHeight: CGFloat = 360
    /// Below this Up Next and the controls do not show together.
    static let upNextRoom: CGFloat = 420

    static func joinsTransport(windowHeight: CGFloat) -> Bool {
        windowHeight > 0 && windowHeight < shortHeight
    }

    /// Up Next shows when it is due, unless a track menu is open, or the
    /// controls are up on a window too short for both: it waits until they
    /// hide, as they do on their timer while the episode plays.
    static func showsUpNext(due: Bool, controlsShown: Bool, menuOpen: Bool, windowHeight: CGFloat) -> Bool {
        due && !menuOpen && (!controlsShown || windowHeight >= upNextRoom)
    }

    /// The longest an open menu holds the controls up and Up Next back.
    static let menuHoldLimit: Duration = .seconds(10)

    /// An OSD menu holds from when it opens until a pick or the next touch
    /// clears `openedAt`. iOS never says when a menu closes, so one dismissed
    /// by a tap outside lets go at the limit.
    static func menuHolds(openedAt: ContinuousClock.Instant?, now: ContinuousClock.Instant) -> Bool {
        guard let openedAt else { return false }
        return now - openedAt < menuHoldLimit
    }

    /// How far above the foot Skip and Up Next sit: just over the controls'
    /// measured height while they show, else near the foot.
    static func overlayBottom(controlsShown: Bool, controlsHeight: CGFloat) -> CGFloat {
        controlsShown ? controlsHeight + 8 : 24
    }
}

/// Seeking on the TV's position bar: 10 s a press, growing while the same
/// direction is pressed again quickly, so twenty minutes is about fifteen
/// presses rather than 120.
enum SeekRamp {
    /// Seconds a press moves, by how many quick presses came before it in that direction.
    static let steps: [Double] = [10, 10, 20, 30, 60, 120]
    /// Presses closer than this continue a run.
    static let window: Duration = .seconds(1)

    static func step(_ run: Int) -> Double { steps[min(max(run, 0), steps.count - 1)] }

    /// The run a press starts or continues: the same direction within the window goes on, anything else starts over.
    static func run(after previous: Int, continuing: Bool) -> Int { continuing ? previous + 1 : 0 }
}
