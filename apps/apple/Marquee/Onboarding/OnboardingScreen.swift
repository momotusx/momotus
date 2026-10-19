import Network
import SwiftUI

/// First run as a full-screen flow, one decision per step: the doors (I use
/// Stremio, Start fresh, I already use Marquee), then the recommended addons
/// with a switch each (plans/m13-default-addons.md 6), then the setup check,
/// where every line has a dot, a word and, when something is missing, its
/// fix. Start fresh and a finished import lead to the addons; joining never
/// does. Settings opens the check step alone as a page.
struct OnboardingScreen: View {
    enum Step { case doors, defaults, check }

    let core: CoreClient
    /// Ends the flow (Browse first, Done); nil when the check is a page.
    let close: (() -> Void)?
    @State private var step: Step
    @State private var addons: LiveRecord<AddonsSnapshot>
    /// The recommended addons step's switches.
    @State private var defaults: DefaultsStep
    /// The defaults the check's fixes read, again whenever the list changes.
    @State private var fixer: DefaultsStep
    /// Where Back on the addons step goes: the doors, or the check that opened it.
    @State private var defaultsBack = Step.doors
    /// The addons step follows a Stremio import: defaults it brought say so.
    @State private var imported = false
    @State private var pingMs: Int?
    @State private var pinged = false
    @State private var online = true
    @State private var busy: String?
    @State private var note: (ok: Bool, text: String)?
    @State private var showImport = false
    @State private var showPairing = false
    /// TV: the doors step enters on the first door, not on Browse first.
    @FocusState private var firstDoor: Bool
    /// TV: the addons step enters on Continue, so one press keeps them all.
    @FocusState private var onContinue: Bool
    @Environment(\.metrics) private var metrics
    @Environment(SyncModel.self) private var sync

