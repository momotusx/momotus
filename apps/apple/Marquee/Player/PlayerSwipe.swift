#if os(iOS)
import AVFoundation
import MediaPlayer
import SwiftUI
import UIKit

/// The vertical swipe on the picture (plans/player-gestures.md 3.1, 3.2): direct touches only, so an iPad's trackpad
/// or mouse drag does nothing. It begins at the pan's own slop when the start is outside the system's bands and the
/// move is at least 2:1 up or down; a second finger before that keeps it from beginning, after it ends the swipe where
/// it is. Once it begins it takes the touch, so the picture's taps never fire for it.
struct LevelSwipe: UIGestureRecognizerRepresentable {
    var enabled: Bool
    /// The picture layer's size, the whole player, and its bands.
    var size: CGSize
    var bands: SwipeRules.Bands
    /// The level a swipe starts from, nil when this side has none now.
    var onStart: (SwipeRules.Side) -> Double?
    var onLevel: (SwipeRules.Side, Double) -> Void
    var onEnd: () -> Void

    func makeCoordinator(converter: CoordinateSpaceConverter) -> Coordinator { Coordinator(converter: converter) }

    func makeUIGestureRecognizer(context: Context) -> UIPanGestureRecognizer {
        let pan = UIPanGestureRecognizer()
        pan.allowedTouchTypes = [NSNumber(value: UITouch.TouchType.direct.rawValue)]
        pan.maximumNumberOfTouches = 2
        pan.delegate = context.coordinator
        return pan
    }

    func updateUIGestureRecognizer(_ recognizer: UIPanGestureRecognizer, context: Context) {
        recognizer.isEnabled = enabled
        context.coordinator.swipe = self
    }

    func handleUIGestureRecognizerAction(_ recognizer: UIPanGestureRecognizer, context: Context) {
        let run = context.coordinator
        switch recognizer.state {
        case .began:
            run.side = nil
            guard let side = run.pendingSide, let start = onStart(side) else { return }
            run.side = side
            run.start = start
            run.from = context.converter.localTranslation?.y ?? 0
        case .changed:
            guard let side = run.side else { return }
            if recognizer.numberOfTouches > 1 {
                run.side = nil
                onEnd()
                return
            }
            let dy = (context.converter.localTranslation?.y ?? run.from) - run.from
            onLevel(side, SwipeRules.level(start: run.start, dy: dy, height: run.height))
        default:
            if run.side != nil { onEnd() }
            run.side = nil
        }
    }

    @MainActor final class Coordinator: NSObject, UIGestureRecognizerDelegate {
        let converter: CoordinateSpaceConverter
        var swipe: LevelSwipe?
        /// The side the pan may begin for, decided in `gestureRecognizerShouldBegin`.
        var pendingSide: SwipeRules.Side?
        /// The swipe under way: its side, its start level, where its translation stood when it began.
        var side: SwipeRules.Side?
        var start: Double = 0
        var from: CGFloat = 0
        var height: CGFloat = 1

        init(converter: CoordinateSpaceConverter) { self.converter = converter }

        func gestureRecognizerShouldBegin(_ recognizer: UIGestureRecognizer) -> Bool {
            pendingSide = nil
            guard let swipe, let pan = recognizer as? UIPanGestureRecognizer, pan.numberOfTouches == 1,
                  let move = converter.localTranslation else { return false }
            let now = converter.localLocation
            let start = CGPoint(x: now.x - move.x, y: now.y - move.y)
            let size = swipe.size
            guard let side = SwipeRules.side(start: start, size: size, bands: swipe.bands),
                  SwipeRules.commits(dx: move.x, dy: move.y, threshold: 0) == true else { return false }
            pendingSide = side
            height = max(size.height, 1)
            return true
        }
    }
}

