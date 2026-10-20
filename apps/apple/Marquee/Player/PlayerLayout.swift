import CoreGraphics
import Foundation

/// Where the phone player puts its parts, as Android settled after three
/// review rounds (plans/android-design-v2.md, slices 14 to 16). Heights are
/// the real window's, bars included, since the player hides them.
enum FailureAction: Hashable { case offer, retry, choose, back }

/// What the player shows over the picture, first match.
enum PlayerStatus: Equatable {
    case failed
    case cannotOpen
    case waiting(words: Bool)
    case none
}

enum PlayerLayout {
    /// What a snapshot's subtitle notes do to the picked file, as the web
    /// does: only the picked file is fetched, so a note new to this snapshot
    /// is about it and is said; when the file also left the list (it could
    /// not be read) subtitles go off, and nothing else changes. Nil without
    /// a picked file or a new note: a failover empties both lists without a
    /// note, and the file stays picked.
    static func subtitleTrouble(
        picked: SubtitleTrack?, old: [SubtitleNote], new: [SubtitleNote], tracks: [SubtitleTrack]
    ) -> SubtitleTrouble? {
        guard let picked, let note = SubtitleNote.added(from: old, to: new).last else { return nil }
        return SubtitleTrouble(say: note.words, off: !tracks.contains { $0.url == picked.url })
    }

    /// The picture never came while the sound runs: AVFoundation plays an HEVC
    /// file tagged hev1 (and any track it cannot show) as audio over a black
    /// layer, with no error. More than 4 s of playhead or of playing without a
    /// frame means this version's picture cannot be shown here.
    static func pictureMissing(shown: Bool, advanced: Double, playingFor: Double) -> Bool {
        !shown && (advanced > pictureWait || playingFor > pictureWait)
    }

    static let pictureWait: Double = 4

    /// The Subtitles button: the picked track, else the count as soon as one
    /// is in, else Searching while the core still lists, else None found. The
    /// mark says more may come: it shows while the core lists and some are in.
    /// The failure card's actions, the first the primary: the core's next
    /// version when it offers one, then Try again (the same versions from the
    /// first), then the way to another version, then Back.
    /// The failure card's headline: with a version offered, only this one
    /// failed, so "None of the sources" would contradict Play {label} below it.
    /// The overlay: the failure card, the card for a view that could not
    /// open, the spinner (with its words until a frame shows, and whenever the
    /// play starts over, a Try again after a frame), or nothing.
    static func status(phase: String?, cannotOpen: Bool, firstFrameShown: Bool, waiting: Bool) -> PlayerStatus {
        if phase == "failed" { return .failed }
        if cannotOpen { return .cannotOpen }
        if phase == "resolving" || !firstFrameShown { return .waiting(words: true) }
        return waiting ? .waiting(words: false) : .none
    }

    static let cannotOpenHeadline = "This version could not be opened"

    /// AirPlay may take a stream only when another device can reach it: never
    /// the core's loopback server (review apple-journey P2-5).
    static func allowsExternalPlayback(_ url: URL) -> Bool {
        guard let host = url.host()?.lowercased() else { return false }
        return !(host == "localhost" || host == "::1" || host == "[::1]" || host.hasPrefix("127."))
    }

    /// Subtitle timing moves in half seconds, kept for the play (review
    /// apple-journey P2-6).
    static let subtitleStep = 0.5

    /// Where the cues are read: a positive delay shows each line later.
    static func cueTime(position: Double, delay: Double) -> Double {
        position - delay
    }

    /// The times the drawn line changes: each cue's start and end, shifted by
    /// the timing, from the film's start, each once and in order.
    static func cueEdges(_ cues: [WebVTT.Cue], delay: Double) -> [Double] {
        Array(Set(cues.flatMap { [$0.start + delay, $0.end + delay] }.filter { $0 >= 0 })).sorted()
    }

    /// "On time", "1.5 s later", "0.5 s earlier".
    static func delayWords(_ delay: Double) -> String {
        guard abs(delay) >= 0.05 else { return "On time" }
        let amount = String(format: "%g", abs(delay))
        return delay > 0 ? "\(amount) s later" : "\(amount) s earlier"
    }

    /// A film brought back from Picture in Picture while it played is playing
    /// again once the window has closed; one paused in the window stays paused.
    static func resumesAfterRestore(playingAtRestore: Bool, playingNow: Bool) -> Bool {
        playingAtRestore && !playingNow
    }

