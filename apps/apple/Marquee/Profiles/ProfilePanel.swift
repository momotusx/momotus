import SwiftUI

#if os(macOS)
/// The panel over the window from the right (variant 7): 420 wide, the page dimmed to 35 percent black under it. The
/// capsule and Search are toolbar items and stay live above it; the shell closes it when either is used and when the
/// player starts. Reduced motion fades it instead of sliding it in.
struct ProfilePanelLayer: View {
    @Binding var open: Bool
    let onPin: (ProfileInfo) -> Void
    let onManage: () -> Void
    let onSettings: () -> Void
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        ZStack(alignment: .trailing) {
            if open {
                Color.black.opacity(0.35)
                    .ignoresSafeArea()
                    .contentShape(Rectangle())
                    .onTapGesture { open = false }
                    .accessibilityHidden(true)
                    .transition(.opacity)
                ProfilePanel(close: { open = false }, onPin: onPin, onManage: onManage, onSettings: onSettings)
                    // Narrow windows keep 48 of the page beside it.
                    .containerRelativeFrame(.horizontal) { width, _ in min(420, width - 48) }
                    .transition(reduceMotion ? .opacity : .move(edge: .trailing))
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .trailing)
        .animation(Theme.Motion.panel, value: open)
    }
}

private struct ProfilePanel: View {
    let close: () -> Void
    let onPin: (ProfileInfo) -> Void
    let onManage: () -> Void
    let onSettings: () -> Void
    @Environment(ProfileSession.self) private var profiles
    @Environment(CoreClient.self) private var core
    @Environment(\.accessibilityReduceTransparency) private var solid
    @State private var last: [String: LastWatched] = [:]
    @FocusState private var focus: String?

    var body: some View {
        if let snapshot = profiles.snapshot, let card = ProfilePanelModel.card(snapshot, last: last) {
            let rows = ProfilePanelModel.rows(snapshot, last: last)
            let manages = ProfilePanelModel.manages(snapshot)
            VStack(alignment: .leading, spacing: 0) {
                header
                ScrollView(.vertical) {
                    VStack(alignment: .leading, spacing: 0) {
                        current(card)
                        if !rows.isEmpty {
                            Text(ProfilePanelModel.switchTo)
                                .textFont(13, weight: .semibold)
                                .foregroundStyle(Theme.Palette.dim)
                                .padding(.horizontal, 4)
                                .padding(.top, 18)
                                .padding(.bottom, 8)
                                .accessibilityAddTraits(.isHeader)
                            VStack(spacing: 6) {
                                ForEach(rows) { row($0, snapshot) }
                            }
                        }
                    }
                    // Room for the focus ring inside the scroll view's clip.
                    .padding(.horizontal, 8)
                    .padding(.bottom, 8)
                }
                .scrollIndicators(.automatic)
                .padding(.horizontal, -8)
                foot(manages)
            }
            .padding(.top, 18)
            .padding(.horizontal, 20)
            .padding(.bottom, 24)
            .frame(maxHeight: .infinity, alignment: .top)
            .background { surface.ignoresSafeArea() }
            .accessibilityElement(children: .contain)
            .accessibilityAddTraits(.isModal)
            .accessibilityLabel(ProfilePanelModel.title)
            .onExitCommand(perform: close)
            .onAppear {
                // The first thing to act on: the first profile to switch to, else Manage profiles, else Settings.
                let first = rows.first { !$0.disabled }?.id ?? (manages ? "manage" : "settings")
                Task { focus = first }
            }
            // Asked on open and again when a profile finishes syncing; never in a kids session or Incognito. A
            // failure leaves names and avatars.
            .task(id: snapshot.profiles.filter(\.syncing).map(\.id)) {
                guard ProfilePanelModel.asks(snapshot) else { return }
                let asked = snapshot.active
                guard let list = try? await core.call("profilesLastWatched", as: [LastWatched].self),
                      profiles.snapshot?.active == asked else { return }
                last = Dictionary(list.map { ($0.profile, $0) }, uniquingKeysWith: { a, _ in a })
            }
        }
    }

