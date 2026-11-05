#if !os(tvOS)
import LocalAuthentication
#endif
import SwiftUI

/// "Who's watching?": the profiles as avatar tiles. In the gate it owns the
/// screen (the start picker, the locked picker, a switch that failed);
/// pushed from the profile button or Settings, Back or the current profile
/// returns with everything as it was.
struct ProfilePicker: View {
    enum Mode { case gate, push }

    let mode: Mode
    let core: CoreClient
    /// The profile with a PIN the bar's menu chose: the picker opens on its keypad (plans/nav-profile.md).
    let pinFor: ProfileInfo?
    @Environment(ProfileSession.self) private var profiles
    @Environment(\.dismiss) private var dismiss
    @Environment(\.metrics) private var metrics
    /// The profile whose PIN is asked.
    @State private var keypad: ProfileInfo?

    /// `manage`: opened on Manage profiles (the Mac panel's pill), Back to the tiles.
    init(mode: Mode, core: CoreClient, pinFor: ProfileInfo? = nil, manage: Bool = false) {
        (self.mode, self.core, self.pinFor) = (mode, core, pinFor)
        _keypad = State(initialValue: pinFor)
        _showManage = State(initialValue: manage)
    }
    @State private var said: String?
    @State private var showManage: Bool
    @State private var showAdd = false
    @FocusState private var focused: String?

    var body: some View {
        ScrollView(.vertical) {
            VStack(spacing: Theme.Space.l) {
                if let snapshot = profiles.snapshot {
                    content(snapshot)
                }
            }
            .frame(maxWidth: 1200)
            .frame(maxWidth: .infinity)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.xl)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        // At large text sizes the grid scrolls: names never pass under the status bar bare.
        .topScrim()
        .navigationDestination(isPresented: $showManage) { ManageProfiles(core: core) }
        .navigationDestination(isPresented: $showAdd) { EditProfile(profileId: nil) }
        .onChange(of: profiles.gone?.count) {
            // A profile removed on another device while its PIN is asked.
            guard let gone = profiles.gone, keypad?.id == gone.id else { return }
            keypad = nil
            said = "That profile was removed on another device."
        }
    }

    @ViewBuilder private func content(_ snapshot: ProfilesSnapshot) -> some View {
        if mode == .gate, let target = profiles.switching, profiles.switchProblem == nil {
            VStack(spacing: Theme.Space.s) {
                ProgressView().controlSize(.large).tint(Theme.Palette.text)
                Text(target == Incognito.id ? Incognito.opening : "Opening \(snapshot.profiles.first { $0.id == target }?.name ?? "the profile")")
                    .textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                    .foregroundStyle(Theme.Palette.text)
            }
            .padding(.top, Theme.Space.xl)
        } else if case let (target, incognito)? = keypadTarget(snapshot) {
            // Incognito asks another profile's PIN (the owner's, or the host's own): the keypad says which and why.
            let keyed = incognito ? Incognito.id : target.id
            PinKeypad(
                title: incognito ? Incognito.pinTitle(target.name) : "Enter the PIN for \(target.name)",
                initial: failedSwitch(for: keyed),
                // Opened by the bar's menu: Cancel goes back where the menu was, not to the tiles.
                cancelTitle: mode == .push && keyed == pinFor?.id ? "Cancel" : "Choose another profile",
                note: incognito ? (Incognito.kidsReason(snapshot) ? Incognito.kidsReason : nil) : ownerNote(target, snapshot),
                reset: incognito ? nil : resetAction(target, snapshot),
                submit: { pin in
                    incognito ? await enterIncognito(pin: pin) : await enter(target, pin: pin, snapshot: snapshot)
                },
                cancel: { leaveKeypad(keyed) })
            .id(keyed)
        } else {
            header(snapshot)
            // The ended line takes the chooser's polite status until the next profile op clears it (5.4).
            if let said = said ?? switchWords ?? (snapshot.incognitoEnded == true ? Incognito.ended : nil) {
                Text(said)
                    .textFont(Theme.type.body, weight: .semibold)
                    .foregroundStyle(Theme.Palette.text)
                    .multilineTextAlignment(.center)
            }
            tiles(snapshot)
            if canManage(snapshot) {
                Button { showManage = true } label: { Label("Manage profiles", glyph: "users") }
                    .buttonStyle(PillButtonStyle())
            }
        }
    }