    init(core: CoreClient, start: Step = .doors, close: (() -> Void)? = nil) {
        self.core = core
        self.close = close
        _step = State(initialValue: start)
        _addons = State(initialValue: LiveRecord("addons", core: core))
        _defaults = State(initialValue: DefaultsStep(core: core, scope: .all))
        _fixer = State(initialValue: DefaultsStep(core: core, scope: .all))
    }

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.l) {
                switch step {
                case .doors: doorsStep
                case .defaults: defaultsStep
                case .check: checkStep
                }
            }
            .frame(maxWidth: metrics.wide ? 760 : .infinity, alignment: .leading)
            .frame(maxWidth: .infinity)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .topScrim()
        .animation(Theme.Motion.panel, value: step)
        .navigationDestination(isPresented: $showImport) {
            ImportScreen(core: core) {
                showImport = false
                imported = true
                openDefaults(from: .doors)
            }
        }
        // A join is the way in: Done goes Home, as the other doors end.
        .navigationDestination(isPresented: $showPairing) { PairingScreen(core: core, done: close) }
        .task(id: addons.taskId) { addons.start() }
        .task(id: addons.snapshot) { if step == .check { await fixer.load() } }
        .task(id: step) {
            if step == .check { await fixer.load() }
            if step == .defaults, defaults.list == nil { await defaults.load() }
        }
        .task { await pingLoop() }
        .task { await watchNetwork() }
    }

    // MARK: steps

    private var doorsStep: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            HStack(alignment: .firstTextBaseline) {
                Wordmark(size: 22)
                Spacer()
                if let close {
                    Button("Browse first") { Task { await browseFirst(close) } }
                        .buttonStyle(.textLink)
                        .font(.text(Theme.type.callout, weight: .semibold, relativeTo: .callout))
                        .foregroundStyle(Theme.Palette.text)
                }
            }
            Text("How would you like to begin?")
                .display(metrics.wide ? 44 : Theme.type.title1 + 4)
                .foregroundStyle(Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            Text("No account needed. Everything stays on this device until you turn on sync.")
                .font(.text(Theme.type.body))
                .foregroundStyle(Theme.Palette.muted)
            let doors = Group {
                Button { showImport = true } label: {
                    Door(symbol: "door-open", title: "I use Stremio", text: "Bring your addons, library and Continue Watching over with a code.")
                }
                .focused($firstDoor)
                Button { openDefaults(from: .doors) } label: {
                    Door(symbol: "sparkle", title: "Start fresh", text: "Begin with the recommended addons for catalogs, streams and subtitles.")
                }
                if !sync.joined {
                    Button { showPairing = true } label: {
                        Door(symbol: "qr-code", title: "I already use Marquee", text: "Join your other device's sync account with a code. Library and addons follow.")
                    }
                }
            }
            .buttonStyle(.row)
            if metrics.wide {
                // On one row the doors share one height.
                HStack(alignment: .top, spacing: Theme.Space.s) { doors }
                    .fixedSize(horizontal: false, vertical: true)
            } else {
                VStack(spacing: 12) { doors }
            }
            if let note, !note.ok {
                StatusDot(tone: .bad, text: note.text)
            }
        }
        .defaultFocus($firstDoor, true)
    }

    private var defaultsStep: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            stepHeading(DefaultsRules.title, DefaultsRules.lead)
            RecommendedAddons(model: defaults, imported: imported)
            #if !os(tvOS)
            // TV adds more from Settings: typing a link with a remote is no first step.
            VStack(alignment: .leading, spacing: Theme.Space.s) {
                Text("Add another addon")
                    .font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                    .foregroundStyle(Theme.Palette.text)
                    .accessibilityAddTraits(.isHeader)
                InstallField(core: core)
                FindAddonsLink(core: core)
            }
            .padding(.top, Theme.Space.s)
            #endif
            HStack {
                Button("Back") { step = defaultsBack }.buttonStyle(PillButtonStyle())
                Spacer()
                Button(defaults.busy ? "Adding" : "Continue") {
                    Task { if await defaults.install() { step = .check } }
                }
                .buttonStyle(PillButtonStyle(prominent: true))
                .disabled(defaults.busy)
                .focused($onContinue)
            }
            .padding(.top, Theme.Space.s)
        }
        .defaultFocus($onContinue, true)
    }

    private var checkStep: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            stepHeading("Setup check", "Every line says what it found, and what to do when something is missing.")
            VStack(alignment: .leading, spacing: 14) {
                ForEach(checks, id: \.label) { check in
                    HStack(alignment: .firstTextBaseline, spacing: 12) {
                        Text(check.label)
                            .font(.text(Theme.type.callout, weight: .semibold, relativeTo: .callout))
                            .foregroundStyle(Theme.Palette.muted)
                            .frame(width: metrics.wide ? 120 : 96, alignment: .leading)
                        // The fix sits under its status, so neither squeezes the other.
                        VStack(alignment: .leading, spacing: 8) {
                            StatusDot(tone: check.tone, text: check.text)
                            if let fix = check.fix {
                                Button(fix.label, action: fix.run).buttonStyle(PillButtonStyle())
                            }
                        }
                        Spacer(minLength: 0)
                    }
                }
                if let note {
                    StatusDot(tone: note.ok ? .ok : .bad, text: note.text)
                }
            }
            .padding(Theme.Space.s + 4)
            .frame(maxWidth: .infinity, alignment: .leading)
            .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
            if let close {
                HStack {
                    Button("Back") { step = .defaults }.buttonStyle(PillButtonStyle())
                    Spacer()
                    Button("Done", action: close).buttonStyle(PillButtonStyle(prominent: true))
                }
                .padding(.top, Theme.Space.s)
            }
        }
    }

    private func stepHeading(_ title: String, _ text: String) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title)
                .display(metrics.wide ? 44 : Theme.type.title1 + 4)
                .foregroundStyle(Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            Text(text)
                .font(.text(Theme.type.body))
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(.top, close == nil ? 0 : Theme.Space.s)
    }

    // MARK: the check

    private struct Check {
        let label: String
        let tone: StatusDot.Tone
        let text: String
        var fix: (label: String, run: () -> Void)?
    }

    private var checks: [Check] {
        let catalogs = names("catalog")
        let sources = names("stream")
        let subtitles = names("subtitles")
        return [
            // The core's latency only matters when it fails.
            Check(
                label: "App engine", tone: pingMs != nil ? .ok : pinged ? .bad : .warn,
                text: pingMs != nil ? "Running" : pinged ? "Not answering" : "Starting"),
            Check(label: "Network", tone: online ? .ok : .bad, text: online ? "Online" : "Offline"),
            Check(label: "Display", tone: .ok, text: DeviceCaps.displayWords),
            Check(
                label: "Catalogs", tone: catalogs.isEmpty ? .idle : .ok, text: catalogs.isEmpty ? "None yet" : catalogs.joined(separator: ", "),
                fix: catalogs.isEmpty ? fix("catalogs") : nil),
            Check(
                label: "Sources", tone: sources.isEmpty ? .idle : .ok,
                text: sources.isEmpty ? "None yet: titles cannot play" : sources.joined(separator: ", "),
                fix: sources.isEmpty ? fix("streams") : nil),
            Check(
                label: "Subtitles", tone: subtitles.isEmpty ? .idle : .ok, text: subtitles.isEmpty ? "None yet" : subtitles.joined(separator: ", "),
                fix: subtitles.isEmpty ? fix("subtitles") : nil),
        ]
    }

    private func names(_ kind: String) -> [String] {
        (addons.snapshot?.addons ?? []).filter { $0.enabled && $0.provides.contains(kind) }.map(\.name)
    }

    /// A missing line's fix from the recommended addons: the group's default
    /// turned on in one press, or the addons step when some are missing.
    private func fix(_ group: String) -> (label: String, run: () -> Void)? {
        switch DefaultsRules.fix(group: group, in: fixer.list) {
        case .turnOn(let key, let name)?:
            return ("Turn \(name) on", { Task { await turnOn(key, name) } })
        case .review?:
            return (DefaultsRules.title, { openDefaults(from: .check) })
        case nil:
            return nil
        }
    }

    private func turnOn(_ key: String, _ name: String) async {
        guard busy == nil else { return }
        busy = key
        note = nil
        defer { busy = nil }
        note = await fixer.turnOn(key) ? (true, "Turned \(name) on") : (false, "Could not turn \(name) on: \(fixer.failure ?? "unknown error")")
    }

    /// The addons step with every switch where it starts.
    private func openDefaults(from back: Step) {
        defaultsBack = back
        defaults = DefaultsStep(core: core, scope: .all)
        step = .defaults
    }

    /// Browse first: a new profile still gets the recommended addons, all on;
    /// one that has a list keeps it as it is.
    private func browseFirst(_ close: () -> Void) async {
        if addons.snapshot?.fresh == true {
            guard await defaults.install() else {
                note = (false, "Could not add the recommended addons: \(defaults.failure ?? "unknown error")")
                return
            }
        }
        close()
    }

    /// Liveness from the core itself, every few seconds while this screen is open.
    private func pingLoop() async {
        while !Task.isCancelled {
            pingMs = await core.ping()
            pinged = true
            try? await Task.sleep(for: .seconds(5))
        }
    }

    private func watchNetwork() async {
        let monitor = NWPathMonitor()
        let paths = AsyncStream { continuation in
            monitor.pathUpdateHandler = { continuation.yield($0.status == .satisfied) }
            continuation.onTermination = { _ in monitor.cancel() }
        }
        monitor.start(queue: .main)
        for await satisfied in paths { online = satisfied }
    }
}

