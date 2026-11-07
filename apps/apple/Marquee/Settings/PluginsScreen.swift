import SwiftUI

/// Settings > Sources > Plugins (plans/plugins-and-marketplace.md 7.7, R7): the profile's Nuvio
/// repositories, a switch for each and for each provider, Remove (asked first), and a link field whose
/// repository is read and shown on the install sheet; nothing is added before the sheet's Add.
struct PluginsScreen: View {
    let core: CoreClient
    @State private var answer: PluginsAnswer?
    @State private var failure: String?
    @State private var said: (tone: StatusDot.Tone, text: String)?
    @State private var link = ""
    @State private var busy = false
    @State private var removing: PluginRepoInfo?
    @State private var gate = InstallGate()
    @Environment(\.metrics) private var metrics

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: PluginWords.title)
                Text(PluginWords.lede)
                    .textFont(Theme.type.body)
                    .foregroundStyle(Theme.Palette.muted)
                if let failure, answer == nil {
                    StateLine(symbol: "triangle-alert", text: failure, action: ("Try again", { Task { await reload() } }))
                } else if let answer {
                    if answer.available {
                        field
                        if answer.repos.isEmpty {
                            StateLine(symbol: "circle-slash", text: PluginWords.empty)
                        }
                        ForEach(answer.repos) { repo in
                            RepoCard(repo: repo,
                                     toggle: { act("setPluginRepoEnabled", ["url": repo.url, "enabled": !repo.enabled]) },
                                     provider: { p in act("setPluginEnabled", ["url": repo.url, "plugin": p.id, "enabled": !p.enabled]) },
                                     remove: { removing = repo })
                        }
                    } else {
                        StateLine(symbol: "circle-slash", text: PluginWords.notAvailable)
                    }
                } else {
                    StatusDot(tone: .idle, text: PluginWords.loading)
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
        .task { await reload() }
        .installSheet(gate)
        .confirmationDialog(
            "Remove \(removing?.shownName ?? "this repository")?",
            isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }),
            titleVisibility: .visible
        ) {
            Button(PluginWords.remove, role: .destructive) {
                if let repo = removing { act("removePluginRepo", ["url": repo.url]) }
            }
        } message: {
            Text("Its providers stop finding links. You can add it again from its link.")
        }
    }

    private var field: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(caps: PluginWords.fieldLabel)
                .textFont(Theme.type.caption, weight: .semibold, relativeTo: .caption)
                .tracking(1.6)
                .foregroundStyle(Theme.Palette.muted)
            HStack(spacing: 12) {
                TextField("https://example.org/manifest.json", text: $link)
                    .textFieldStyle(.plain)
                    .textFont(Theme.type.body)
                    .autocorrectionDisabled()
                    #if os(iOS)
                    .textInputAutocapitalization(.never)
                    .keyboardType(.URL)
                    #endif
                    .onSubmit(check)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
                    .fieldChrome(focused: false)
                    .accessibilityLabel(PluginWords.fieldLabel)
                Button(action: check) { Label(PluginWords.add, glyph: "plus") }
                    .buttonStyle(PillButtonStyle(prominent: !link.trimmingCharacters(in: .whitespaces).isEmpty))
                    .disabled(link.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            if let said {
                StatusDot(tone: said.tone, text: said.text)
            }
        }
    }

    private func reload() async {
        do {
            answer = try await core.call("plugins", as: PluginsAnswer.self)
            failure = nil
        } catch {
            failure = ErrorWords.of(error)
        }
    }

    /// Reads the link and asks on the sheet; only its Add adds the repository.
    private func check() {
        let url = link.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !url.isEmpty, !busy else { return }
        busy = true
        said = (.idle, PluginWords.checking)
        Task {
            defer { busy = false }
            do {
                let p = try await core.call("previewPluginRepo", ["url": url], as: PluginRepoPreview.self)
                said = nil
                gate.ask(InstallAsk(
                    title: PluginWords.fill(PluginWords.sheetTitle, ["name": p.name]),
                    lines: [PluginWords.adds(p.providers), PluginWords.sheetNote],
                    run: { add(p) },
                    action: PluginWords.add,
                    details: PluginWords.fill(PluginWords.sheetFrom, ["host": p.host])))
            } catch {
                said = (.bad, PluginWords.fill(PluginWords.couldNotAdd, ["reason": ErrorWords.of(error)]))
            }
        }
    }

    private func add(_ p: PluginRepoPreview) {
        said = (.idle, PluginWords.adding)
        Task {
            do {
                answer = try await core.call("addPluginRepo", ["url": p.url], as: PluginsAnswer.self)
                said = (.ok, PluginWords.fill(PluginWords.added, ["name": p.name]))
                link = ""
            } catch {
                said = (.bad, PluginWords.fill(PluginWords.couldNotAdd, ["reason": ErrorWords.of(error)]))
            }
        }
    }

    /// A switch or a removal, then the list as the core has it now.
    private func act(_ op: String, _ fields: [String: any Sendable]) {
        Task {
            do {
                _ = try await core.call(op, fields, as: EmptyAnswer.self)
            } catch {
                said = (.bad, ErrorWords.of(error))
            }
            await reload()
        }
    }
}