    private func header(_ snapshot: ProfilesSnapshot) -> some View {
        VStack(spacing: Theme.Space.xs) {
            Text("Who's watching?")
                .display(metrics.wide ? 44 : Theme.type.title1 + 4)
                .foregroundStyle(Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            if ProfileRules.lockLine(snapshot) {
                Text("This profile is locked. Choose a profile, then enter its PIN.")
                    .textFont(Theme.type.body)
                    .foregroundStyle(Theme.Palette.muted)
                    .multilineTextAlignment(.center)
            }
        }
    }

    private var tile: CGFloat {
        #if os(tvOS)
        168
        #else
        metrics.wide ? 140 : 112
        #endif
    }

    private func tiles(_ snapshot: ProfilesSnapshot) -> some View {
        let addable = canManage(snapshot) && snapshot.profiles.count < snapshot.max
        let incognito = Incognito.offered(snapshot)
        let count = snapshot.profiles.count + (incognito ? 1 : 0) + (addable ? 1 : 0)
        let columns = Array(repeating: GridItem(.fixed(tile + 24), spacing: Theme.Space.m), count: metrics.wide ? min(max(count, 1), 6) : 2)
        return LazyVGrid(columns: columns, spacing: Theme.Space.m) {
            ForEach(snapshot.profiles) { profile in
                Button { pick(profile, snapshot) } label: {
                    VStack(spacing: 10) {
                        ProfileAvatar(profile.avatar, name: profile.name, size: tile)
                        HStack(spacing: 6) {
                            Text(profile.name).lineLimit(2).multilineTextAlignment(.center)
                            if profile.hasPin && !profile.syncing { Glyph("lock", size: 16) }
                        }
                        .textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                        if profile.syncing {
                            Text("Syncing").textFont(Theme.type.caption, relativeTo: .caption).foregroundStyle(Theme.Palette.muted)
                        }
                    }
                    .foregroundStyle(Theme.Palette.text)
                    .opacity(profile.syncing ? 0.55 : 1)
                }
                .buttonStyle(TileButtonStyle())
                .disabled(profile.syncing)
                .focused($focused, equals: profile.id)
                .accessibilityLabel(ProfileRules.spoken(profile, active: snapshot.active))
            }
            // Incognito, quiet, after the profiles and before Add profile, drawn like it (5.1).
            if incognito {
                Button { pickIncognito(snapshot) } label: {
                    VStack(spacing: 10) {
                        Circle()
                            .fill(Theme.Palette.raised)
                            .overlay(Glyph("hat-glasses", size: tile * 0.4))
                            .frame(width: tile, height: tile)
                        HStack(spacing: 6) {
                            Text(Incognito.name)
                            // As a profile with a PIN shows it: a keypad comes next.
                            if Incognito.pinProfile(snapshot) != nil { Glyph("lock", size: 16) }
                        }
                        .textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                    }
                    .foregroundStyle(Theme.Palette.text)
                }
                .buttonStyle(TileButtonStyle())
                .focused($focused, equals: Incognito.id)
                .accessibilityLabel(Incognito.spoken(snapshot))
            }
            if addable {
                Button { showAdd = true } label: {
                    VStack(spacing: 10) {
                        Circle()
                            .fill(Theme.Palette.raised)
                            .overlay(Glyph("plus", size: tile * 0.3))
                            .frame(width: tile, height: tile)
                        Text("Add profile").textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                    }
                    .foregroundStyle(Theme.Palette.text)
                }
                .buttonStyle(TileButtonStyle())
                .accessibilityLabel("Add profile")
            }
        }
        #if os(tvOS)
        .defaultFocus($focused, snapshot.active)
        #endif
    }

    /// Owner tools: in the pushed picker, unlocked, for the owner.
    private func canManage(_ snapshot: ProfilesSnapshot) -> Bool {
        mode == .push && !snapshot.locked && snapshot.current?.isOwner == true
    }

    /// The keypad's profile: the one chosen, or a switch that failed for a PIN; `incognito` when it opens Incognito
    /// with that profile's PIN.
    private func keypadTarget(_ snapshot: ProfilesSnapshot) -> (ProfileInfo, incognito: Bool)? {
        let incognitoPin = Incognito.pinProfile(snapshot).map { ($0, incognito: true) }
        if let keypad {
            if keypad.id == Incognito.id { return incognitoPin }
            return snapshot.profiles.first { $0.id == keypad.id }.map { ($0, incognito: false) }
        }
        guard mode == .gate, let target = profiles.switching, profiles.switchProblem != nil else { return nil }
        if target == Incognito.id { return incognitoPin }
        return snapshot.profiles.first { $0.id == target && $0.hasPin }.map { ($0, incognito: false) }
    }

    private func failedSwitch(for target: String) -> KeypadResult? {
        guard mode == .gate, profiles.switching == target, let problem = profiles.switchProblem else { return nil }
        return .of(problem)
    }

    /// A switch to a profile with no PIN that failed: the reason, over the tiles.
    private var switchWords: String? {
        guard mode == .gate, profiles.switching != nil, let problem = profiles.switchProblem else { return nil }
        if case .problem(let text) = KeypadResult.of(problem) { return text }
        return "That did not work. Try again."
    }

    private func pick(_ profile: ProfileInfo, _ snapshot: ProfilesSnapshot) {
        let choice = ProfileRules.choice(profile, in: snapshot)
        guard choice != .none else { return }
        said = nil
        switch choice {
        case .none: break
        case .close:
            switch mode {
            case .push: dismiss()
            case .gate:
                // A failed switch is given up, and this profile is the answer.
                if profiles.switching != nil { profiles.cancelSwitch() }
                profiles.markChosen()
            }
        case .pin: keypad = profile
        case .switchTo: Task { await profiles.switchTo(profile.id, pin: nil) }
        }
    }

    /// The Incognito tile: it does what a profile's tile does (`Incognito.choice`); from a tile, Leave comes back here.
    private func pickIncognito(_ snapshot: ProfilesSnapshot) {
        said = nil
        switch Incognito.choice(snapshot) {
        case .close:
            if mode == .push { dismiss() }
        case .pin: keypad = Incognito.profile
        default: Task { await profiles.enterIncognito(pin: nil, fromChooser: true) }
        }
    }

    private func enterIncognito(pin: String) async -> KeypadResult {
        // The menu's keypad says false; a failed entry asked again keeps what it said.
        let fromChooser = keypad == nil ? profiles.incognitoFromChooser : pinFor?.id != Incognito.id
        if await profiles.enterIncognito(pin: pin, fromChooser: fromChooser) { return .ok }
        return profiles.switchProblem.map { .of($0) } ?? .problem(Incognito.couldNotOpen)
    }

    private func enter(_ target: ProfileInfo, pin: String, snapshot: ProfilesSnapshot) async -> KeypadResult {
        if target.id == snapshot.active && snapshot.locked {
            do {
                try await profiles.unlock(pin: pin)
                return .ok
            } catch {
                return .of(error)
            }
        }
        if await profiles.switchTo(target.id, pin: pin) { return .ok }
        return profiles.switchProblem.map { .of($0) } ?? .problem("The switch did not finish.")
    }

    private func leaveKeypad(_ target: String) {
        // Opened by the bar's menu on this keypad: Cancel leaves the picker, back where the menu was.
        if mode == .push, target == pinFor?.id { return dismiss() }
        keypad = nil
        // A switch that failed is given up: the current profile opens again.
        if mode == .gate, profiles.switching != nil { profiles.cancelSwitch() }
        focused = target
    }

    private func ownerNote(_ target: ProfileInfo, _ snapshot: ProfilesSnapshot) -> String? {
        guard target.isOwner else { return nil }
        #if os(tvOS)
        return "Forgot the PIN? Reset it from Momotus on your phone or Mac. This device gets the change at its next sync."
        #else
        return "Forgot the PIN?"
        #endif
    }

    private func resetAction(_ target: ProfileInfo, _ snapshot: ProfilesSnapshot) -> (title: String, run: () async -> KeypadResult)? {
        #if os(tvOS)
        return nil
        #else
        guard target.isOwner else { return nil }
        return ("Reset with this device's lock", { await resetOwner(target) })
        #endif
    }

    #if !os(tvOS)
    /// The device's own lock, then `resetOwnerPin`, then the session opens.
    private func resetOwner(_ owner: ProfileInfo) async -> KeypadResult {
        let context = LAContext()
        var error: NSError?
        guard context.canEvaluatePolicy(.deviceOwnerAuthentication, error: &error) else {
            return .problem("Set a passcode on this device first, then try again.")
        }
        guard (try? await context.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: "Reset the main profile's PIN")) == true else {
            return .ok
        }
        do {
            try await profiles.resetOwnerPin(owner: owner.id)
            return .ok
        } catch {
            return .problem(ProfileRules.problem((error as? CoreError)?.code, fallback: "The PIN could not be reset."))
        }
    }
    #endif
}