/// The recommended addons grouped, each with its line and a switch; one
/// already in the list and on says so instead. Onboarding and the Home
/// card's Review show it.
struct RecommendedAddons: View {
    let model: DefaultsStep
    var imported = false

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            if model.list == nil {
                if let failure = model.failure {
                    StateLine(symbol: "triangle-alert", text: failure, action: ("Try again", { Task { await model.load() } }))
                } else {
                    Label("Reading the list", glyph: "clock")
                        .font(.text(Theme.type.body))
                        .foregroundStyle(Theme.Palette.muted)
                }
            }
            ForEach(DefaultsRules.groups, id: \.id) { group in
                let rows = model.rows.filter { $0.group == group.id }
                if !rows.isEmpty {
                    VStack(alignment: .leading, spacing: 10) {
                        Text(caps: group.title)
                            .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
                            .tracking(1.6)
                            .foregroundStyle(Theme.Palette.muted)
                            .accessibilityAddTraits(.isHeader)
                        ForEach(rows) { addon in
                            row(addon)
                            // Under the catalog's own row, where the switch was moved.
                            if addon.key == DefaultsRules.catalogKey, DefaultsRules.warns(model.choices) {
                                StatusDot(tone: .warn, text: DefaultsRules.catalogWarning)
                            }
                        }
                    }
                }
            }
            if model.list != nil, let failure = model.failure {
                StatusDot(tone: .bad, text: failure)
            }
        }
    }

    @ViewBuilder private func row(_ addon: DefaultAddonInfo) -> some View {
        if DefaultsRules.hasSwitch(addon) {
            let on = model.isOn(addon)
            Button { model.flip(addon) } label: {
                DefaultRow(addon: addon) { SwitchMark(on: on) }
            }
            .buttonStyle(.row)
            // On the button itself: a new element around it would not press it (VoiceOver, Full Keyboard Access).
            .accessibilityLabel("\(addon.name). \(addon.line)")
            .accessibilityValue(on ? "On" : "Off")
            .accessibilityAddTraits(.isToggle)
        } else {
            DefaultRow(addon: addon) {
                Text(DefaultsRules.presentWord(imported: imported))
                    .font(.text(Theme.type.callout, weight: .semibold, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
            }
            .accessibilityElement(children: .combine)
        }
    }
}

/// One default: name and its line, with a switch or a word at the end.
private struct DefaultRow<End: View>: View {
    let addon: DefaultAddonInfo
    @ViewBuilder let end: End

    var body: some View {
        HStack(alignment: .center, spacing: Theme.Space.s) {
            VStack(alignment: .leading, spacing: 4) {
                Text(addon.name).font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                Text(addon.line)
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 0)
            end
        }
        .multilineTextAlignment(.leading)
        .foregroundStyle(Theme.Palette.text)
        .padding(Theme.Space.s)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
    }
}

/// The Home card's Review: the recommended addons the profile misses, with
/// their switches on; Continue installs them and goes back.
struct RecommendedAddonsPage: View {
    @State private var model: DefaultsStep
    @FocusState private var onContinue: Bool
    @Environment(\.metrics) private var metrics
    @Environment(\.dismiss) private var dismiss

    init(core: CoreClient) {
        _model = State(initialValue: DefaultsStep(core: core, scope: .missing))
    }

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: DefaultsRules.title)
                Text(DefaultsRules.lead)
                    .font(.text(Theme.type.body))
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
                RecommendedAddons(model: model)
                Button(model.busy ? "Adding" : "Continue") {
                    Task { if await model.install() { dismiss() } }
                }
                .buttonStyle(PillButtonStyle(prominent: true))
                .disabled(model.busy || model.list == nil)
                .focused($onContinue)
                .padding(.top, Theme.Space.s)
            }
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .topScrim()
        .defaultFocus($onContinue, true)
        .task { await model.load() }
    }
}

