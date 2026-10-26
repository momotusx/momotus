import CoreGraphics
import Foundation
import SwiftUI

/// Where the phone player puts its parts, as Android settled after three
/// review rounds (plans/android-design-v2.md, slices 14 to 16). Heights are
/// the real window's, bars included, since the player hides them.
/// The failure card's actions (plans/play-every-version.md, the shared table): never an account step.
enum FailureAction: Hashable { case offer, playFromDevice, retry, choose, versions, settingsAddons, back }

/// What the player shows over the picture, first match.
enum PlayerStatus: Equatable {
    case failed
    /// Play opened the player while versions were found, and none can start.
    case noPick
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
    static func status(phase: String?, cannotOpen: Bool, firstFrameShown: Bool, waiting: Bool, noPick: Bool = false) -> PlayerStatus {
        if phase == "failed" { return .failed }
        if noPick { return .noPick }
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

    /// The AirPlay button for a source: AVPlayer's rule above; never on the second engine, whose players refuse
    /// external playback (plans/apple-second-engine.md patch 2), so none is left from a source before a fallback.
    static func airPlayable(on backend: Backend, _ url: URL) -> Bool {
        backend == .avPlayer && allowsExternalPlayback(url)
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

    /// Whether a key press goes to the stepper: a modifier at the press makes it someone else's; at
    /// the release (Shift tapped mid-hold) the stepper's own hold must still end.
    static func stepperTakes(down: Bool, modifiers: EventModifiers) -> Bool {
        !down || stepperKey(modifiers)
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
        // A file track's last cue stays open (infinite end) until a delivery ends it.
        Array(Set(cues.flatMap { [$0.start + delay, $0.end + delay] }.filter { $0 >= 0 && $0.isFinite })).sorted()
    }

    /// A file's own track takes timing when the app can draw it: subtitles and text, not the captions
    /// carried in the video (CEA-608), whose layout the legible output cannot hand over, as on Android.
    static func fileTrackTimeable(mediaType: String) -> Bool {
        mediaType != "clcp"
    }

    /// The timing key of a file's own track: its place in the menu. In-file tracks have no core
    /// identity, so their value is kept for the play only, never sent.
    static func fileTrackKey(_ index: Int) -> String { fileTrackPrefix + "\(index)" }

    /// Where a file track's timing starts: where it was left this play, else none.
    static func startFileDelay(textTrack: Int?, kept: [String: Double]) -> Double {
        textTrack.flatMap { kept[fileTrackKey($0)] } ?? 0
    }

    /// The file's timing applies to a timeable file track with no addon track on: an addon track has its own, and a
    /// forced line the player shows beside it keeps its time.
    static func fileDelay(addonOn: Bool, timeable: Bool, delay: Double) -> Double {
        !addonOn && timeable ? delay : 0
    }

    /// The overlay's text: the file's line, then the addon track's, each already at its own timing.
    static func overlayText(file: String?, addon: String?) -> String? {
        let lines = [file, addon].compactMap { $0 }
        return lines.isEmpty ? nil : lines.joined(separator: "\n")
    }
    /// Where a bitmap subtitle (PGS, DVD, DVB) sits in `view`: its `position` is [0, 1] against the video frame,
    /// which is fitted into the view; nothing before the video's size is known.
    static func imageCueRect(position: CGRect, video: CGSize, view: CGSize) -> CGRect {
        guard video.width > 0, video.height > 0 else { return .zero }
        let scale = min(view.width / video.width, view.height / video.height)
        let fitted = CGSize(width: video.width * scale, height: video.height * scale)
        let origin = CGPoint(x: (view.width - fitted.width) / 2, y: (view.height - fitted.height) / 2)
        return CGRect(x: origin.x + position.minX * fitted.width, y: origin.y + position.minY * fitted.height,
                      width: position.width * fitted.width, height: position.height * fitted.height)
    }

    static let fileTrackPrefix = "file:"

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
    static func pipSubtitleNote(addonSubtitle: Bool, fileSubtitle: Bool = false, told: Bool) -> String? {
        guard !told else { return nil }
        if addonSubtitle { return "Addon subtitles do not show in Picture in Picture" }
        // The file's own track is drawn by the app too (plans/apple-file-subtitle-timing.md, decision A).
        return fileSubtitle ? "Subtitles from the file do not show in Picture in Picture" : nil
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

    /// Skip shows over the picture while a segment is under the playhead, never over a failure, the end card or an
    /// open track picker (it drew over the picker's rows).
    static func skipShows(segment: Bool, failed: Bool, endCard: Bool, pickerOpen: Bool) -> Bool {
        segment && !failed && !endCard && !pickerOpen
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

    /// The failure card's buttons, in the shared words.
    static func failureLabel(_ action: FailureAction, offer: String?) -> String {
        let empty = StreamChoice.words.empty
        return switch action {
        case .offer: "Play \(offer ?? "the next version") instead"
        case .playFromDevice: empty["unreachable"]?.playFromDevice ?? "Play from this device"
        case .retry: empty["notStarted"]?.action ?? "Try again"
        case .choose: StreamChoice.words.player.chooseAnother
        case .versions: empty["nothingPlaysHere"]?.action ?? "Choose a version"
        case .settingsAddons: empty["noSource"]?.action ?? "Settings"
        case .back: StreamChoice.words.page.back
        }
    }

    /// The core's line for why nothing played ("This didn't start"), else the plain words of an older core.
    static func failureHeadline(empty: String?, offer: Bool) -> String {
        if let line = empty.flatMap(VersionWords.emptyLine) { return line }
        return offer ? "This version could not be played" : "None of the sources could be played"
    }

    /// The card's actions as the shared table orders them, Back last. A version picked by hand that
    /// failed offers the ranking's next; the start's budget ran out (notStarted, or an older core):
    /// Try again; no room: Back only; no source in reach: the source kept here, then Try again; none
    /// plays here: another version.
    static func failureActions(empty: String?, offer: Bool, kept: Bool = false, canChoose: Bool) -> [FailureAction] {
        let choose: [FailureAction] = canChoose ? [.choose] : []
        if offer { return [.offer] + choose + [.back] }
        return switch empty {
        case "noRoom": [.back]
        case "unreachable": (kept ? [.playFromDevice] : []) + [.retry, .back]
        case "nothingPlaysHere": choose + [.back]
        default: [.retry] + choose + [.back]
        }
    }

    /// Play opened the player while versions were found and the core's view ended empty: the shared
    /// table's actions for its state, Back last (Choose a version for the rows that may still start).
    static func noPickActions(_ empty: EmptyVersions?, canChoose: Bool) -> [FailureAction] {
        let first: [FailureAction] = switch empty?.kind {
        case "noSource": [.settingsAddons]
        case "unreachable": (empty?.key == nil ? [] : [.playFromDevice]) + [.retry]
        case "nothingPlaysHere": canChoose ? [.versions] : []
        default: []
        }
        return first + [.back]
    }

    /// Why no version could start: the view's empty state; with none, when the view failed (its open, a
    /// command or an op), the unreachable state with no source kept (its line and Try again). A real
    /// empty state wins over a later failure.
    static func noPickState(_ empty: EmptyVersions?, failed: Bool) -> EmptyVersions? {
        empty ?? (failed ? EmptyVersions(kind: "unreachable", count: 0, rows: [], key: nil) : nil)
    }

    /// The card's headline when the view ended empty: the core's line, else (no view to ask) "No
    /// source has this yet".
    static func noPickHeadline(_ empty: EmptyVersions?) -> String {
        VersionWords.emptyLine(empty?.kind ?? "noResults") ?? VersionWords.emptyLine("noResults") ?? ""
    }

    /// A view that could not open: mostly "that version is no longer listed",
    /// where Try again opens the same key again, so another version comes
    /// first; Try again stays for a lost subscription (review r4 P3-6).
    static func cannotOpenActions(canChoose: Bool) -> [FailureAction] {
        (canChoose ? [.choose] : []) + [.retry, .back]
    }

    /// The failure history in plain words, each reason once with its count: never an addon's name.
    static func failureLines(_ attempts: [Attempt]) -> [String] {
        var order: [(error: String, count: Int)] = []
        for attempt in attempts {
            if let i = order.firstIndex(where: { $0.error == attempt.error }) { order[i].count += 1 } else { order.append((attempt.error, 1)) }
        }
        return order.map { $0.count > 1 ? "\($0.error) \u{B7} \($0.count) sources" : $0.error }
    }

    /// The source kept on this device, which starts with no source in reach: the empty state's, else
    /// the row the core marks "On this device".
    static func kept(_ versions: VersionsSnapshot?) -> String? {
        guard let versions else { return nil }
        return versions.empty?.key ?? ([versions.recommended].compactMap { $0 } + versions.rows).first { $0.onDevice != nil }?.key
    }

    /// Every start subtitle the core tried could not be read (`startSubtitleFailed`): one neutral line,
    /// naming no source.
    static func startSubtitleFailedWords(_ playback: PlaybackSnapshot?) -> String? {
        playback?.startSubtitleFailed == true ? StreamChoice.words.player.subtitlesFailed : nil
    }

    /// "Choose another version" on the loading screen and over a stalled frame: only while the core
    /// says so (one clock, the core's), never on a failure or a played frame that plays on.
    static func offersAnother(_ playback: PlaybackSnapshot?, status: PlayerStatus) -> Bool {
        guard playback?.chooseAnother == true, case .waiting = status else { return false }
        return true
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

    /// Under the spinner: "Finding the best version" while Play waits for the pick, the version the
    /// core moves on to while it resolves, else the player's own word on a failed source, else the
    /// core's stage caption, else the plain wait. Never an addon's name.
    static func waitingWords(_ playback: PlaybackSnapshot?, notice: String?, tryingAgain: Bool = false, findingPick: Bool = false) -> String {
        if findingPick { return StreamChoice.words.player.finding }
        // A Try again that moves on at once names the version it moved to.
        if playback?.phase == "resolving", let label = playback?.trying?.label {
            return "Trying the next version: \(label)"
        }
        if tryingAgain { return "Trying again" }
        return notice ?? playback?.start?.caption ?? "Getting the stream ready"
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
        let clear = controlsShown ? safeBottom + controlsHeight + 16 : subtitleClear(screenHeight: screenHeight, safeBottom: safeBottom)
        return min(pictureFoot, screenHeight - clear)
    }

    /// The least room under a subtitle's foot with the controls hidden: 24 pt
    /// over the safe area, 40 pt at least, and never under the title-safe line,
    /// 5% of the screen over its foot (EBU R 95; plans/subtitles-letterbox.md),
    /// which a tall Mac window whose picture fills it would pass.
    static func subtitleClear(screenHeight: CGFloat, safeBottom: CGFloat) -> CGFloat {
        max(40, safeBottom + 24, screenHeight * 0.05)
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
        let plain = controlsShown ? scrubberTop - subtitleGap : screenHeight - PlayerLayout.subtitleClear(screenHeight: screenHeight, safeBottom: safeBottom)
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
    /// True when the command went (a view was open to take it).
    private let send: (Command) -> Bool
    private var pending: (url: String, seconds: Double)?
    private var settle: Task<Void, Never>?
    /// Each track's value in this play, by URL: going back to a track finds it where it was.
    private(set) var kept: [String: Double] = [:]
    /// Told the value once it settled from a press or hold, for VoiceOver.
    var settled: (Double) -> Void = { _ in }

    init(clock: StepperClock = .real, send: @escaping (Command) -> Bool) {
        (self.clock, self.send) = (clock, send)
    }

    /// `spoken`: the settled value is said (a press or hold); an adjustable action's value is read
    /// by VoiceOver as it changes, so it is not said again. `sent` false: kept for the play only (a
    /// file's own track, which the core does not know).
    func set(url: String, seconds: Double, spoken: Bool = true, sent: Bool = true) {
        kept[url] = seconds
        if sent { pending = (url, seconds) } else { flush() }
        settle?.cancel()
        let clock = clock
        settle = Task { [weak self] in
            await clock.sleep(.milliseconds(PlayerLayout.delaySettleMs))
            guard !Task.isCancelled, let self else { return }
            flush()
            if spoken { settled(seconds) }
        }
    }

    /// The value not sent yet goes now, once; with no view open to take it (a server move between
    /// views) it stays for the next flush.
    func flush() {
        settle?.cancel()
        settle = nil
        guard let pending, send(PlayerLayout.delayCommand(url: pending.url, seconds: pending.seconds)) else { return }
        self.pending = nil
    }

    /// Where `track` starts: its value earlier in this play, else the core's `delayMs`.
    func start(_ track: SubtitleTrack?) -> Double { PlayerLayout.startDelay(track, kept: kept) }

    /// Values kept under keys starting with `prefix` go: another version is another file, whose own tracks share
    /// only their places with the last one's (`file:<index>`).
    func forget(prefix: String) {
        kept = kept.filter { !$0.key.hasPrefix(prefix) }
        if pending?.url.hasPrefix(prefix) == true { pending = nil }
    }

    /// A server move gave the track a new URL: its value and anything unsent follow it.
    func moved(from old: String, to new: String) {
        if let value = kept.removeValue(forKey: old) { kept[new] = value }
        if pending?.url == old { pending?.url = new }
    }
}

/// The player's side of a mid-play stall (plans/play-every-version.md 2e, Step 3 item 7). It tells the
/// core when a wait for data starts after steady playing (not after the start, a seek or a pause) and
/// when that wait ends: playing again, a seek, a pause, a stop or another source.
struct StallWatch {
    /// Seconds of playing since the start, a seek or a pause before a wait counts as a stall.
    static let steadyAfter = 1.0
    /// Where playing began after the last interruption; nil while it does not play.
    private var from: Double?
    private var steady = false
    private(set) var stalled = false

    /// The player plays, waits for data, or neither (paused, stopped) at `position`: the `stalled`
    /// value to send, or nil.
    mutating func see(playing: Bool, waiting: Bool, position: Double) -> Bool? {
        if playing {
            if stalled {
                // Playing on after a stall: still mid-play, so the next wait is a stall too.
                (stalled, from) = (false, position)
                return false
            }
            let from = self.from ?? position
            self.from = from
            if position - from >= Self.steadyAfter { steady = true }
            return nil
        }
        if waiting {
            guard steady, !stalled else { return nil }
            (stalled, from) = (true, nil)
            return true
        }
        return interrupted()
    }

    /// A seek, a pause, a stop or another source: a stall ends, and the next wait counts only after
    /// steady playing again.
    mutating func interrupted() -> Bool? {
        (steady, from) = (false, nil)
        guard stalled else { return nil }
        stalled = false
        return false
    }

    /// The command for a change.
    static func command(_ stalled: Bool, position: Double) -> [String: any Sendable] {
        ["type": "stalled", "stalled": stalled, "positionMs": Int64((max(0, position) * 1000).rounded())]
    }
}

/// The swarm's rate over the start window, from the core's own counts: the bar's one swarm figure for every play on
/// either player (code r5). The start bytes the core had at its last start snapshot less those at its first one with
/// bytes, over the time between; a new source (fewer bytes than before) starts it over; n/a with fewer than two.
struct StartWindow {
    private var first: (at: ContinuousClock.Instant, bytes: Int64)?
    private var last: (at: ContinuousClock.Instant, bytes: Int64)?

    mutating func saw(_ bytes: Int64, at: ContinuousClock.Instant) {
        guard bytes > 0 else { return }
        if let last, bytes < last.bytes { first = nil }
        if first == nil { first = (at, bytes) }
        last = (at, bytes)
    }

    var kibPerS: String {
        guard let first, let last, last.at > first.at else { return "n/a" }
        return "\(Int(Double(last.bytes - first.bytes) / 1024 / ((last.at - first.at) / .seconds(1))))"
    }

    /// The figure's raw ends, milliseconds from `tap` (code r6), so a bench row carries its inputs, not only the quotient.
    func ends(from tap: ContinuousClock.Instant) -> String {
        let ms = { (at: ContinuousClock.Instant) in Int((at - tap) / .milliseconds(1)) }
        guard let first, let last else { return "start_bytes_first=n/a start_ms_first=n/a start_bytes_last=n/a start_ms_last=n/a" }
        return "start_bytes_first=\(first.bytes) start_ms_first=\(ms(first.at)) start_bytes_last=\(last.bytes) start_ms_last=\(ms(last.at))"
    }
}