/// The bar's far-right item (plans/nav-profile.md): the active profile, drawn by each bar (`face`), opening a menu of
/// the profiles, the current one checked, then Settings, and nothing account-like. A profile does what its picker
/// tile does (`ProfileRules.choice`): the current one closes the menu, one with a PIN opens the picker on its keypad
/// (`onPin`), any other switches. tvOS presents the same rows as a dialog, whose focus the system keeps.
struct ProfileMenu<Face: View>: View {
    let onSettings: () -> Void
    let onPin: (ProfileInfo) -> Void
    #if os(tvOS)
    /// The bar's focus on the avatar, set on the button itself (Menu on the avatar goes Home; Up enters on it for Settings).
    var focus: FocusState<Bool>.Binding? = nil
    #elseif os(macOS)
    /// The Mac's profile panel (plans/profile-panel.md): the avatar opens it and closes it again.
    let panel: Binding<Bool>
    #endif
    @ViewBuilder let face: (ProfileInfo) -> Face
    @Environment(ProfileSession.self) private var profiles
    #if os(tvOS)
    @State private var open = false
    /// Stands in when the bar passes no focus binding.
    @FocusState private var unfocused: Bool
    #elseif os(macOS)
    /// Back on the avatar when the panel closes.
    @FocusState private var avatar: Bool
    #endif