/// A big choice: icon, a title in the display face, one line, chevron.
private struct Door: View {
    let symbol: String
    let title: String
    let text: String
    @State private var hovered = false
    @Environment(\.metrics) private var metrics

    var body: some View {
        // Side by side on a row the doors are narrow: the icon and chevron go above the words.
        let layout = metrics.wide
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: Theme.Space.s))
            : AnyLayout(HStackLayout(spacing: Theme.Space.s))
        layout {
            if metrics.wide {
                HStack {
                    Glyph(symbol, size: 26)
                    Spacer(minLength: 0)
                    Glyph("chevron-right").foregroundStyle(Theme.Palette.muted)
                }
            } else {
                Glyph(symbol, size: 26).frame(width: 36)
            }
            VStack(alignment: .leading, spacing: 4) {
                Text(title).display(Theme.type.title3)
                Text(text)
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if !metrics.wide {
                Spacer(minLength: 0)
                Glyph("chevron-right").foregroundStyle(Theme.Palette.muted)
            }
        }
        .frame(maxHeight: .infinity, alignment: .top)
        .foregroundStyle(Theme.Palette.text)
        .padding(Theme.Space.s + 4)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
        .brightness(hovered ? 0.04 : 0)
        .contentShape(Rectangle())
        #if !os(tvOS)
        .onHover { hovered = $0 }
        #endif
    }
}

/// A colored dot with its word, so state never rests on color alone.
struct StatusDot: View {
    enum Tone { case ok, warn, bad, idle }
    let tone: Tone
    let text: String

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Circle().fill(color).frame(width: 9, height: 9).alignmentGuide(.firstTextBaseline) { $0[.bottom] - 1 }
            Text(text)
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.text)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var color: Color {
        switch tone {
        case .ok: Theme.Palette.ok
        case .warn: Theme.Palette.warn
        case .bad: Theme.Palette.bad
        case .idle: Theme.Palette.muted
        }
    }
}