/// Find addons > Nuvio plugins (R8): the repositories nuvioplugins.com lists, in its order, each with
/// its provider count, kinds and languages, searched on the device (not on TV); Add reads one and asks
/// on the install sheet first, and one the profile has says Installed (removed in Settings > Plugins).
struct PluginDirectoryScreen: View {
    let core: CoreClient
    @State private var listed: PluginDirectory?
    @State private var query = ""
    @State private var said: (url: String, tone: StatusDot.Tone, text: String)?
    @State private var gate = InstallGate()
    @Environment(\.metrics) private var metrics

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: PluginWords.directoryTitle)
                Text(PluginWords.fill(PluginWords.directoryFrom, ["source": listed?.source ?? "nuvioplugins.com"]))
                    .textFont(Theme.type.body)
                    .foregroundStyle(Theme.Palette.muted)
                if let listed {
                    if listed.repos.isEmpty {
                        StateLine(symbol: "circle-slash", text: PluginWords.notAvailable)
                    } else {
                        #if !os(tvOS)
                        TextField(PluginWords.directorySearch, text: $query)
                            .textFieldStyle(.plain)
                            .textFont(Theme.type.body)
                            .autocorrectionDisabled()
                            .padding(.horizontal, 16)
                            .padding(.vertical, 12)
                            .fieldChrome(focused: false)
                        #endif
                        let shown = listed.repos.filter { PluginWords.matches($0, query) }
                        if shown.isEmpty {
                            StateLine(symbol: "circle-slash", text: PluginWords.directoryNoMatch)
                        }
                        ForEach(shown) { repo in card(repo, source: listed.source) }
                    }
                } else {
                    StatusDot(tone: .idle, text: PluginWords.loading)
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
        .task { await reload() }
        .installSheet(gate)
    }

    private func card(_ r: ListedRepo, source: String) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            HStack(alignment: .firstTextBaseline) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(r.name).textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                    Text(PluginWords.fill(PluginWords.directoryFrom, ["source": source]))
                        .textFont(Theme.type.caption).foregroundStyle(Theme.Palette.muted)
                }
                Spacer(minLength: 12)
                if r.installed {
                    Label(PluginWords.installed, glyph: "check").textFont(Theme.type.callout, weight: .semibold)
                } else {
                    Button(PluginWords.add, glyph: "plus") { ask(r) }
                        .buttonStyle(PillButtonStyle())
                        .accessibilityLabel("\(PluginWords.add) \(r.name)")
                }
            }
            Text(PluginWords.directoryLine(r)).textFont(Theme.type.callout, weight: .medium)
            if !r.description.isEmpty {
                Text(r.description)
                    .textFont(Theme.type.callout)
                    .foregroundStyle(Theme.Palette.muted)
                    .lineLimit(3)
            }
            if let said, said.url == r.url {
                StatusDot(tone: said.tone, text: said.text)
            }
        }
        .padding(Theme.Space.m)
        .background(Theme.Palette.surface, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
    }

    private func reload() async {
        listed = try? await core.call("pluginDirectory", as: PluginDirectory.self)
    }

    private func ask(_ r: ListedRepo) {
        said = (r.url, .idle, PluginWords.checking)
        Task {
            do {
                let p = try await core.call("previewPluginRepo", ["url": r.url], as: PluginRepoPreview.self)
                said = nil
                gate.ask(InstallAsk(
                    title: PluginWords.fill(PluginWords.sheetTitle, ["name": p.name]),
                    lines: [PluginWords.adds(p.providers), PluginWords.sheetNote],
                    run: { add(p) },
                    action: PluginWords.add,
                    details: PluginWords.fill(PluginWords.sheetFrom, ["host": p.host])))
            } catch {
                said = (r.url, .bad, PluginWords.fill(PluginWords.couldNotAdd, ["reason": ErrorWords.of(error)]))
            }
        }
    }

    private func add(_ p: PluginRepoPreview) {
        Task {
            do {
                _ = try await core.call("addPluginRepo", ["url": p.url], as: PluginsAnswer.self)
                said = (p.url, .ok, PluginWords.fill(PluginWords.added, ["name": p.name]))
            } catch {
                said = (p.url, .bad, PluginWords.fill(PluginWords.couldNotAdd, ["reason": ErrorWords.of(error)]))
            }
            await reload()
        }
    }
}

