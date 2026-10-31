import SwiftUI

extension KeypadResult {
    /// A core refusal sorted for the keypad.
    static func of(_ error: any Error) -> KeypadResult {
        let core = error as? CoreError
        return of(code: core?.code, retryAfterMs: core?.retryAfterMs, message: error.localizedDescription)
    }
}

/// A profile's avatar: a Lucide glyph (or the name's first letter) in white
/// on one of the avatar colors, drawn here; nothing is fetched.
struct ProfileAvatar: View {
    let avatar: ProfileRules.Avatar
    let name: String
    var size: CGFloat = 44
    /// Under TV focus: white with a dark glyph (design/DECISION.md, Profiles).
    var lit = false

    init(_ stored: String, name: String, size: CGFloat = 44, lit: Bool = false) {
        avatar = ProfileRules.parseAvatar(stored)
        self.name = name
        self.size = size
        self.lit = lit
    }

    init(_ avatar: ProfileRules.Avatar, name: String, size: CGFloat = 44) {
        self.avatar = avatar
        self.name = name
        self.size = size
    }

    var body: some View {
        Circle()
            .fill(lit ? Theme.Palette.primary : Self.color(avatar.color))
            .overlay {
                if avatar.glyph == "letter" {
                    Text(ProfileRules.initial(name))
                        .textFont(size * 0.46, weight: .bold)
                } else {
                    Image(avatar.glyph == "gamepad" ? "gamepad-2" : avatar.glyph)
                        .renderingMode(.template)
                        .resizable()
                        .scaledToFit()
                        .frame(width: size * 0.5, height: size * 0.5)
                }
            }
            .foregroundStyle(lit ? Theme.Palette.onPrimary : .white)
            .frame(width: size, height: size)
            .accessibilityHidden(true)
    }

    static func color(_ id: String?) -> Color {
        switch id {
        case "ember": Theme.Palette.avatarEmber
        case "amber": Theme.Palette.avatarAmber
        case "moss": Theme.Palette.avatarMoss
        case "teal": Theme.Palette.avatarTeal
        case "sky": Theme.Palette.avatarSky
        case "plum": Theme.Palette.avatarPlum
        case "rose": Theme.Palette.avatarRose
        case "graphite": Theme.Palette.avatarGraphite
        default: Theme.Palette.raised
        }
    }
}

/// Four dots and a 3 x 4 grid of keys. The fourth digit submits; one try at
/// a time. A wrong PIN clears the digits, too many tries wait with a
/// countdown (focus on the way out, back on 5 when it ends). Digits live only
/// here and are cleared when the keypad closes or the app goes to the back.
struct PinKeypad: View {
    let title: String
    /// Shown once on appear: a switch that failed before this keypad took over.
    var initial: KeypadResult?
    var cancelTitle = "Choose another profile"
    var note: String?
    /// The owner's way out of a forgotten PIN, behind the device's own lock.
    var reset: (title: String, run: () async -> KeypadResult)?
    let submit: (String) async -> KeypadResult
    let cancel: () -> Void

    @State private var digits = ""
    @State private var busy = false
    @State private var said: String?
    @State private var until: Date?
    @State private var now = Date.now
    @State private var attempt = 0
    @FocusState private var focus: String?
    @Environment(\.metrics) private var metrics
    @Environment(\.scenePhase) private var phase

    private var waiting: Bool { until.map { $0 > now } ?? false }
    private var key: CGFloat {
        #if os(tvOS)
        96
        #else
        metrics.wide ? 72 : 64
        #endif
    }