/// The system volume, read from the audio session and set through an `MPVolumeView`'s slider (public classes, the
/// way VLC and Infuse do it). The view in the window also keeps the system's volume HUD away, so the player answers
/// the hardware buttons itself. A set that does not take turns the swipe off for the play (3.3).
@MainActor @Observable final class SystemVolume {
    @ObservationIgnored fileprivate weak var view: MPVolumeView?
    @ObservationIgnored private var lastSet: Float?
    @ObservationIgnored private var checked = false
    /// The volume before the play's first set: the read-back compares against it.
    @ObservationIgnored private var before: Float?
    @ObservationIgnored private var observation: NSKeyValueObservation?
    /// No volume swipe for this play: no slider, or a route that keeps its own volume.
    private(set) var off = false

    /// Read at a swipe's commit, which needs the picture up, so the player's audio session is active and it is current.
    var level: Float { AVAudioSession.sharedInstance().outputVolume }

    func set(_ value: Float) {
        guard !off else { return }
        let before = self.before ?? level
        self.before = before
        lastSet = value
        guard put(value) else {
            off = true
            return
        }
        guard !checked, SwipeRules.checksVolume(before: before, set: value) else { return }
        checked = true
        Task { @MainActor [weak self] in
            try? await Task.sleep(for: .milliseconds(300))
            guard let self else { return }
            if !SwipeRules.volumeTook(before: before, set: value, after: self.level) { self.off = true }
        }
    }

    /// Moves the slider; false when the view has none.
    @discardableResult func put(_ value: Float) -> Bool {
        guard let slider = view?.subviews.lazy.compactMap({ $0 as? UISlider }).first else { return false }
        slider.setValue(value, animated: false)
        slider.sendActions(for: .valueChanged)
        return true
    }

    /// A swipe ended: 500 ms on, a button press that lands near its last level is the viewer's again, not an echo.
    func swipeEnded() {
        let mark = lastSet
        Task { @MainActor [weak self] in
            try? await Task.sleep(for: .milliseconds(500))
            if let self, self.lastSet == mark { self.lastSet = nil }
        }
    }

    /// The hardware buttons' changes, past the echo of the swipe's own sets; the first delivery is skipped.
    func watch(_ changed: @escaping @MainActor (Float) -> Void) {
        observation = AVAudioSession.sharedInstance().observe(\.outputVolume, options: [.new]) { [weak self] _, change in
            guard let value = change.newValue else { return }
            Task { @MainActor in
                guard let self, !SwipeRules.isEcho(value: value, lastSet: self.lastSet) else { return }
                changed(value)
            }
        }
    }
}

/// The `MPVolumeView` the volume needs in the window: 1 by 1 pt, nearly clear, hidden from VoiceOver.
struct VolumeControl: UIViewRepresentable {
    let volume: SystemVolume

    func makeUIView(context: Context) -> MPVolumeView {
        let view = MPVolumeView(frame: CGRect(x: 0, y: 0, width: 1, height: 1))
        view.alpha = 0.01
        view.isAccessibilityElement = false
        view.accessibilityElementsHidden = true
        volume.view = view
        return view
    }

    func updateUIView(_ view: MPVolumeView, context: Context) {}
}

/// The system brightness a swipe changed, and putting it back (3.5). iOS has no per-app brightness, so the player puts
/// back what it found when it leaves or goes to the background, but only if the screen still shows what it set: a
/// change the viewer made in Control Center since then is theirs. A record in `UserDefaults` repairs a crash, and stays
/// until a put is read back on the screen: a return that did not take is tried again when the scene is active.
@MainActor final class BrightnessKeeper {
    private var found: CGFloat?
    private var set: CGFloat?
    /// The swipe under way has written the record once (it is written again at the swipe's end).
    private var recorded = false
    /// A return that did not take (no screen, or a write the system ignored from the background).
    private(set) var pending = false
    /// The screen the swipe used, held so a return from a scene already in the background still reaches it.
    private weak var held: UIScreen?

    /// The screen the swipe used, else the active window scene's, else any window scene's: by the time the scene
    /// phase says background, the scene is no longer active. Never `UIScreen.main` (deprecated, and not the scene's).
    private var screen: UIScreen? {
        if let held { return held }
        let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
        return (scenes.first { $0.activationState == .foregroundActive } ?? scenes.first)?.screen
    }