    /// `glassStrong` with the token blur, a hairline at its leading edge; solid `surface` under Reduce Transparency.
    @ViewBuilder private var surface: some View {
        if solid {
            Theme.Palette.surface
        } else {
            ZStack {
                Rectangle().fill(.ultraThinMaterial)
                Theme.Palette.glassStrong
            }
            .overlay(alignment: .leading) { Theme.Palette.edge.frame(width: 1) }
            .shadow(color: .black.opacity(0.45), radius: 40, x: -30)
        }
    }

    private var header: some View {
        HStack {
            Text(ProfilePanelModel.title)
                .textFont(28, weight: .bold)
                .foregroundStyle(Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            Spacer(minLength: 0)
            Button(action: close) {
                Glyph("x", size: 16).foregroundStyle(Theme.Palette.text).frame(width: 32, height: 32)
            }
            .buttonStyle(PanelButtonStyle(shape: Circle(), fill: 0.12))
            .keyboardShortcut(.cancelAction)
            .focused($focus, equals: "close")
            .accessibilityLabel(ProfilePanelModel.close)
        }
        .frame(height: 36)
    }

    /// The current profile on the backdrop of its last title, under a ramp dark enough for 4.5:1 over a white sky.
    private func current(_ card: ProfilePanelModel.Card) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            Spacer(minLength: 0)
            HStack(spacing: 14) {
                ProfileAvatar(card.profile.avatar, name: card.profile.name, size: 64)
                VStack(alignment: .leading, spacing: 2) {
                    Text(card.profile.name)
                        .textFont(22, weight: .bold)
                        .foregroundStyle(Theme.Palette.text)
                        .lineLimit(2)
                    Text(card.sub)
                        .textFont(13)
                        .foregroundStyle(Theme.Palette.muted)
                }
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("\(card.profile.name), current profile, \(card.sub)")
            if let last = card.last {
                (Text("\(ProfilePanelModel.lastWatched) ") + Text(last.label).fontWeight(.semibold).foregroundStyle(Theme.Palette.text))
                    .textFont(13)
                    .foregroundStyle(Theme.Palette.muted)
                    .lineLimit(2)
                    .padding(.top, 10)
                    .accessibilityLabel("\(ProfilePanelModel.lastWatched) \(last.spoken)")
            }
            if card.leave {
                // The gate takes over at once; a failure shows in the picker, as the menu's Leave did.
                Button { close(); profiles.leaveIncognito() } label: {
                    Label(Incognito.leave, glyph: "log-out", size: 16)
                        .textFont(14, weight: .semibold)
                        .foregroundStyle(.white)
                        .padding(.horizontal, 18)
                        .frame(height: 44)
                }
                .buttonStyle(PanelButtonStyle(shape: Capsule(), fill: 0.16))
                .focused($focus, equals: "leave")
                .padding(.top, 12)
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, minHeight: 196, alignment: .leading)
        .background {
            ZStack {
                Theme.Palette.raised
                if let art = card.last?.art {
                    PanelArt(url: art, pixels: ArtSize.backdrop, ramp: true)
                }
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
        .padding(.top, 18)
        .accessibilityElement(children: .contain)
    }

    private func row(_ row: ProfilePanelModel.Row, _ snapshot: ProfilesSnapshot) -> some View {
        Button {
            close()
            if let profile = row.profile {
                profiles.choose(profile, snapshot, onPin: onPin)
            } else {
                profiles.chooseIncognito(snapshot, onPin: onPin)
            }
        } label: {
            HStack(spacing: 14) {
                if let profile = row.profile {
                    ProfileAvatar(profile.avatar, name: profile.name, size: 48)
                } else {
                    // At rest a `raised` circle with the white glyph; sea blue only while it runs (DECISION, Profiles).
                    ProfileAvatar(ProfileRules.Avatar(glyph: Incognito.id, color: nil), name: Incognito.name, size: 48)
                }
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 8) {
                        Text(row.name)
                            .textFont(16, weight: .semibold)
                            .foregroundStyle(row.disabled ? Theme.Palette.dim : Theme.Palette.text)
                            .lineLimit(1)
                        if row.lock { Glyph("lock", size: 14).foregroundStyle(Theme.Palette.dim) }
                        if row.kids { kidsBadge }
                    }
                    if let line = row.line {
                        Text(line)
                            .textFont(13)
                            .foregroundStyle(Theme.Palette.dim)
                            .lineLimit(1)
                    }
                }
                Spacer(minLength: 0)
                if let art = row.art {
                    PanelArt(url: art, pixels: ArtSize.poster, ramp: false)
                        .frame(width: 84, height: 48)
                        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                }
            }
            .padding(.horizontal, 12)
            .frame(maxWidth: .infinity, minHeight: 76, maxHeight: 76, alignment: .leading)
        }
        .buttonStyle(PanelButtonStyle(shape: RoundedRectangle(cornerRadius: 14, style: .continuous), fill: 0.06))
        .disabled(row.disabled)
        .focused($focus, equals: row.id)
        .accessibilityLabel(row.spoken)
    }

    private var kidsBadge: some View {
        Text(ProfilePanelModel.kids)
            .textFont(11, weight: .bold)
            .foregroundStyle(Theme.Palette.onPrimary)
            .padding(.horizontal, 7)
            .frame(height: 18)
            .background(Capsule().fill(Theme.Palette.primary))
    }

    /// Manage profiles and Settings side by side, stacked when a narrow panel has no room for both.
    private func foot(_ manages: Bool) -> some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 10) { pills(manages) }
            VStack(spacing: 10) { pills(manages) }
        }
        .padding(.top, 12)
    }

    @ViewBuilder private func pills(_ manages: Bool) -> some View {
        if manages {
            pill(ProfilePanelModel.manage, glyph: "users", id: "manage") { close(); onManage() }
        }
        pill(ProfilePanelModel.settings, glyph: "settings", id: "settings") { close(); onSettings() }
    }

    private func pill(_ title: String, glyph: String, id: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Label(title, glyph: glyph, size: 16)
                .textFont(14, weight: .semibold)
                .foregroundStyle(.white)
                .lineLimit(1)
                .fixedSize()
                .padding(.horizontal, 18)
                .frame(maxWidth: .infinity, minHeight: 44)
        }
        .buttonStyle(PanelButtonStyle(shape: Capsule(), fill: 0.16))
        .focused($focus, equals: id)
    }
}

