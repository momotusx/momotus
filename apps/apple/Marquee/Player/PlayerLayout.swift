import CoreGraphics
import Foundation
import SwiftUI

/// Where the phone player puts its parts, as Android settled after three
/// review rounds (plans/android-design-v2.md, slices 14 to 16). Heights are
/// the real window's, bars included, since the player hides them.
enum FailureAction: Hashable { case account, offer, retry, choose, back }

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
    /// frame means this version's picture cannot be shown here. Only while
    /// a frame watch looks (the page's view, or the Picture in Picture
    /// window's layer): with none, a missing frame says nothing about the
    /// version (review apple-journey r3 P1-1).
    static func pictureMissing(shown: Bool, advanced: Double, playingFor: Double, watched: Bool = true) -> Bool {
        watched && !shown && (advanced > pictureWait || playingFor > pictureWait)
    }

    /// The failure kind a missing picture reports. On the page it is the
    /// version's own ("decode"), which the core keeps as did not play on this
    /// device; in the Picture in Picture window it only moves to the next
    /// version, as a window's frame says less about the device (review
    /// apple-journey r3 P1-1).
    static func pictureFailureKind(floating: Bool) -> String {
        floating ? "picture" : "decode"
    }

    static let pictureWait: Double = 4

    /// Whether a frame watch looks at the picture now: in the Picture in
    /// Picture window its own watch; on the page only while the app is on
    /// screen. In the background a layer never shows a frame, so a source that
    /// loaded there would fail as this version's own (the iPhone walk, round
    /// 3: another app in front while Pioneer One loaded).
    static func pictureWatched(floating: Bool, windowWatches: Bool, onScreen: Bool) -> Bool {
        floating ? windowWatches : onScreen
    }

    /// Time with no watch never counts toward a missing picture: the clocks
    /// start again from here, so a source needs 4 s in sight without a frame.
    static func pictureClocks(
        watched: Bool, position: Double, loadStart: Double, playingSince: ContinuousClock.Instant?, now: ContinuousClock.Instant
    ) -> (loadStart: Double, playingSince: ContinuousClock.Instant?) {
        watched ? (loadStart, playingSince) : (position, playingSince.map { _ in now })
    }

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

    /// The subtitle timing stepper, as Stremio's (stremio-web subtitleDelay.ts; the web's lib/delay.ts
    /// and Android's SubtitleTiming.kt): 0.1 s a press; held, the first repeat comes at 350 ms, then one
    /// every 100 ms moving 0.1 s times min(whole seconds held + 3, 6); within 60 s either way. Plus
    /// shows the words later. The core keeps the value per track once it rests for 700 ms.
    static let subtitleStep = 0.1
    static let maxSubtitleDelay = 60.0
    static let firstRepeatMs = 350
    static let repeatMs = 100
    static let delaySettleMs = 700

    /// What one repeat moves after `heldMs` of holding: 0.3 s a tick in the first second, 0.4 and 0.5
    /// in the next two, 0.6 from the third on.
    static func holdStep(heldMs: Int) -> Double {
        subtitleStep * Double(min(heldMs / 1000 + 3, 6))
    }

    /// `value` moved by `by`, snapped to 0.1 s and kept within 60 s either way.
    static func nudge(_ value: Double, by: Double) -> Double {
        let next = ((value + by) * 10).rounded() / 10
        let kept = min(maxSubtitleDelay, max(-maxSubtitleDelay, next))
        return kept == 0 ? 0 : kept
    }

    /// Where a track's timing starts: where the viewer left it earlier in this play (`kept`, by URL),
    /// else where the core remembers it from the last play (`delayMs`, one track at a time); on time
    /// for none.
    static func startDelay(_ track: SubtitleTrack?, kept: [String: Double] = [:]) -> Double {
        guard let track else { return 0 }
        // The core's value to the tenth the stepper shows (a 40 ms delay is 0.0 s, with no Reset).
        return kept[track.url] ?? (Double(track.delayMs ?? 0) / 100).rounded() / 10
    }

    /// The core's `subtitleDelay` for a settled value: the track's URL and the value in ms.
    static func delayCommand(url: String, seconds: Double) -> [String: any Sendable] {
        ["type": "subtitleDelay", "url": url, "ms": Int((seconds * 1000).rounded())]
    }

    /// A key the stepper takes: one with Command, Option, Control or Shift belongs to someone else
    /// (Cmd-Left is no step).
    static func stepperKey(_ modifiers: EventModifiers) -> Bool {
        modifiers.isDisjoint(with: [.command, .option, .control, .shift])
    }

    /// The value as the stepper shows it: "+0.3 s", "\u{2212}1.2 s" (a true minus), "0.0 s".
    static func delayText(_ delay: Double) -> String {
        let sign = delay > 0 ? "+" : delay < 0 ? "\u{2212}" : ""
        return sign + String(format: "%.1f s", abs(delay))
    }

    /// Where the cues are read: a positive delay shows each line later.
    static func cueTime(position: Double, delay: Double) -> Double {
        position - delay
    }

    /// The times the drawn line changes: each cue's start and end, shifted by
    /// the timing, from the film's start, each once and in order.
    static func cueEdges(_ cues: [WebVTT.Cue], delay: Double) -> [Double] {
        Array(Set(cues.flatMap { [$0.start + delay, $0.end + delay] }.filter { $0 >= 0 })).sorted()
    }

    /// "Plus 1.5 seconds", "Minus 0.3 seconds", "0.0 seconds": the signed value as VoiceOver says it,
    /// once it settles. Never "earlier" or "later" (the owner, 2026-10-22).
    static func delayWords(_ delay: Double) -> String {
        let amount = String(format: "%.1f seconds", abs(delay))
        guard abs(delay) >= 0.05 else { return "0.0 seconds" }
        return delay > 0 ? "Plus \(amount)" : "Minus \(amount)"
    }

    /// The phone player opens across on a play's first appear; back from
    /// Versions or from the window it turns the way it was when it left, so a
    /// viewer watching upright stays upright (review apple-journey r3 P3-6).
    static func opensAcross(lastAcross: Bool?) -> Bool {
        lastAcross ?? true
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
        case .account: StreamChoice.words.blocked.needsAccount.action
        case .offer: "Play \(offer ?? "the next version")"
        case .retry: "Try again"
        case .choose: "Choose a version"
        case .back: "Back"
        }
    }

    static func failureHeadline(offer: Bool) -> String {
        offer ? "This version could not be played" : "None of the sources could be played"
    }

    /// Every version needs a streaming service account (`needsAccount`): adding one comes first, as
    /// the title page's primary and Versions say it; Choose a version stays beside it.
    static func failureActions(offer: Bool, canChoose: Bool, needsAccount: Bool = false) -> [FailureAction] {
        if needsAccount { return accountActions(canChoose: canChoose) }
        return (offer ? [.offer] : []) + [.retry] + (canChoose ? [.choose] : []) + [.back]
    }

    /// Every version needs an account: Add an account, then Choose a version, then Back, as the web
    /// and Android cards have it; no other source and no Try again can play one.
    static func accountActions(canChoose: Bool) -> [FailureAction] {
        [.account] + (canChoose ? [.choose] : []) + [.back]
    }

    /// The versions as they stand say every one needs a streaming service account.
    static func needsAccount(_ versions: VersionsSnapshot?) -> Bool {
        versions?.done == true && versions?.recommended == nil && versions?.blocked?.reason == "needsAccount"
    }

    /// A view that could not open: mostly "that version is no longer listed",
    /// where Try again opens the same key again, so another version comes
    /// first; Try again stays for a lost subscription (review r4 P3-6).
    static func cannotOpenActions(canChoose: Bool, needsAccount: Bool = false) -> [FailureAction] {
        needsAccount ? accountActions(canChoose: canChoose) : (canChoose ? [.choose] : []) + [.retry, .back]
    }

    /// The Subtitles value. The core asks the subtitle addons once it has a
    /// stream (`ready`); before that it has not asked yet, so the value says
    /// Searching, never None found (the iPhone walk, round 3).
    static func subtitleValue(picked: String?, found: Int, pending: Bool, ready: Bool) -> (text: String, searching: Bool) {
        let asking = pending || !ready
        let searching = asking && found > 0
        if let picked { return (picked, searching) }
        if found == 0 { return (asking ? "Searching" : "None found", false) }
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

    /// The longest an open menu holds the controls up and Up Next back, from
    /// its opening or the last press in it. Long enough to read and choose: at
    /// 10 s an open menu went with the hiding controls (the iPhone walk, round
    /// 2). iOS never says when a menu closes, so one dismissed by a tap outside
    /// keeps the controls up this long, unless the player is touched first; a
    /// minute held them over the film too long (review apple-journey r3 P3-1).
    // ponytail: a fixed cap; a UIKit menu with willEnd would end the hold exactly.
    static let menuHoldLimit: Duration = .seconds(30)

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

    /// The iPhone, the controls up: the spinner turns in the Play circle, as
    /// the system player's does, and the words sit in one line 8 pt over the
    /// transport in the screen's middle (review apple-journey r3 P2-1). The
    /// words' bottom padding from the safe area's foot.
    static func statusOverTransport(windowHeight: CGFloat, safeBottom: CGFloat) -> CGFloat {
        windowHeight / 2 + PhoneFrame.transportHalfHeight + 8 - safeBottom
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

/// The iPhone player's frame, as the system player lays it out
/// (AVPlayerViewController on an iPhone 17 simulator, 2026-10-20): the bars at
/// least 10 pt from the screen's edge, at the safe area where it is wider; the
/// top bar 12 pt under the safe area's top; the scrubber and the actions at
/// the foot, 5 pt over the safe area's bottom; the transport in the screen's
/// middle, 36 pt between its buttons upright and 68 pt across.
enum PhoneFrame {
    static let edge: CGFloat = 10
    static let topGap: CGFloat = 12
    static let bottomGap: CGFloat = 5
    /// The scrubber's bar: the system's capsule height, its times 11 pt in.
    static let barHeight: CGFloat = 38
    static let barInset: CGFloat = 11

    /// Padding inside a safe area edge that keeps a bar `edge` from the screen.
    static func side(safe: CGFloat) -> CGFloat {
        max(edge - safe, 0)
    }

    static func transportGap(landscape: Bool) -> CGFloat {
        landscape ? 68 : 36
    }
}

extension PlayerLayout {
    /// Where a drawn subtitle's foot sits, from the screen's top: at the
    /// picture's foot, and over the controls' foot block (on the iPhone the
    /// scrubber and the actions) while they show, the safe area's foot counted:
    /// the lower part of the screen, never its top.
    static func subtitleFoot(screenHeight: CGFloat, pictureHeight: CGFloat, safeBottom: CGFloat, controlsShown: Bool, controlsHeight: CGFloat) -> CGFloat {
        let pictureFoot = (screenHeight + pictureHeight) / 2 - 16
        let clear = controlsShown ? safeBottom + controlsHeight + 16 : max(40, safeBottom + 24)
        return min(pictureFoot, screenHeight - clear)
    }
}

extension PhoneFrame {
    /// Half the transport's height (the play circle), and the room a subtitle
    /// keeps from it.
    static let transportHalfHeight: CGFloat = 34
    static let subtitleClearance: CGFloat = 4
    /// A subtitle's foot over the scrubber's capsule.
    static let subtitleGap: CGFloat = 8
    /// The size a subtitle takes while it would meet the transport.
    static let subtitleShrink: CGFloat = 0.85
    /// The smallest it ever draws: past it a long subtitle meets the
    /// transport's gaps rather than turning unreadable (review apple-journey
    /// r3 P3-2: two Extra Large lines drew at 42% on a 13 mini).
    static let subtitleFloor: CGFloat = 0.75

    /// The gap between the picture's foot and a subtitle drawn in the black band under it.
    static let bandGap: CGFloat = 8

    /// A drawn subtitle on the iPhone, where its box ends and how large it
    /// draws. A black band under the picture that holds the box at full size
    /// and 8 pt takes it, just under the picture, the controls up or not
    /// (upright). Else, with the controls up, the box's foot sits 8 pt over the
    /// scrubber's capsule or at the picture's foot when that is higher, at 85%
    /// while it would meet the transport and smaller still if 85% does not
    /// clear it, down to 75%; with them hidden it sits at the picture's foot.
    /// Skip or Up Next (`pillTop`, the pill's top) lifts the foot 8 pt over
    /// the pill (review apple-journey r3 P3-3); with the controls up only when
    /// the box at 75% still clears the transport there, else it stays on the
    /// capsule and the pill covers its end while they show (a phone across).
    /// `boxHeight` is the box at full size.
    static func subtitle(screenHeight: CGFloat, pictureHeight: CGFloat, safeBottom: CGFloat, controlsShown: Bool, scrubberTop: CGFloat, boxHeight: CGFloat, pillTop: CGFloat? = nil) -> (foot: CGFloat, scale: CGFloat) {
        let pictureBottom = (screenHeight + pictureHeight) / 2
        let plain = controlsShown ? scrubberTop - subtitleGap : screenHeight - max(40, safeBottom + 24)
        let overPill = pillTop.map { $0 - subtitleGap } ?? .infinity
        if min(plain, overPill) - pictureBottom >= boxHeight + bandGap {
            return (pictureBottom + bandGap + boxHeight, 1)
        }
        let pictureFoot = pictureBottom - 16
        guard controlsShown else { return (min(min(pictureFoot, plain) - 10, overPill), 1) }
        let clear = screenHeight / 2 + transportHalfHeight + subtitleClearance
        let low = min(pictureFoot, plain)
        let foot = overPill - boxHeight * subtitleFloor >= clear ? min(low, overPill) : low
        let room = foot - clear
        guard boxHeight > room else { return (foot, 1) }
        if boxHeight * subtitleShrink <= room { return (foot, subtitleShrink) }
        return (foot, max(room / boxHeight, subtitleFloor))
    }
}

/// The iPhone OSD's column (`PhoneFrame`): the top bar `topGap` under the
/// safe area's top, the scrubber block `bottomGap` over its foot.
struct PhoneOSD<Top: View, Bottom: View>: View {
    @ViewBuilder let top: Top
    @ViewBuilder let bottom: Bottom

    var body: some View {
        VStack(spacing: 0) {
            top
            Spacer(minLength: 0)
            bottom
        }
        .padding(.top, PhoneFrame.topGap)
        .padding(.bottom, PhoneFrame.bottomGap)
    }
}

/// The clock a hold and the outbox run on: the time since a start and a sleep. Tests pass a virtual
/// one, so the rules hold on a loaded machine too.
struct StepperClock: Sendable {
    let now: @Sendable () -> Duration
    let sleep: @Sendable (Duration) async -> Void

    static let real: StepperClock = {
        let origin = ContinuousClock.now
        return StepperClock(now: { ContinuousClock.now - origin }, sleep: { try? await Task.sleep(for: $0) })
    }()
}

/// A press or hold on the timing stepper's minus or plus by `PlayerLayout`'s rules: `start` on the
/// press, `stop(single: true)` on its release (one 0.1 s step when no repeat came yet), `stop(single:
/// false)` when it is cut off (a cancelled touch, lost focus, the app going inactive). `move` gets
/// each step with its sign.
@MainActor final class TimingHold {
    private let move: (Double) -> Void
    private let clock: StepperClock
    private var task: Task<Void, Never>?
    private(set) var direction = 0
    private var repeated = false

    init(clock: StepperClock = .real, move: @escaping (Double) -> Void) {
        (self.clock, self.move) = (clock, move)
    }

    func start(_ direction: Int) {
        stop(single: false)
        (self.direction, repeated) = (direction, false)
        let (clock, since) = (clock, clock.now())
        task = Task { [weak self] in
            await clock.sleep(.milliseconds(PlayerLayout.firstRepeatMs))
            while !Task.isCancelled, let self {
                repeated = true
                let held = Int((clock.now() - since) / .milliseconds(1))
                move(Double(direction) * PlayerLayout.holdStep(heldMs: held))
                await clock.sleep(.milliseconds(PlayerLayout.repeatMs))
            }
        }
    }

    func stop(single: Bool) {
        task?.cancel()
        task = nil
        if direction != 0, single, !repeated { move(Double(direction) * PlayerLayout.subtitleStep) }
        direction = 0
    }
}

/// The stepper's value on its way to the core (`subtitleDelay`): each track's value for the play, and
/// the one not sent yet, which goes once it rests 700 ms or at once on `flush` (another track, a
/// failover, a server move, close). `send` is synchronous (`commandNow`), so a flush reaches the core
/// before anything sent after it, an unsubscribe included.
@MainActor final class DelayOutbox {
    typealias Command = [String: any Sendable]
    private let clock: StepperClock
    private let send: (Command) -> Void
    private var pending: (url: String, seconds: Double)?
    private var settle: Task<Void, Never>?
    /// Each track's value in this play, by URL: going back to a track finds it where it was.
    private(set) var kept: [String: Double] = [:]
    /// Told the value once it settled from a press or hold, for VoiceOver.
    var settled: (Double) -> Void = { _ in }

    init(clock: StepperClock = .real, send: @escaping (Command) -> Void) {
        (self.clock, self.send) = (clock, send)
    }

    /// `spoken`: the settled value is said (a press or hold); an adjustable action's value is read
    /// by VoiceOver as it changes, so it is not said again.
    func set(url: String, seconds: Double, spoken: Bool = true) {
        kept[url] = seconds
        pending = (url, seconds)
        settle?.cancel()
        let clock = clock
        settle = Task { [weak self] in
            await clock.sleep(.milliseconds(PlayerLayout.delaySettleMs))
            guard !Task.isCancelled, let self else { return }
            flush()
            if spoken { settled(seconds) }
        }
    }

    /// The value not sent yet goes now, once.
    func flush() {
        settle?.cancel()
        settle = nil
        guard let pending else { return }
        self.pending = nil
        send(PlayerLayout.delayCommand(url: pending.url, seconds: pending.seconds))
    }

    /// Where `track` starts: its value earlier in this play, else the core's `delayMs`.
    func start(_ track: SubtitleTrack?) -> Double { PlayerLayout.startDelay(track, kept: kept) }

    /// A server move gave the track a new URL: its value and anything unsent follow it.
    func moved(from old: String, to new: String) {
        if let value = kept.removeValue(forKey: old) { kept[new] = value }
        if pending?.url == old { pending?.url = new }
    }
}