/// Any answer, read as nothing.
private struct EmptyAnswer: Decodable, Sendable {
    init(from decoder: any Decoder) throws {}
}

/// One repository: its name, host and description with its switch, then each provider with its own.
private struct RepoCard: View {
    let repo: PluginRepoInfo
    let toggle: () -> Void
    let provider: (PluginInfo) -> Void
    let remove: () -> Void
    @State private var hostShown = false

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            HStack(alignment: .firstTextBaseline) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(repo.shownName).textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                    // The repository by its own name; its host only behind Details.
                    Button(InstallSheetRules.details) { hostShown.toggle() }
                        .buttonStyle(PillButtonStyle())
                        .accessibilityLabel("\(repo.shownName) \(InstallSheetRules.details.lowercased())")
                        .accessibilityAddTraits(hostShown ? .isSelected : [])
                    if hostShown { Text(repo.host).textFont(Theme.type.caption).foregroundStyle(Theme.Palette.muted) }
                }
                Spacer(minLength: 12)
                switchButton(on: repo.enabled, name: repo.shownName, action: toggle)
                Button(PluginWords.remove, glyph: "trash", role: .destructive, action: remove)
                    .buttonStyle(PillButtonStyle())
                    .accessibilityLabel("\(PluginWords.remove) \(repo.shownName)")
            }
            if !repo.description.isEmpty {
                Text(repo.description)
                    .textFont(Theme.type.callout)
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let problem = repo.problem {
                StatusDot(tone: .bad, text: PluginWords.fill(PluginWords.unread, ["problem": problem]))
            }
            ForEach(repo.providers) { p in
                HStack {
                    Text(p.languages.isEmpty ? p.name : "\(p.name)  \(p.languages.joined(separator: ", "))")
                        .textFont(Theme.type.callout)
                        .foregroundStyle(p.enabled && repo.enabled ? Theme.Palette.text : Theme.Palette.muted)
                    Spacer(minLength: 12)
                    switchButton(on: p.enabled, name: p.name) { provider(p) }
                }
            }
        }
        .padding(Theme.Space.m)
        .background(Theme.Palette.surface, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
    }

    /// On or Off as a pill, which every form presses (a TV has no switch to flip).
    private func switchButton(on: Bool, name: String, action: @escaping () -> Void) -> some View {
        Button(on ? PluginWords.on : PluginWords.off, glyph: "power", action: action)
            .buttonStyle(PillButtonStyle(prominent: false))
            .accessibilityLabel(name)
            .accessibilityValue(on ? PluginWords.on : PluginWords.off)
    }
}