    var body: some View {
        VStack(spacing: Theme.Space.m) {
            Text(title)
                .display(Theme.type.title2)
                .multilineTextAlignment(.center)
                .foregroundStyle(Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            dots
            if let said {
                Text(said)
                    .textFont(Theme.type.body, weight: .semibold)
                    .foregroundStyle(Theme.Palette.text)
                    .multilineTextAlignment(.center)
            }
            if let until, waiting {
                Text("Try again in \(ProfileRules.waitText(Int64(until.timeIntervalSince(now) * 1000)))")
                    .textFont(Theme.type.callout, relativeTo: .callout).monospacedDigit()
                    .foregroundStyle(Theme.Palette.muted)
            }
            grid
            if let note {
                Text(note)
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.muted)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: 420)
            }
            // Side by side when they fit, else one over the other.
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 12) { ways }
                VStack(spacing: 12) { ways }
            }
        }
        .frame(maxWidth: .infinity)
        // Hardware digits and Delete type wherever focus is within the keypad.
        .onKeyPress(characters: .decimalDigits, phases: .down) { press in
            press.characters.forEach { type($0) }
            return .handled
        }
        .onKeyPress(.delete, phases: .down) { _ in delete(onScreen: false); return .handled }
        #if os(tvOS)
        // Menu leaves the keypad for the profile tiles, one step, never the whole picker.
        .onExitCommand { leave() }
        #else
        .focusable()
        .focusEffectDisabled()
        .focused($focus, equals: "keys")
        #endif
        .onAppear {
            #if os(tvOS)
            focus = "5"
            #else
            focus = "keys"
            #endif
            if let initial { show(initial) }
        }
        .onDisappear { digits = "" }
        .onChange(of: phase) { _, phase in if phase != .active { digits = "" } }
        .task(id: until) {
            // The countdown, each second, until the wait ends.
            while let until, until > Date.now {
                now = .now
                try? await Task.sleep(for: .seconds(1))
                if Task.isCancelled { return }
            }
            now = .now
            if until != nil {
                until = nil
                say("You can try again.")
                #if os(tvOS)
                focus = "5"
                #endif
            }
        }
    }

    @ViewBuilder private var ways: some View {
        Button(cancelTitle, action: leave)
            .buttonStyle(PillButtonStyle())
            .focused($focus, equals: "cancel")
            // A try with the core is never undone from here: its answer decides.
            .disabled(busy)
        if let reset {
            Button(reset.title) { run { await reset.run() } }
                .buttonStyle(PillButtonStyle())
                .disabled(busy)
        }
    }

    private var dots: some View {
        HStack(spacing: 16) {
            ForEach(0..<4, id: \.self) { index in
                Circle()
                    .fill(index < digits.count ? Theme.Palette.text : .clear)
                    .overlay(Circle().strokeBorder(Theme.Palette.text, lineWidth: 2))
                    .frame(width: 16, height: 16)
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(digits.count) of 4 digits entered")
    }

    private var grid: some View {
        let rows = [["1", "2", "3"], ["4", "5", "6"], ["7", "8", "9"], ["", "0", "delete"]]
        return VStack(spacing: 12) {
            ForEach(rows, id: \.self) { row in
                HStack(spacing: 12) {
                    ForEach(row, id: \.self) { name in
                        if name.isEmpty {
                            Color.clear.frame(width: key, height: key)
                        } else {
                            Button { name == "delete" ? delete(onScreen: true) : type(Character(name)) } label: {
                                Group {
                                    if name == "delete" { Glyph("delete", size: 24) } else { Text(name).textFont(Theme.type.title2, weight: .semibold) }
                                }
                                .frame(width: key, height: key)
                                .background(Circle().fill(Theme.Palette.raised))
                                .overlay(Circle().strokeBorder(Theme.Palette.control, lineWidth: 1))
                                .contentShape(Circle())
                            }
                            .buttonStyle(.round)
                            .foregroundStyle(Theme.Palette.text)
                            .focused($focus, equals: name)
                            .opacity(busy || waiting ? 0.4 : 1)
                            .disabled(busy || waiting)
                            .accessibilityLabel(name == "delete" ? "Delete" : name)
                        }
                    }
                }
            }
        }
    }

    private func type(_ digit: Character) {
        guard digit.isASCII, digit.isNumber, !busy, !waiting, digits.count < 4 else { return }
        digits.append(digit)
        if digits.count == 4 {
            let pin = digits
            run { await submit(pin) }
        }
    }

    /// Removes the last digit; with none, the keyboard's Delete leaves the keypad and the key on screen does nothing
    /// (`KeypadDelete`). Works while waiting.
    private func delete(onScreen: Bool) {
        guard !busy else { return }
        switch KeypadDelete.of(digits: digits, onScreen: onScreen) {
        case .removeLast: digits.removeLast()
        case .leave: leave()
        case .nothing: break
        }
    }

    private func leave() {
        digits = ""
        cancel()
    }

    /// One try at a time: a reply from an earlier try, or after the keypad
    /// closed, does nothing.
    private func run(_ work: @escaping () async -> KeypadResult) {
        busy = true
        attempt += 1
        let mine = attempt
        Task {
            let result = await work()
            guard mine == attempt else { return }
            busy = false
            digits = ""
            show(result)
        }
    }

    private func show(_ result: KeypadResult) {
        switch result {
        case .ok:
            break
        case .wrong:
            say("Wrong PIN")
        case .wait(let ms):
            until = Date.now.addingTimeInterval(TimeInterval(ms) / 1000)
            now = .now
            say(ProfileRules.waitSentence(ms))
            focus = "cancel"
        case .problem(let text):
            say(text)
        }
    }

    private func say(_ text: String) {
        said = text
        AccessibilityNotification.Announcement(text).post()
    }
}
