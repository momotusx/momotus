import Network
import SwiftUI

/// First run, as the web does it: two doors (I use Stremio, Start fresh),
/// the official addons to add, and a setup check where each line has a dot,
/// a word and, when something is missing, the fix.
struct OnboardingScreen: View {
    let core: CoreClient
    @State private var addons: LiveRecord<AddonsSnapshot>
    @State private var pingMs: Int?
    @State private var pinged = false
    @State private var online = true
    @State private var busy: String?
    @State private var note: (ok: Bool, text: String)?
    @State private var focusInstall = 0
    @Environment(\.metrics) private var metrics

    /// Official Stremio addons for metadata and subtitles. Marquee ships no
    /// stream sources; the user adds those.
    private static let suggested = [
        (url: "https://v3-cinemeta.strem.io/manifest.json", name: "Cinemeta", what: "Catalogs and details"),
        (url: "https://opensubtitles-v3.strem.io/manifest.json", name: "OpenSubtitles v3", what: "Subtitles"),
    ]

    init(core: CoreClient) {
        self.core = core
        _addons = State(initialValue: LiveRecord("addons", core: core))
    }

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.l) {
                header
                if metrics.wide {
                    HStack(alignment: .top, spacing: Theme.Space.l) {
                        doors.frame(maxWidth: 420)
                        VStack(spacing: Theme.Space.m) {
                            addPanel
                            checkPanel
                        }
                    }
                } else {
                    doors
                    addPanel
                    checkPanel
                }
            }
            .frame(maxWidth: 1100, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .task(id: addons.taskId) { addons.start() }
        .task { await pingLoop() }
        .task { await watchNetwork() }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Marquee").font(.display(Theme.type.title2)).foregroundStyle(Theme.Palette.muted)
            Text("How would you like to begin?")
                .font(.display(metrics.wide ? 44 : Theme.type.title1 + 4))
                .foregroundStyle(Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            Text("No account needed. Everything stays on this device until you turn on sync.")
                .font(.text(Theme.type.body))
                .foregroundStyle(Theme.Palette.muted)
        }
    }

    private var doors: some View {
        VStack(alignment: .leading, spacing: 12) {
            NavigationLink {
                ImportScreen(core: core)
            } label: {
                Door(symbol: "door.left.hand.open", title: "I use Stremio", text: "Bring your addons, library and Continue Watching over with a code.")
            }
            .buttonStyle(.plain)
            Button { focusInstall += 1 } label: {
                Door(symbol: "sparkle", title: "Start fresh", text: "Browse catalogs right away. Add a source when you want to play.")
            }
            .buttonStyle(.plain)
            Text("A source is an addon that finds a playable stream for a title. Marquee does not include any.")
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.muted)
        }
    }

    private var addPanel: some View {
        Panel(title: "Add addons") {
            VStack(alignment: .leading, spacing: Theme.Space.s) {
                ForEach(Self.suggested, id: \.url) { addon in
                    let done = installed(addon.url)
                    Button { Task { await add(addon.url, addon.name) } } label: {
                        HStack(spacing: 12) {
                            Image(systemName: done ? "checkmark" : "plus").frame(width: 20)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(addon.name).font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                                Text(done ? "Installed, \(addon.what.lowercased())" : busy == addon.url ? "Installing" : addon.what)
                                    .font(.text(Theme.type.callout, relativeTo: .callout))
                                    .foregroundStyle(Theme.Palette.muted)
                            }
                            Spacer()
                        }
                        .padding(12)
                        .background(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous).fill(Theme.Palette.raised.opacity(done ? 0.4 : 0.8)))
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(Theme.Palette.text)
                    .disabled(done || busy != nil)
                }
                if let note {
                    StatusDot(tone: note.ok ? .ok : .bad, text: note.text)
                }
                InstallField(addons: addons, focusRequest: focusInstall)
            }
        }
    }

    private var checkPanel: some View {
        Panel(title: "Setup check") {
            VStack(alignment: .leading, spacing: 12) {
                ForEach(checks, id: \.label) { check in
                    HStack(alignment: .firstTextBaseline, spacing: 12) {
                        Text(check.label)
                            .font(.text(Theme.type.callout, weight: .semibold, relativeTo: .callout))
                            .foregroundStyle(Theme.Palette.muted)
                            .frame(width: 84, alignment: .leading)
                        StatusDot(tone: check.tone, text: check.text)
                        Spacer(minLength: 0)
                        if let fix = check.fix {
                            Button(fix.label, action: fix.run)
                                .buttonStyle(PillButtonStyle())
                        }
                    }
                }
            }
        }
    }

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
            Check(
                label: "Core", tone: pingMs != nil ? .ok : pinged ? .bad : .warn,
                text: pingMs.map { $0 < 1 ? "Connected, under 1 ms" : "Connected, \($0) ms" } ?? (pinged ? "Not answering" : "Connecting")),
            Check(label: "Network", tone: online ? .ok : .bad, text: online ? "Online" : "Offline"),
            Check(label: "Display", tone: .ok, text: DeviceCaps.displayWords),
            Check(
                label: "Catalogs", tone: catalogs.isEmpty ? .idle : .ok, text: catalogs.isEmpty ? "None yet" : catalogs.joined(separator: ", "),
                fix: catalogs.isEmpty ? ("Add Cinemeta", { Task { await add(Self.suggested[0].url, "Cinemeta") } }) : nil),
            Check(
                label: "Sources", tone: sources.isEmpty ? .idle : .ok,
                text: sources.isEmpty ? "None yet: titles cannot play" : sources.joined(separator: ", "),
                fix: sources.isEmpty ? ("Add a source", { focusInstall += 1 }) : nil),
            Check(
                label: "Subtitles", tone: subtitles.isEmpty ? .idle : .ok, text: subtitles.isEmpty ? "None yet" : subtitles.joined(separator: ", "),
                fix: subtitles.isEmpty ? ("Add OpenSubtitles", { Task { await add(Self.suggested[1].url, "OpenSubtitles v3") } }) : nil),
        ]
    }

    private func names(_ kind: String) -> [String] {
        (addons.snapshot?.addons ?? []).filter { $0.enabled && $0.provides.contains(kind) }.map(\.name)
    }

    private func installed(_ url: String) -> Bool {
        (addons.snapshot?.addons ?? []).contains { $0.url == url }
    }

    private func add(_ url: String, _ name: String) async {
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

/// A big choice: icon, title, one line, chevron.
private struct Door: View {
    let symbol: String
    let title: String
    let text: String
    @State private var hovered = false

    var body: some View {
        HStack(spacing: Theme.Space.s) {
            Image(systemName: symbol).font(.system(size: 26)).frame(width: 36)
            VStack(alignment: .leading, spacing: 4) {
                Text(title).font(.text(Theme.type.headline + 2, weight: .semibold, relativeTo: .headline))
                Text(text).font(.text(Theme.type.callout, relativeTo: .callout)).foregroundStyle(Theme.Palette.muted)
            }
            Spacer(minLength: 0)
            Image(systemName: "chevron.right").foregroundStyle(Theme.Palette.muted)
        }
        .foregroundStyle(Theme.Palette.text)
        .padding(Theme.Space.s + 4)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous).fill(Theme.Palette.surface.opacity(hovered ? 1 : 0.85)))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous).strokeBorder(Theme.Palette.line.opacity(0.7)))
        .contentShape(Rectangle())
        #if !os(tvOS)
        .onHover { hovered = $0 }
        #endif
    }
}

/// A titled card section.
private struct Panel<Content: View>: View {
    let title: String
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Text(title.uppercased())
                .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
                .tracking(1.6)
                .foregroundStyle(Theme.Palette.muted)
            content
        }
        .padding(Theme.Space.s + 4)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous).fill(Theme.Palette.surface))
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