    var body: some View {
        if let snapshot = profiles.snapshot, let me = snapshot.current {
            #if os(tvOS)
            Button { open = true } label: { face(me) }
                .focused(focus ?? $unfocused)
                .accessibilityLabel("Profile: \(me.name)")
                .confirmationDialog("Profiles", isPresented: $open, titleVisibility: .visible) {
                    ForEach(snapshot.profiles) { profile in
                        Button(title(profile, snapshot)) { profiles.choose(profile, snapshot, onPin: onPin) }
                    }
                    if Incognito.offered(snapshot) {
                        Button(snapshot.incognito ? "\(Incognito.name), current" : Incognito.pinProfile(snapshot) == nil ? Incognito.name : "\(Incognito.name), locked") { profiles.chooseIncognito(snapshot, onPin: onPin) }
                    }
                    if snapshot.incognito { Button(Incognito.leave) { profiles.leaveIncognito() } }
                    Button("Settings", action: onSettings)
                }
            #elseif os(macOS)
            // No menu on the Mac: the avatar toggles the panel from the right, which the shell draws over the window.
            Button { panel.wrappedValue.toggle() } label: { face(me) }
                .buttonStyle(.plain)
                .focused($avatar)
                .accessibilityLabel("Profile: \(me.name)")
                .accessibilityValue(panel.wrappedValue ? "Open" : "")
                .onChange(of: panel.wrappedValue) { _, open in if !open { avatar = true } }
            #else
            Menu {
                // An inline picker: the system draws the current profile's check and says it is selected.
                Picker("Profiles", selection: Binding { snapshot.active } set: { id in
                    if id == Incognito.id { return profiles.chooseIncognito(snapshot, onPin: onPin) }
                    if let profile = snapshot.profiles.first(where: { $0.id == id }) { profiles.choose(profile, snapshot, onPin: onPin) }
                }) {
                    ForEach(snapshot.profiles) { profile in
                        row(profile, snapshot).tag(profile.id).disabled(profile.syncing)
                    }
                    // The same rows in both states (5.1): Incognito after the profiles, checked while it runs.
                    if Incognito.offered(snapshot) {
                        Label(Incognito.name, image: Incognito.pinProfile(snapshot) == nil ? "hat-glasses" : "lock")
                            .accessibilityLabel(Incognito.spoken(snapshot))
                            .tag(Incognito.id)
                    }
                }
                .pickerStyle(.inline)
                // Directly under the checked row; focus starts on the current row, never here.
                if snapshot.incognito {
                    Button(Incognito.leave, glyph: "log-out") { profiles.leaveIncognito() }
                }
                Divider()
                Button("Settings", glyph: "settings", action: onSettings)
            } label: {
                face(me)
            }
            .menuStyle(.button)
            .buttonStyle(.plain)
            .menuIndicator(.hidden)
            .accessibilityLabel("Profile: \(me.name)")
            #endif
        }
    }

