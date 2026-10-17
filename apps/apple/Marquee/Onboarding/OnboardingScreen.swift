import Network
import SwiftUI

/// First run as a full-screen flow, one decision per step: the two doors (I
/// use Stremio, Start fresh), then a source, then the setup check, where
/// every line has a dot, a word and, when something is missing, its fix.
/// Settings opens the check step alone as a page.
struct OnboardingScreen: View {
    enum Step { case doors, source, check }

    let core: CoreClient
    /// Ends the flow (Browse first, Done); nil when the check is a page.
    let close: (() -> Void)?
    @State private var step: Step
    @State private var addons: LiveRecord<AddonsSnapshot>
    @State private var pingMs: Int?
    @State private var pinged = false
    @State private var online = true
    @State private var busy: String?
    @State private var note: (ok: Bool, text: String)?
    @State private var showImport = false
    @Environment(\.metrics) private var metrics

    /// Official Stremio addons for metadata and subtitles, offered as fixes.
    /// Marquee ships no stream sources; the user adds those.
    private static let cinemeta = (url: "https://v3-cinemeta.strem.io/manifest.json", name: "Cinemeta")
    private static let openSubtitles = (url: "https://opensubtitles-v3.strem.io/manifest.json", name: "OpenSubtitles v3")

    init(core: CoreClient, start: Step = .doors, close: (() -> Void)? = nil) {
        self.core = core
        self.close = close
        _step = State(initialValue: start)
        _addons = State(initialValue: LiveRecord("addons", core: core))
    }

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.l) {
                switch step {
                case .doors: doorsStep
                case .source: sourceStep
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
        .navigationDestination(isPresented: $showImport) { ImportScreen(core: core) }
        .task(id: addons.taskId) { addons.start() }
        .task { await pingLoop() }
        .task { await watchNetwork() }
    }

    // MARK: steps

    private var doorsStep: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            HStack(alignment: .firstTextBaseline) {
                Text("Marquee").font(.display(Theme.type.title2)).foregroundStyle(Theme.Palette.muted)
                Spacer()
                if let close {
                    Button("Browse first", action: close)
                        .buttonStyle(.plain)
                        .font(.text(Theme.type.callout, weight: .semibold, relativeTo: .callout))
                        .foregroundStyle(Theme.Palette.text)
                }
            }
            Text("How would you like to begin?")
                .font(.display(metrics.wide ? 44 : Theme.type.title1 + 4))
                .foregroundStyle(Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            Text("No account needed. Everything stays on this device until you turn on sync.")
                .font(.text(Theme.type.body))
                .foregroundStyle(Theme.Palette.muted)
            let doors = Group {
                Button { showImport = true } label: {
                    Door(symbol: "door.left.hand.open", title: "I use Stremio", text: "Bring your addons, library and Continue Watching over with a code.")
                }
                Button { step = .source } label: {
                    Door(symbol: "sparkle", title: "Start fresh", text: "Browse catalogs right away. Add a source when you want to play.")
                }
            }
            .buttonStyle(.plain)
            if metrics.wide {
                HStack(alignment: .top, spacing: Theme.Space.s) { doors }
            } else {
                VStack(spacing: 12) { doors }
            }
            Text("A source is an addon that finds a playable stream for a title. Marquee does not include any.")
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.muted)
        }
    }

    private var sourceStep: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            stepHeading("Add a source", "A source is an addon that finds a playable stream for a title. Marquee does not include any. Paste a source addon's link, or skip this and add one later.")
            InstallField(addons: addons, focusRequest: 1)
            let sources = (addons.snapshot?.addons ?? []).filter(\.isSource)
            ForEach(sources) { addon in
                HStack(spacing: 10) {
                    Image(systemName: "checkmark.circle.fill").foregroundStyle(Theme.Palette.ok).accessibilityHidden(true)
                    Text(addon.name).font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                    Text("Installed").font(.text(Theme.type.callout, relativeTo: .callout)).foregroundStyle(Theme.Palette.muted)
                }
                .foregroundStyle(Theme.Palette.text)
                .accessibilityElement(children: .combine)
            }
            HStack {
                Button("Back") { step = .doors }.buttonStyle(PillButtonStyle())
                Spacer()
                Button(sources.isEmpty ? "Skip for now" : "Continue") { step = .check }
                    .buttonStyle(PillButtonStyle(prominent: !sources.isEmpty))
            }
            .padding(.top, Theme.Space.s)
        }
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
            .background(RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous).fill(Theme.Palette.surface))
            if let close {
                HStack {
                    Button("Back") { step = .source }.buttonStyle(PillButtonStyle())
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
                .font(.display(metrics.wide ? 44 : Theme.type.title1 + 4))
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
                fix: catalogs.isEmpty ? ("Add Cinemeta", { Task { await add(Self.cinemeta.url, Self.cinemeta.name) } }) : nil),
            Check(
                label: "Sources", tone: sources.isEmpty ? .idle : .ok,
                text: sources.isEmpty ? "None yet: titles cannot play" : sources.joined(separator: ", "),
                fix: sources.isEmpty ? ("Add a source", { step = .source }) : nil),
            Check(
                label: "Subtitles", tone: subtitles.isEmpty ? .idle : .ok, text: subtitles.isEmpty ? "None yet" : subtitles.joined(separator: ", "),
                fix: subtitles.isEmpty ? ("Add OpenSubtitles", { Task { await add(Self.openSubtitles.url, Self.openSubtitles.name) } }) : nil),
        ]
    }

    private func names(_ kind: String) -> [String] {
        (addons.snapshot?.addons ?? []).filter { $0.enabled && $0.provides.contains(kind) }.map(\.name)
    }

    private func add(_ url: String, _ name: String) async {
        guard busy == nil else { return }
        busy = url
        note = nil
        if await addons.perform("installAddon", ["url": url]) {
            note = (true, "Installed \(name)")
        } else {
            note = (false, "Could not install \(name): \(addons.failure ?? "unknown error")")
        }
        busy = nil
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

/// A big choice: icon, a title in the display face, one line, chevron.
private struct Door: View {
    let symbol: String
    let title: String
    let text: String
    @State private var hovered = false

    var body: some View {
        HStack(spacing: Theme.Space.s) {
            Image(systemName: symbol).font(.system(size: 26)).frame(width: 36)
            VStack(alignment: .leading, spacing: 4) {
                Text(title).font(.display(Theme.type.title3))
                Text(text)
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 0)
            Image(systemName: "chevron.right").foregroundStyle(Theme.Palette.muted)
        }
        .foregroundStyle(Theme.Palette.text)
        .padding(Theme.Space.s + 4)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous).fill(Theme.Palette.surface.opacity(hovered ? 1 : 0.85)))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous).strokeBorder(Theme.Palette.line.opacity(0.7)))
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
