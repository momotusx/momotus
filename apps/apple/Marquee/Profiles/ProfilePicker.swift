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
    @Environment(ProfileSession.self) private var profiles
    @Environment(\.dismiss) private var dismiss
    @Environment(\.metrics) private var metrics
    /// The profile whose PIN is asked.
    @State private var keypad: ProfileInfo?
    @State private var said: String?
    @State private var showManage = false
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
                Text("Opening \(snapshot.profiles.first { $0.id == target }?.name ?? "the profile")")
                    .font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                    .foregroundStyle(Theme.Palette.text)
            }
            .padding(.top, Theme.Space.xl)
        } else if let target = keypadTarget(snapshot) {
            PinKeypad(
                title: "Enter the PIN for \(target.name)",
                initial: failedSwitch(for: target),
                note: ownerNote(target, snapshot),
                reset: resetAction(target, snapshot),
                submit: { pin in await enter(target, pin: pin, snapshot: snapshot) },
                cancel: { leaveKeypad(target) })
            .id(target.id)
        } else {
            header(snapshot)
            if let said = said ?? switchWords {
                Text(said)
                    .font(.text(Theme.type.body, weight: .semibold))
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
            if snapshot.locked {
                Text("This profile is locked. Choose a profile, then enter its PIN.")
                    .font(.text(Theme.type.body))
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
        let count = snapshot.profiles.count + (addable ? 1 : 0)
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
                        .font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                        if profile.syncing {
                            Text("Syncing").font(.text(Theme.type.caption, relativeTo: .caption)).foregroundStyle(Theme.Palette.muted)
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
            if addable {
                Button { showAdd = true } label: {
                    VStack(spacing: 10) {
                        RoundedRectangle(cornerRadius: tile * 0.22, style: .continuous)
                            .fill(Theme.Palette.raised)
                            .overlay(RoundedRectangle(cornerRadius: tile * 0.22, style: .continuous).strokeBorder(Theme.Palette.control))
                            .overlay(Glyph("plus", size: tile * 0.3))
                            .frame(width: tile, height: tile)
                        Text("Add profile").font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
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

    /// The keypad's profile: the one chosen, or a switch that failed for a PIN.
    private func keypadTarget(_ snapshot: ProfilesSnapshot) -> ProfileInfo? {
        if let keypad { return snapshot.profiles.first { $0.id == keypad.id } }
        guard mode == .gate, let target = profiles.switching, profiles.switchProblem != nil else { return nil }
        return snapshot.profiles.first { $0.id == target && $0.hasPin }
    }

    private func failedSwitch(for target: ProfileInfo) -> KeypadResult? {
        guard mode == .gate, profiles.switching == target.id, let problem = profiles.switchProblem else { return nil }
        return .of(problem)
    }

    /// A switch to a profile with no PIN that failed: the reason, over the tiles.
    private var switchWords: String? {
        guard mode == .gate, profiles.switching != nil, let problem = profiles.switchProblem else { return nil }
        if case .problem(let text) = KeypadResult.of(problem) { return text }
        return "That did not work. Try again."
    }

    private func pick(_ profile: ProfileInfo, _ snapshot: ProfilesSnapshot) {
        guard !profile.syncing else { return }
        said = nil
        if profile.id == snapshot.active && !snapshot.locked {
            switch mode {
            case .push: dismiss()
            case .gate:
                // A failed switch is given up, and this profile is the answer.
                if profiles.switching != nil { profiles.cancelSwitch() }
                profiles.markChosen()
            }
        } else if profile.hasPin {
            keypad = profile
        } else {
            Task { await profiles.switchTo(profile.id, pin: nil) }
        }
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

    private func leaveKeypad(_ target: ProfileInfo) {
        keypad = nil
        // A switch that failed is given up: the current profile opens again.
        if mode == .gate, profiles.switching != nil { profiles.cancelSwitch() }
        focused = target.id
    }

    private func ownerNote(_ target: ProfileInfo, _ snapshot: ProfilesSnapshot) -> String? {
        guard target.isOwner else { return nil }
        #if os(tvOS)
        return "Forgot the PIN? Reset it from Marquee on your phone or Mac. This device gets the change at its next sync."
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

/// The active profile's avatar, opening the picker: Home's header on the
/// phone, the end of the TV's top bar.
struct ProfileButton: View {
    var size: CGFloat = 32
    let action: () -> Void
    @Environment(ProfileSession.self) private var profiles

    var body: some View {
        if let me = profiles.snapshot?.current {
            Button(action: action) {
                ProfileAvatar(me.avatar, name: me.name, size: size)
                    .overlay(RoundedRectangle(cornerRadius: size * 0.22, style: .continuous).strokeBorder(.white.opacity(0.3), lineWidth: 1))
                    #if os(iOS)
                    // A 44 pt target around the 32 pt avatar.
                    .frame(minWidth: 44, minHeight: 44)
                    .contentShape(Rectangle())
                    #endif
            }
            #if os(tvOS)
            .buttonStyle(NavLinkStyle(on: false))
            #else
            .buttonStyle(.plain)
            #endif
            .accessibilityLabel("Profile: \(me.name), switch profile")
        }
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