    private func title(_ profile: ProfileInfo, _ snapshot: ProfilesSnapshot) -> String {
        if profile.id == snapshot.active { return "\(profile.name), current" }
        if profile.syncing { return "\(profile.name), syncing" }
        return profile.hasPin ? "\(profile.name), locked" : profile.name
    }

    #if os(iOS)
    @ViewBuilder private func row(_ profile: ProfileInfo, _ snapshot: ProfilesSnapshot) -> some View {
        let words = profile.syncing ? "\(profile.name), syncing" : profile.name
        Group {
            if profile.hasPin && !profile.syncing && profile.id != snapshot.active {
                Label(words, image: "lock")
            } else {
                Text(words)
            }
        }
        .accessibilityLabel(ProfileRules.spoken(profile, active: snapshot.active))
    }
    #endif
}

/// What choosing a profile in a bar's menu or the Mac's panel does (plans/nav-profile.md, plans/profile-panel.md):
/// one rule for both, shared with the picker's tiles (`ProfileRules.choice`).
extension ProfileSession {
    func choose(_ profile: ProfileInfo, _ snapshot: ProfilesSnapshot, onPin: (ProfileInfo) -> Void) {
        switch ProfileRules.choice(profile, in: snapshot) {
        case .none, .close: break
        case .pin: onPin(profile)
        case .switchTo: Task { await switchTo(profile.id, pin: nil) }
        }
    }

    /// The Incognito row: as its tile does, but Leave goes back to this profile (`fromChooser` false).
    func chooseIncognito(_ snapshot: ProfilesSnapshot, onPin: (ProfileInfo) -> Void) {
        switch Incognito.choice(snapshot) {
        case .none, .close: break
        case .pin: onPin(Incognito.profile)
        case .switchTo: Task { await enterIncognito(pin: nil, fromChooser: false) }
        }
    }
}

/// The avatar a bar draws for `ProfileMenu`: the filled circle with no edge (design/DECISION.md: fills, not edges),
/// ringed white only while Settings shows (`lit`).
struct BarAvatar: View {
    let profile: ProfileInfo
    var size: CGFloat = 32
    var lit = false

    var body: some View {
        ProfileAvatar(profile.avatar, name: profile.name, size: size)
            .overlay { if lit { Circle().strokeBorder(.white.opacity(0.92), lineWidth: 2) } }
    }
}

/// A tile: the card style's focus edge and lift, a little room around it.
struct TileButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        CardButtonStyle(radius: Theme.Radius.l, scale: Theme.Focus.scaleTile)
            .makeBody(configuration: configuration)
            .padding(4)
    }
}
