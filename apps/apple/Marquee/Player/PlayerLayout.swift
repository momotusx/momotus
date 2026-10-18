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

    /// How far above the foot Skip and Up Next sit: just over the controls'
    /// measured height while they show, else near the foot.
    static func overlayBottom(controlsShown: Bool, controlsHeight: CGFloat) -> CGFloat {
        controlsShown ? controlsHeight + 8 : 24
    }
}
