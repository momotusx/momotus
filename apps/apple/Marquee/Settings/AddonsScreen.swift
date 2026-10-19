import SwiftUI

/// How a link gets into the field: a remote types it, only a touch screen or
/// a keyboard pastes one.
#if os(tvOS)
let linkVerb = "Type"
#else
let linkVerb = "Paste"
#endif

/// Installed addons in their order (earlier ones answer first), with a link
/// field to install more. Addon URLs can hold API keys and are never shown.
struct AddonsScreen: View {
    let addons: LiveRecord<AddonsSnapshot>
    /// Show only sources (addons that give streams): where "Add a source" leads.
    @State private var sourcesOnly: Bool
    /// Opened to add a source: the link field takes focus.
    private let focusLink: Bool
    @State private var removing: AddonInfo?
    @Environment(\.metrics) private var metrics

    init(addons: LiveRecord<AddonsSnapshot>, sourcesOnly: Bool = false) {
        self.addons = addons
        _sourcesOnly = State(initialValue: sourcesOnly)
        focusLink = sourcesOnly
    }

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: "Addons")
                Text(sourcesOnly
                    ? "A source is an addon that finds a playable stream for a title. \(linkVerb) a source addon's link to add one."
                    : "Addons bring catalogs, details, streams and subtitles. Order matters: earlier addons answer first.")
                    .font(.text(Theme.type.body))
                    .foregroundStyle(Theme.Palette.muted)
                InstallField(core: addons.core, focusRequest: focusLink ? 1 : 0)
                FindAddonsLink(core: addons.core)
                if let failure = addons.failure {
                    StateLine(symbol: "triangle-alert", text: failure)
                }
                let all = addons.snapshot?.addons ?? []
                ChipRow(
                    label: "Show",
                    chips: [Chip(id: false, title: "All", detail: "\(all.count)"),
                            Chip(id: true, title: "Sources", detail: "\(all.filter(\.isSource).count)")],
                    isSelected: { $0 == sourcesOnly },
                    select: { sourcesOnly = $0 })
                // Moves use the index in the full list, which is the order the core keeps.
                let shown = all.enumerated().filter { !sourcesOnly || $0.element.isSource }
                if sourcesOnly, shown.isEmpty, addons.snapshot != nil {
                    StateLine(symbol: "circle-slash", text: "No source yet. \(linkVerb) a source addon's link above.")
                }
                ForEach(shown, id: \.element.id) { index, addon in
                    AddonCard(
                        addon: addon, first: index == 0, last: index == all.count - 1,
                        toggle: { act("setAddonEnabled", addon, ["enabled": !addon.enabled]) },
                        move: { offset in act("moveAddon", addon, ["to": index + offset]) },
                        remove: { removing = addon })
                }
            }
            .frame(maxWidth: 940, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .topScrim()
        .task(id: addons.taskId) { addons.start() }
        .confirmationDialog(
            "Remove \(removing?.name ?? "this addon")?", isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }),
            titleVisibility: .visible
        ) {
            Button("Remove", role: .destructive) {
                if let addon = removing { act("removeAddon", addon, [:]) }
            }
        } message: {
            Text("Its catalogs, details and streams go away. You can install it again from its link.")
        }
    }

    private func act(_ op: String, _ addon: AddonInfo, _ fields: [String: any Sendable]) {
        var all = fields
        all["url"] = addon.url
        Task { await addons.perform(op, all) }
    }
}

/// The Addons page with its own subscription, for links outside Settings.
struct AddonsPage: View {
    @State private var addons: LiveRecord<AddonsSnapshot>
    let sourcesOnly: Bool

    init(core: CoreClient, sourcesOnly: Bool = false) {
        _addons = State(initialValue: LiveRecord("addons", core: core))
        self.sourcesOnly = sourcesOnly
    }

    var body: some View {
        AddonsScreen(addons: addons, sourcesOnly: sourcesOnly)
    }
}

extension AddonInfo {
    /// Gives streams: a source in the app's words.
    var isSource: Bool { provides.contains("stream") }
}