    /// Picture in Picture shows the picture only: an addon's subtitles are
    /// drawn over the page, which has left. Said once per film.
    static func pipSubtitleNote(addonSubtitle: Bool, told: Bool) -> String? {
        addonSubtitle && !told ? "Addon subtitles do not show in Picture in Picture" : nil
    }

    /// The core took a Try again: `retries` rose within one view. The first
    /// snapshot of a view counts nothing (review r2 P3-6).
    static func retryRose(old: Int?, new: Int?) -> Bool {
        guard let old, let new else { return false }
        return new > old
    }

    /// "Trying again" holds at least this long after `retries` rises, as on
    /// the web and Android, unless the play is ready first.
    static let tryingAgainHold: Duration = .milliseconds(1500)

    /// The line keeps showing until the hold ends or the play is ready.
    static func keepsTryingAgain(showing: Bool, phase: String?) -> Bool {
        showing && phase != "ready"
    }

    /// On a refail the card stays, with this line under its headline.
    static func failureNote(tryingAgain: Bool) -> String? {
        tryingAgain ? "Trying again" : nil
    }

    /// The action the card focuses as it shows: always the first, Play
    /// {offer} when there is one, as on the web and Android (review r4 P3-5).
    static func failureFocus(_ order: [FailureAction]) -> FailureAction? {
        order.first
    }

    /// The failure card's buttons, in the words the web and Android use.
    static func failureLabel(_ action: FailureAction, offer: String?) -> String {
        switch action {
        case .offer: "Play \(offer ?? "the next version")"
        case .retry: "Try again"
        case .choose: "Choose a version"
        case .back: "Back"
        }
    }

    static func failureHeadline(offer: Bool) -> String {
        offer ? "This version could not be played" : "None of the sources could be played"
    }

    static func failureActions(offer: Bool, canChoose: Bool) -> [FailureAction] {
        (offer ? [.offer] : []) + [.retry] + (canChoose ? [.choose] : []) + [.back]
    }

    /// A view that could not open: mostly "that version is no longer listed",
    /// where Try again opens the same key again, so another version comes
    /// first; Try again stays for a lost subscription (review r4 P3-6).
    static func cannotOpenActions(canChoose: Bool) -> [FailureAction] {
        (canChoose ? [.choose] : []) + [.retry, .back]
    }

    static func subtitleValue(picked: String?, found: Int, pending: Bool) -> (text: String, searching: Bool) {
        let searching = pending && found > 0
        if let picked { return (picked, searching) }
        if found == 0 { return (pending ? "Searching" : "None found", false) }
        return ("Off, \(found) found", searching)
    }

    /// Under the spinner: the version the core moves on to while it resolves,
    /// else the player's own word on a failed source, else the plain wait.
    static func waitingWords(_ playback: PlaybackSnapshot?, notice: String?, tryingAgain: Bool = false) -> String {
        // A Try again that moves on at once names the version it moved to.
        if playback?.phase == "resolving", let label = playback?.trying?.label {
            return "Trying the next version: \(label)"
        }
        if tryingAgain { return "Trying again" }
        return notice ?? "Getting the stream ready"
    }

    /// What `progress` tells Trakt when the player's state changes: a pause, or a
    /// play after one, from anywhere (the Lock Screen, a call); buffering is
    /// neither. Nil when there is nothing new to say.
    static func traktPaused(paused: Bool, buffering: Bool, was: Bool) -> Bool? {
        guard !buffering, paused != was else { return nil }
        return paused
    }

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

    /// The longest an open menu holds the controls up and Up Next back. Long
    /// enough to read and choose in it: at 10 s an open menu went with the
    /// hiding controls (the iPhone walk, round 2). iOS never says when a menu
    /// closes, so one dismissed by a tap outside keeps the controls up this
    /// long, unless the player is touched first.
    // ponytail: a fixed cap; a UIKit menu with willEnd would end the hold exactly.
    static let menuHoldLimit: Duration = .seconds(60)

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

    /// The waiting status sits centred, unless the shown controls reach the
    /// middle (a phone in landscape): then it sits just over them, in one
    /// line, and stays in sight, since the viewer tapped to see what is
    /// happening. Nil keeps it centred; else its bottom padding.
    static func statusOverControls(controlsShown: Bool, windowHeight: CGFloat, controlsHeight: CGFloat) -> CGFloat? {
        guard controlsShown, windowHeight > 0, controlsHeight > windowHeight / 2 - statusHalfHeight else { return nil }
        return overlayBottom(controlsShown: true, controlsHeight: controlsHeight)
    }

    /// Half the centred status: the large spinner, its gap and one line.
    static let statusHalfHeight: CGFloat = 48
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