    /// A swipe begins: the level now, and what to put back later.
    func start() -> Double? {
        guard let screen else { return nil }
        held = screen
        let now = screen.brightness
        found = SwipeRules.found(current: now, set: set, found: found)
        return Double(now)
    }

    func apply(_ level: Double) {
        guard let screen else { return }
        screen.brightness = CGFloat(level)
        set = CGFloat(level)
        if !recorded { ended() }
    }

    /// A swipe ended (and its first move): the record holds what to put back and what the player set.
    func ended() {
        guard let found, let set else { return }
        recorded = true
        Self.save(found: found, set: set)
    }

    /// Puts the brightness found back by the rule of 3.5; a play with no swipe of its own repairs a record a crash left.
    func restore() {
        recorded = false
        if found == nil, let record = Self.record {
            (found, set) = (record.found, record.set)
        }
        let screen = screen
        switch SwipeRules.brightnessReturn(found: found, set: set, current: screen?.brightness) {
        case .forget:
            forget()
        case .keep:
            pending = true
        case .put(let value):
            screen?.brightness = value
            if let screen, SwipeRules.took(brightness: screen.brightness, put: value, active: Self.active) { forget() } else { pending = true }
        }
    }

    private func forget() {
        found = nil
        set = nil
        pending = false
        Self.clear()
    }

    /// A scene is in the foreground and active: only then does a read-back prove a write took.
    private static var active: Bool {
        UIApplication.shared.connectedScenes.contains { ($0 as? UIWindowScene)?.activationState == .foregroundActive }
    }

    /// The launch's first activation has run the repair (later ones must not: a player may be up).
    static var repairedOnActive = false

    /// At launch: a record a crash left behind is put back by the same rule.
    static func repair(_ screen: UIScreen?) {
        let keeper = BrightnessKeeper()
        keeper.held = screen
        keeper.restore()
    }

    private static var record: (found: CGFloat, set: CGFloat)? {
        guard let values = UserDefaults.standard.array(forKey: key) as? [Double], values.count == 2 else { return nil }
        return (CGFloat(values[0]), CGFloat(values[1]))
    }

    private static func save(found: CGFloat, set: CGFloat) {
        UserDefaults.standard.set([Double(found), Double(set)], forKey: key)
    }

    private static func clear() { UserDefaults.standard.removeObject(forKey: key) }

    private static let key = "MQBrightnessRecord"
}

/// What the indicator shows. Only the indicator reads `value`, so a swipe's moves redraw it and nothing else; it goes
/// 800 ms after a swipe ends or a volume button's change.
@MainActor @Observable final class SwipeLevel {
    private(set) var side: SwipeRules.Side = .volume
    private(set) var value: Double = 0
    private(set) var shown = false
    /// Counts the times a swipe reached an end, for the haptic tick.
    private(set) var ends = 0
    @ObservationIgnored private var hide: Task<Void, Never>?
    /// A swipe is under way: the buttons' changes (late echoes of its own sets among them) wait.
    @ObservationIgnored private var swiping = false

    func begin(_ side: SwipeRules.Side, at value: Double) {
        swiping = true
        hide?.cancel()
        self.side = side
        self.value = value
        shown = true
    }

    func move(to value: Double) {
        if SwipeRules.reachedEnd(before: self.value, after: value) { ends += 1 }
        self.value = value
    }

    func end() {
        swiping = false
        hide?.cancel()
        hide = Task { [weak self] in
            do { try await Task.sleep(for: .milliseconds(800)) } catch { return }
            self?.shown = false
        }
    }

    /// The volume swipe turned off under the finger (3.3): the indicator goes at once, so it never shows a level the
    /// device does not have.
    func hideNow() {
        hide?.cancel()
        shown = false
    }

    /// A volume button's change, shown as a swipe that ends at once.
    func show(_ side: SwipeRules.Side, at value: Double) {
        guard !swiping else { return }
        begin(side, at: value)
        end()
    }
}