/// An addon link field with Install; shared by Addons and onboarding. A link
/// whose settings carry a sign-in is installed only once the user says it is theirs.
struct InstallField: View {
    /// Bump to move focus into the field ("Add a source" does).
    var focusRequest = 0
    @State private var install: LinkInstall
    @FocusState private var fieldFocused: Bool

    init(core: CoreClient, focusRequest: Int = 0) {
        self.focusRequest = focusRequest
        _install = State(initialValue: LinkInstall(core: core))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(caps: "Addon link")
                .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
                .tracking(1.6)
                .foregroundStyle(Theme.Palette.muted)
            HStack(spacing: 12) {
                TextField("https://example.org/manifest.json or stremio://", text: $install.link)
                    .textFieldStyle(.plain)
                    .font(.text(Theme.type.body))
                    .autocorrectionDisabled()
                    #if os(iOS)
                    .textInputAutocapitalization(.never)
                    .keyboardType(.URL)
                    #endif
                    .onSubmit(installLink)
                    .focused($fieldFocused)
                    .onChange(of: focusRequest, initial: true) { if focusRequest > 0 { fieldFocused = true } }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
                    .fieldChrome(focused: fieldFocused)
                Button(action: installLink) {
                    Label(install.installing ? "Installing" : "Install", glyph: "plus")
                }
                // The primary only once there is a link to install.
                .buttonStyle(PillButtonStyle(prominent: !install.trimmed.isEmpty))
                .disabled(install.trimmed.isEmpty || install.installing)
            }
            if install.refused != nil {
                StatusDot(tone: .warn, text: LinkInstall.tokenWarning)
                Button(LinkInstall.tokenConfirm) { Task { await install.install(acknowledged: true) } }
                    .buttonStyle(PillButtonStyle())
                    .disabled(install.installing)
            } else if let problem = install.problem {
                StatusDot(tone: .bad, text: problem)
            }
        }
    }

    private func installLink() {
        Task { await install.install() }
    }
}

/// One addon: logo, name and version, host, description, what it provides,
/// and whether it is on. The card opens its actions; Remove comes last and
/// asks first. Phone and Mac press the switch itself, as on the web and
/// Android; on TV the card stays one focus stop and the switch is in its actions.
private struct AddonCard: View {
    let addon: AddonInfo
    let first: Bool
    let last: Bool
    let toggle: () -> Void
    let move: (Int) -> Void
    let remove: () -> Void
    @Environment(\.metrics) private var metrics
    @Environment(\.openURL) private var openURL