/// The panel's art: nothing until it loads (a failure leaves the card plain and the row without a thumbnail), then
/// a short fade; the card's under its ramp.
private struct PanelArt: View {
    let url: String
    let pixels: Int
    let ramp: Bool
    @State private var art: DecodedArt?

    var body: some View {
        Color.clear
            .overlay {
                if let art {
                    Image(decorative: art.image, scale: 1)
                        .resizable()
                        .aspectRatio(contentMode: .fill)
                        .overlay {
                            if ramp {
                                LinearGradient(
                                    stops: [.init(color: .black.opacity(0.25), location: 0), .init(color: .black.opacity(0.7), location: 0.4),
                                            .init(color: .black.opacity(0.88), location: 1)],
                                    startPoint: .top, endPoint: .bottom)
                            }
                        }
                        .transition(.opacity)
                }
            }
            .clipped()
            .accessibilityHidden(true)
            .task(id: url) {
                let start = ContinuousClock.now
                let loaded = await ArtLoader.shared.image(url, maxPixels: pixels)
                withAnimation(reveal(since: start)) { art = loaded }
            }
    }
}

/// A row, a pill or the round Close: a white fill at `fill`, a step lighter under the pointer and while pressed, the
/// key ring for keyboard focus.
private struct PanelButtonStyle<S: InsettableShape>: ButtonStyle {
    let shape: S
    let fill: Double

    func makeBody(configuration: Configuration) -> some View {
        Look(configuration: configuration, shape: shape, fill: fill)
    }

    private struct Look: View {
        let configuration: ButtonStyleConfiguration
        let shape: S
        let fill: Double
        @KeyboardFocused private var focused
        @Environment(\.isEnabled) private var enabled
        @State private var hovered = false

        var body: some View {
            configuration.label
                .background(shape.fill(.white.opacity(fill + (enabled && (hovered || configuration.isPressed) ? 0.06 : 0))))
                .contentShape(shape)
                .modifier(FocusRing(shape: shape, active: focused))
                .onHover { hovered = $0 }
        }
    }
}
#endif