#if DEBUG
/// Walk builds (`-MQMute YES`): the device volume a walk found before its first volume swipe goes back when the player
/// closes, so a walk never leaves the owner's phone louder or quieter. The record outlives a crash: the next player to
/// open, once its volume view is in the window, puts it back.
@MainActor enum WalkLevels {
    /// The player opened: a record a crashed run left is put back once the volume view is in the window, once per
    /// process (a later open, back from Versions, would put back this run's own record mid-walk).
    static func begin(_ volume: SystemVolume) {
        guard DebugHooks.mute else { return }
        coreLog.notice("debug: walk opened, brightness \(Self.brightness, privacy: .public)")
        guard !repaired else { return }
        repaired = true
        guard UserDefaults.standard.object(forKey: key) != nil else { return }
        Task { @MainActor in
            try? await Task.sleep(for: .seconds(1))
            end(volume)
        }
    }

    /// A volume swipe commits: the volume it starts from, read with the audio session active, is the one to put back.
    static func found(_ level: Float) {
        guard DebugHooks.mute, UserDefaults.standard.object(forKey: key) == nil else { return }
        UserDefaults.standard.set(level, forKey: key)
        coreLog.notice("debug: walk found volume \(level, privacy: .public)")
    }

    /// Puts the found volume back while the volume view is still in the window; the record stays when it cannot.
    static func end(_ volume: SystemVolume) {
        guard DebugHooks.mute, UserDefaults.standard.object(forKey: key) != nil else { return }
        let found = UserDefaults.standard.float(forKey: key)
        guard volume.put(found) else { return }
        UserDefaults.standard.removeObject(forKey: key)
        Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(300))
            coreLog.notice("debug: walk left volume \(volume.level, privacy: .public) of \(found, privacy: .public) brightness \(Self.brightness, privacy: .public)")
        }
    }

    private static var brightness: Double {
        Double(UIApplication.shared.connectedScenes.lazy.compactMap { $0 as? UIWindowScene }.first?.screen.brightness ?? -1)
    }

    /// The crash repair of `begin` has run in this process.
    private static var repaired = false

    private static let key = "MQWalkVolume"
}
#endif

/// The answer to a swipe or a volume button: a 44 by 168 capsule at 40 percent black (the double-tap hint's
/// platter), the side's icon and a 4 by 112 bar, white 92 percent over a white 20 percent track (DECISION, Progress).
struct LevelIndicator: View {
    let levels: SwipeLevel
    private var side: SwipeRules.Side { levels.side }
    private var level: Double { levels.value }

    var body: some View {
        VStack(spacing: 8) {
            Glyph(icon, size: 20)
            GeometryReader { geo in
                ZStack(alignment: .bottom) {
                    Capsule().fill(.white.opacity(0.2))
                    Capsule().fill(.white.opacity(0.92)).frame(height: geo.size.height * level)
                }
            }
            .frame(width: 4, height: 112)
        }
        .foregroundStyle(.white)
        .padding(.vertical, 12)
        .frame(width: 44, height: 168)
        .background(Capsule().fill(.black.opacity(0.4)))
        .accessibilityHidden(true)
        .allowsHitTesting(false)
    }

    private var icon: String {
        switch side {
        case .brightness: "sun"
        case .volume: level == 0 ? "volume-x" : level < 0.5 ? "volume-1" : "volume-2"
        }
    }
}

/// The double-tap answer, as Android draws it: a 96 pt circle at 40 percent black, two chevrons and "10 seconds".
struct SeekHint: View {
    let forward: Bool

    var body: some View {
        VStack(spacing: 2) {
            HStack(spacing: 0) {
                Glyph(forward ? "chevron-right" : "chevron-left", size: 22)
                Glyph(forward ? "chevron-right" : "chevron-left", size: 22)
            }
            Text("10 seconds")
                .textFont(Theme.type.caption, weight: .semibold, relativeTo: .caption)
                .lineLimit(1)
        }
        .foregroundStyle(.white)
        .frame(width: 96, height: 96)
        .background(Circle().fill(.black.opacity(0.4)))
        .accessibilityHidden(true)
        .allowsHitTesting(false)
    }
}
#endif