    var body: some View {
        Menu {
            // A configure page is a web page: not on TV, which has no browser.
            #if !os(tvOS)
            if let url = addon.configureUrl.flatMap(URL.init(string:)) {
                Button("Configure", glyph: "settings") { openURL(url) }
            }
            #endif
            // Short words on screen, spoken with the addon's name: VoiceOver moves between several addons.
            Button("Move up", glyph: "arrow-up") { move(-1) }.disabled(first)
                .accessibilityLabel("Move \(addon.name) up")
            Button("Move down", glyph: "arrow-down") { move(1) }.disabled(last)
                .accessibilityLabel("Move \(addon.name) down")
            Button(addon.enabled ? "Turn off" : "Turn on", glyph: "power", action: toggle)
                .accessibilityLabel(addon.enabled ? "Turn \(addon.name) off" : "Turn \(addon.name) on")
            Divider()
            Button("Remove", glyph: "trash", role: .destructive, action: remove)
                .accessibilityLabel("Remove \(addon.name)")
        } label: {
            card
        }
        .menuStyle(.button)
        .buttonStyle(CardButtonStyle(radius: Theme.Radius.m))
        #if os(macOS)
        .menuIndicator(.hidden)
        #endif
        .accessibilityHint("Opens its actions: move, turn off, remove")
        #if !os(tvOS)
        .overlay(alignment: .topTrailing) {
            Button(action: toggle) {
                SwitchMark(on: addon.enabled).padding(Theme.Space.s).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(addon.name)
            .accessibilityValue(addon.enabled ? "On" : "Off")
            .accessibilityAddTraits(.isToggle)
        }
        #endif
    }

    private var card: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top, spacing: Theme.Space.s) {
                logo
                VStack(alignment: .leading, spacing: 4) {
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text(addon.name).font(.text(Theme.type.headline + 1, weight: .semibold, relativeTo: .headline))
                        Text(addon.version).font(.text(Theme.type.caption + 1, relativeTo: .caption)).foregroundStyle(Theme.Palette.muted)
                    }
                    Text(addon.host).font(.mono(Theme.type.caption + 1)).foregroundStyle(Theme.Palette.muted)
                    if let description = addon.description {
                        Text(description)
                            .font(.text(Theme.type.callout, relativeTo: .callout))
                            .foregroundStyle(Theme.Palette.muted)
                            .lineLimit(2)
                    }
                    Text(provides)
                        .font(.text(Theme.type.callout, weight: .medium, relativeTo: .callout))
                        .foregroundStyle(Theme.Palette.text)
                }
                .opacity(addon.enabled ? 1 : 0.55)
                Spacer(minLength: 0)
                #if os(tvOS)
                SwitchMark(on: addon.enabled)
                #else
                // The pressable switch lies over this place.
                SwitchMark(on: addon.enabled).hidden().accessibilityHidden(true)
                #endif
            }
        }
        .multilineTextAlignment(.leading)
        .foregroundStyle(Theme.Palette.text)
        .padding(Theme.Space.s)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
    }

    private var logo: some View {
        RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous)
            .fill(Theme.Palette.raised)
            .overlay {
                if addon.logo != nil {
                    ArtImage(url: addon.logo, mode: .fit, maxPixels: 160).padding(6)
                } else {
                    Text(addon.name.prefix(1).uppercased()).display(Theme.type.title3)
                }
            }
            .frame(width: 48, height: 48)
            .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous))
            .accessibilityHidden(true)
    }

    /// "Catalogs, Details for movie, series"
    private var provides: String {
        let words = addon.provides.map(providesWord)
        let types = addon.types.isEmpty ? "" : " for \(addon.types.joined(separator: ", "))"
        return words.joined(separator: ", ") + types
    }
}

/// On or off as a switch, the state only; a button around it acts. Drawn
/// by `SwitchLook`: a white track with a dark knob when on, an empty track
/// with the control edge when off. TV and wide windows say On or Off beside it.
struct SwitchMark: View {
    let on: Bool
    @Environment(\.metrics) private var metrics

    var body: some View {
        let look = SwitchLook.of(on: on)
        HStack(spacing: 10) {
            Capsule()
                .fill(look.track.map(color) ?? .clear)
                .overlay { if let edge = look.edge { Capsule().strokeBorder(color(edge), lineWidth: 1.5) } }
                .frame(width: Self.track.width, height: Self.track.height)
                .overlay(alignment: on ? .trailing : .leading) {
                    Circle().fill(color(look.knob)).padding(Self.track.height * 0.18)
                }
            if SwitchLook.showsWord(wide: metrics.wide) {
                // Sized by the longer word, so the tracks of a list line up.
                ZStack(alignment: .leading) {
                    Text("Off").hidden()
                    Text(on ? "On" : "Off")
                }
                .font(.text(Theme.type.callout, weight: .semibold, relativeTo: .callout))
                .foregroundStyle(on ? Theme.Palette.text : Theme.Palette.muted)
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(on ? "On" : "Off")
    }

    #if os(tvOS)
    private static let track = CGSize(width: 60, height: 34)
    #else
    private static let track = CGSize(width: 38, height: 22)
    #endif

    private func color(_ token: SwitchLook.Token) -> Color {
        switch token {
        case .primary: Theme.Palette.primary
        case .onPrimary: Theme.Palette.onPrimary
        case .control: Theme.Palette.control
        case .muted: Theme.Palette.muted
        }
    }
}
