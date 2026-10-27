import Network
import SwiftUI

/// First run as a full-screen flow, one decision per step: the doors (I use
/// Stremio, Start fresh, I already use Marquee), then the recommended addons
/// with a switch each (plans/m13-default-addons.md 6), the stream question,
/// the language subtitles start in (plans/m13-subtitle-languages.md 7), then the setup check,
/// where every line has a dot, a word and, when something is missing, its
/// fix. Start fresh, Browse first on a new profile and a finished import
/// lead to the addons; joining never does. Settings opens the check step
/// alone as a page.
struct OnboardingScreen: View {
    /// `signIn`: a phone, tablet or Mac with a hosted relay signs in first
    /// (plans/accounts.md 4.1); `tvSignIn`: a TV shows the phone's QR (4.2).
    enum Step { case signIn, tvSignIn, doors, defaults, question, subtitles, check }

    let core: CoreClient
    /// Ends the flow (Browse first, Done); nil when the check is a page.
    let close: (() -> Void)?
    @State private var step: Step
    @State private var addons: LiveRecord<AddonsSnapshot>
    /// The profile's preferences: a profile with subtitle languages passes that step.
    @State private var prefs: LiveRecord<Prefs>
    /// The subtitles step: every language the core knows, the one chosen
    /// (nil: No subtitles), and whether the rest show.
    @State private var languages: [LanguageChoice] = []
    @State private var subtitle: String? = nil
    @State private var moreLanguages = false
    /// The core's list is in (or failed): Continue waits for it, since the
    /// device's language is unknown before; a choice made before it stays.
    @State private var languagesLoaded = false
    @State private var subtitleTouched = false
    /// The recommended addons step's switches.
    @State private var defaults: DefaultsStep
    /// The defaults the check's fixes read, again whenever the list changes.
    @State private var fixer: DefaultsStep
    /// The addons step follows a Stremio import: defaults it brought say so.
    @State private var imported = false
    /// Browse first passed through the addons step: Continue goes Home.
    @State private var thenHome = false
    @State private var pingMs: Int?
    @State private var pinged = false
    @State private var online = true
    @State private var busy: String?
    @State private var note: (ok: Bool, text: String)?
    @State private var showImport = false
    @State private var showPairing = false
    /// The still the framed steps show (B4), asked once as first run opens.
    @State private var still: FirstRunStill?
    /// TV: the doors step enters on the first door, not on Browse first.
    @FocusState private var firstDoor: Bool
    /// TV: the addons step enters on Continue, so one press keeps them all.
    @FocusState private var onContinue: Bool
    /// The question enters on Start fast, the default.
    @FocusState private var onFast: Bool
    /// The subtitles step enters on Continue, so one press keeps the device's language.
    @FocusState private var onSubtitles: Bool
    /// The setup check opens on Done, its way on (the tvOS walk, ATV-F3).
    @FocusState private var onDone: Bool
    @Environment(\.metrics) private var metrics
    @Environment(SyncModel.self) private var sync

    init(core: CoreClient, start: Step = .doors, close: (() -> Void)? = nil) {
        self.core = core
        self.close = close
        #if DEBUG
        let walk = UserDefaults.standard.string(forKey: "MQOnboardingStep")
        #else
        let walk: String? = nil
        #endif
        // `-MQOnboardingStep subtitles` (or `question`) opens a first run on that step, for a walk.
        let opened: Step? = start != .doors ? nil
            : DefaultsRules.walkOpensDefaults(walk, debug: true) ? .defaults : walk == "subtitles" ? .subtitles : walk == "question" ? .question : nil
        _step = State(initialValue: opened ?? Self.first(start))
        _addons = State(initialValue: LiveRecord("addons", core: core))
        _prefs = State(initialValue: LiveRecord("settings", core: core))
        _defaults = State(initialValue: DefaultsStep(core: core, scope: .all))
        _fixer = State(initialValue: DefaultsStep(core: core, scope: .all))
    }

    /// With a hosted relay, first run begins at sign-in: a TV with the phone's QR.
    @MainActor private static func first(_ start: Step) -> Step {
        guard start == .doors, AccountServer.default != nil else { return start }
        #if os(tvOS)
        return .tvSignIn
        #else
        return .signIn
        #endif
    }

    var body: some View {
        Group {
            if step == .tvSignIn, let server = AccountServer.default {
                tvSignInStep(server)
            } else {
                steps
            }
        }
        .background(Theme.Palette.bg)
        .animation(Theme.Motion.panel, value: step)
        // Already in an account when first run opens: sign-in is done. (A join made
        // in it ends it there, Home or the doors.)
        .onAppear { if sync.joined, step == .signIn || step == .tvSignIn { step = .doors } }
    }

    /// A TV's first run (plans/tv-qr-login.md 5.5): the QR a phone's camera
    /// scans, "Scan with your phone's camera", and Browse first, the one focus
    /// stop, which leads to the doors (a recovery kit is reachable from there).
    /// A TV that joined goes Home, past the doors.
    private func tvSignInStep(_ server: String) -> some View {
        PairingScreen(core: core, done: close, autostart: server,
                      heading: AccountWords.shared.tvSignIn, lede: AccountWords.shared.tvScanHow,
                      more: [
                          PairingMore(title: AccountWords.shared.browseFirst, glyph: "chevron-right") {
                              sync.endPairing()
                              step = .doors
                          },
                      ])
    }

    /// The question steps are framed (B4) and fill the screen; the addons, sign-in and the check keep a column.
    private var steps: some View {
        Group {
            switch step {
            case .signIn: signInStep
            case .tvSignIn, .doors: doorsStep
            case .defaults: column { defaultsStep }
            case .question: questionStep
            case .subtitles: subtitlesStep
            case .check: column { checkStep }
            }
        }
        // The still is asked as first run opens, so the frame rarely waits for it.
        .task { still = await StillOnce.shared.get { [core] in try await core.call("firstRunStill", as: FirstRunStill?.self) } }
        .navigationDestination(isPresented: $showImport) {
            ImportScreen(core: core) {
                showImport = false
                imported = true
                openDefaults()
            }
        }
        // A join is the way in: Done goes Home, as the other doors end.
        .navigationDestination(isPresented: $showPairing) { PairingScreen(core: core, done: close) }
        .task(id: addons.taskId) { addons.start() }
        .task(id: prefs.taskId) { prefs.start() }
        .task(id: addons.snapshot) { if step == .check { await fixer.load() } }
        .task(id: step) {
            if step == .check { await fixer.load() }
            if step == .defaults, defaults.list == nil { await defaults.load() }
            if step == .subtitles, languages.isEmpty {
                let device: [String: any Sendable] = Self.device.map { ["device": $0] } ?? [:]
                do {
                    languages = try await core.call("languages", device, as: [LanguageChoice].self)
                } catch {
                    note = (false, "\(SubtitleStepRules.listFailed): \(error.localizedDescription)")
                }
                subtitle = SubtitleStepRules.loaded(subtitle, touched: subtitleTouched, all: languages)
                languagesLoaded = true
            }
        }
        .task { await pingLoop() }
        .task { await watchNetwork() }
    }

    private func column<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.l) { content() }
                .frame(maxWidth: DefaultsRules.column(wide: metrics.wide, tv: onTV) ?? .infinity, alignment: .leading)
                .frame(maxWidth: .infinity)
                .padding(.horizontal, metrics.gutter)
                .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .topScrim()
    }

    /// The step's header: its count, Back to the step before, and its skip.
    private func header(_ place: FilmRules.Place, skip: String? = nil, skipDisabled: Bool = false, onSkip: @escaping () -> Void = {}) -> StepHeader {
        StepHeader(count: FilmRules.count(place), back: back(place), skip: skip, skipDisabled: skipDisabled || busy != nil, onSkip: onSkip)
    }

    private func back(_ place: FilmRules.Place) -> (() -> Void)? {
        switch place {
        case .defaults: { step = .doors }
        case .question: { step = .defaults }
        case .subtitles: { step = .question }
        case .doors, .other: nil
        }
    }

    // MARK: steps

    /// "How should Marquee pick what to play?": Start fast (focused) or Best
    /// quality, each a patch of the stream preferences from the shared words.
    /// Skip writes nothing: the defaults are Start fast's.
    private var questionStep: some View {
        let q = StreamChoice.words.question
        return FramedStep(header: header(.question, skip: q.skip.label) { afterQuestion() }, still: still, question: q.title, lead: q.lead) {
            VStack(spacing: Theme.Space.s) {
                FilmChoices {
                    Button { Task { await answer(q.fast) } } label: { FilmChoice(symbol: "zap", title: q.fast.label, text: q.fast.text) }
                        .focused($onFast)
                    Button { Task { await answer(q.best) } } label: { FilmChoice(symbol: "gem", title: q.best.label, text: q.best.text) }
                }
                .buttonStyle(.filmChoice)
                .disabled(busy != nil)
                if let note, !note.ok {
                    StatusDot(tone: .bad, text: note.text)
                }
            }
        }
        .defaultFocus($onFast, true)
    }

    private func answer(_ answer: StreamChoiceWords.Question.Answer) async {
        guard busy == nil else { return }
        busy = "question"
        note = nil
        defer { busy = nil }
        do {
            try await core.perform("setPrefs", ["patch": StreamChoice.patch(answer)])
            afterQuestion()
        } catch {
            note = (false, "Could not save the choice: \(error.localizedDescription)")
        }
    }

    /// The subtitles step next, unless the profile has languages already.
    private func afterQuestion() {
        go(SubtitleStepRules.afterQuestion(prefs.snapshot?.subtitleLanguages, thenHome: thenHome))
    }

    /// Browse first goes Home; Start fresh and an import go to the setup check.
    private func afterSubtitles() {
        go(SubtitleStepRules.afterSubtitles(thenHome: thenHome))
    }

    private func go(_ next: SubtitleStepRules.Next) {
        switch next {
        case .subtitles: step = .subtitles
        case .home: if let close { close() } else { step = .check }
        case .check: step = .check
        }
    }

    /// This device's language, as the core's tags begin ("tr").
    private static var device: String? { DeviceCaps.language }

    /// The language subtitles start in, one choice: this device's chosen at
    /// first, then the common ones and No subtitles, the rest behind More
    /// languages. Continue writes it; Skip writes nothing.
    private var subtitlesStep: some View {
        let columns = Array(repeating: GridItem(.flexible(), spacing: Theme.Space.s, alignment: .top), count: metrics.wide ? 3 : 1)
        // Skip is off while Continue saves: the step leaves once.
        return FramedStep(header: header(.subtitles, skip: "Skip") { afterSubtitles() }, still: still,
                          question: SubtitleStepRules.title, lead: SubtitleStepRules.lead) {
            subtitleChoices(columns)
        }
        .defaultFocus($onSubtitles, true)
        // A disabled Continue cannot take the focus: TV gives it once the list is in.
        .onChange(of: languagesLoaded) { _, loaded in if loaded { onSubtitles = true } }
    }

    private func subtitleChoices(_ columns: [GridItem]) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            LazyVGrid(columns: columns, alignment: .leading, spacing: Theme.Space.xs) {
                let mine = SubtitleStepRules.listed(languages)
                ForEach(SubtitleStepRules.common(languages, device: mine), id: \.code) { language in
                    languageRow(language.code, language.name, line: language.code == mine ? SubtitleStepRules.device : nil)
                }
                languageRow(nil, SubtitleStepRules.none, line: SubtitleStepRules.noneLine)
            }
            if let note, !note.ok {
                StatusDot(tone: .bad, text: note.text)
            }
            HStack {
                Button(moreLanguages ? SubtitleStepRules.fewer : SubtitleStepRules.more) { moreLanguages.toggle() }
                    .buttonStyle(PillButtonStyle())
                Spacer()
                Button(busy == "subtitles" ? "Saving" : "Continue") { Task { await saveSubtitle() } }
                    .buttonStyle(PillButtonStyle(prominent: true))
                    .disabled(busy != nil || !languagesLoaded)
                    .focused($onSubtitles)
            }
            .padding(.top, Theme.Space.s)
            if moreLanguages {
                LazyVGrid(columns: columns, alignment: .leading, spacing: Theme.Space.xs) {
                    ForEach(SubtitleStepRules.rest(languages, device: SubtitleStepRules.listed(languages)), id: \.code) { language in
                        languageRow(language.code, language.name, line: nil)
                    }
                }
            }
        }
    }

    /// One language: its name, a line when it has one, a check when chosen.
    private func languageRow(_ code: String?, _ name: String, line: String?) -> some View {
        let on = subtitle == code
        let shape = RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous)
        return Button {
            subtitleTouched = true
            subtitle = code
        } label: {
            HStack(spacing: Theme.Space.s) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(name).textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                    if let line {
                        Text(line)
                            .textFont(Theme.type.callout, relativeTo: .callout)
                            .foregroundStyle(Theme.Palette.muted)
                    }
                }
                Spacer(minLength: 0)
                if on { Glyph("check") }
            }
            .multilineTextAlignment(.leading)
            .foregroundStyle(Theme.Palette.text)
            .padding(.horizontal, Theme.Space.s)
            .padding(.vertical, Theme.Space.xs + 2)
            .frame(maxWidth: .infinity, minHeight: 56, alignment: .leading)
            // A row is content: solid surface, no glass (design/DECISION.md, Surfaces).
            .background(shape.fill(Theme.Palette.surface))
            .overlay(shape.strokeBorder(Theme.Palette.text, lineWidth: on ? 2 : 0))
            .contentShape(Rectangle())
        }
        .buttonStyle(.row)
        .accessibilityAddTraits(on ? .isSelected : [])
    }

    private func saveSubtitle() async {
        guard busy == nil, languagesLoaded else { return }
        busy = "subtitles"
        note = nil
        defer { busy = nil }
        do {
            try await core.perform("setPrefs", ["patch": SubtitleStepRules.patch(subtitle)])
            afterSubtitles()
        } catch {
            note = (false, "Could not save the language: \(error.localizedDescription)")
        }
    }

    /// Sign in (4.1): Not now and Use my own sync server lead to the doors;
    /// an account that existed goes Home; a new one goes on to the doors.
    @ViewBuilder private var signInStep: some View {
        #if os(tvOS)
        doorsStep
        #else
        if let server = AccountServer.default {
            column {
                SignInScreen(server: server, sync: sync,
                             notNow: { step = .doors },
                             joined: { if let close { close() } else { step = .doors } },
                             created: { step = .doors })
            }
        } else {
            doorsStep
        }
        #endif
    }

    private var doorsStep: some View {
        let skip = close.map { close in { browseFirst(close) } }
        // The lead is said to someone without an account only: after Create it is not so.
        return FramedStep(header: header(.doors, skip: skip == nil ? nil : "Browse first") { skip?() }, still: still,
                          question: "How would you like to begin?",
                          lead: sync.joined ? nil : "No account needed. Everything stays on this device until you turn on sync.") {
            VStack(spacing: Theme.Space.s) {
                FilmChoices {
                    Button { showImport = true } label: {
                        FilmChoice(symbol: "door-open", title: "I use Stremio", text: "Bring your addons, library and Continue Watching over with a code.")
                    }
                    .focused($firstDoor)
                    Button { openDefaults() } label: {
                        FilmChoice(symbol: "sparkle", title: "Start fresh", text: "Pick from the recommended addons, then browse and play.")
                    }
                    if !sync.joined {
                        Button { showPairing = true } label: {
                            FilmChoice(symbol: "qr-code", title: "I already use Momotus", text: "Join your other device's sync account with a code. Library and addons follow.")
                        }
                    }
                }
                .buttonStyle(.filmChoice)
                if let note, !note.ok {
                    StatusDot(tone: .bad, text: note.text)
                }
            }
        }
        .defaultFocus($firstDoor, true)
    }

    private var defaultsStep: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            header(.defaults)
            stepHeading(DefaultsRules.title, DefaultsRules.lead)
            RecommendedAddons(model: defaults, imported: imported, hasCatalog: !names("catalog").isEmpty)
            #if !os(tvOS)
            // TV adds more from Settings: typing a link with a remote is no first step.
            VStack(alignment: .leading, spacing: Theme.Space.s) {
                Text("Add another addon")
                    .textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                    .foregroundStyle(Theme.Palette.text)
                    .accessibilityAddTraits(.isHeader)
                // Continue stays the one white action, also with a link typed.
                InstallField(core: core, primary: false)
                FindAddonsLink(core: core)
            }
            .padding(.top, Theme.Space.s)
            #endif
            HStack {
                Spacer()
                Button(defaults.busy ? "Adding" : "Continue") {
                    Task {
                        guard await defaults.install() else { return }
                        // A first run asks the one question next (stream choice 4.5).
                        step = .question
                    }
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
                            .textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout)
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
                        .focused($onDone)
                }
                .padding(.top, Theme.Space.s)
            }
        }
        .defaultFocus($onDone, true)
    }

    private func stepHeading(_ title: String, _ text: String) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title)
                .display(metrics.wide ? 44 : Theme.type.title1 + 4)
                .foregroundStyle(Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            Text(text)
                .textFont(Theme.type.body)
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
                fix: catalogs.isEmpty ? fix("cinemeta") : nil),
            Check(
                label: "Sources", tone: sources.isEmpty ? .idle : .ok,
                text: sources.isEmpty ? "None yet: titles cannot play" : sources.joined(separator: ", "),
                fix: sources.isEmpty ? fix("torrentio") : nil),
            Check(
                label: "Subtitles", tone: subtitles.isEmpty ? .idle : .ok, text: subtitles.isEmpty ? "None yet" : subtitles.joined(separator: ", "),
                fix: subtitles.isEmpty ? fix("opensubtitles") : nil),
        ]
    }

    private func names(_ kind: String) -> [String] {
        (addons.snapshot?.addons ?? []).filter { $0.enabled && $0.provides.contains(kind) }.map(\.name)
    }

    /// A missing line's fix: its recommended addon, added or turned on in
    /// one press, and nothing else.
    private func fix(_ key: String) -> (label: String, run: () -> Void)? {
        guard let addon = DefaultsRules.fix(key, in: fixer.list) else { return nil }
        let words = DefaultsRules.fixWords(addon)
        return (words, { Task { await turnOn(addon, words) } })
    }

    private func turnOn(_ addon: DefaultAddonInfo, _ words: String) async {
        guard busy == nil else { return }
        busy = addon.key
        note = nil
        defer { busy = nil }
        let added = addon.installed == .absent
        note = await fixer.set(addon.key, on: true)
            ? (true, added ? "Added \(addon.name)" : "Turned \(addon.name) on")
            : (false, "\(words) did not work: \(fixer.failure ?? "unknown error")")
    }

    /// The addons step with every switch on.
    private func openDefaults(thenHome: Bool = false) {
        self.thenHome = thenHome
        defaults = DefaultsStep(core: core, scope: .all)
        step = .defaults
    }

    /// Browse first: a new profile still passes the switches (nothing is
    /// installed unasked) and then goes Home; one with a list goes Home.
    private func browseFirst(_ close: () -> Void) {
        if addons.snapshot?.fresh == true { openDefaults(thenHome: true) } else { close() }
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

/// The recommended addons grouped, each with its line. In the step a
/// missing one has a switch, on until turned off, and one the profile has
/// says so; in Settings every one is a switch that acts at once.
struct RecommendedAddons: View {
    let model: DefaultsStep
    var imported = false
    /// An addon on already brings a catalog: turning the recommended ones off warns nothing.
    var hasCatalog = false
    /// Settings: each switch adds or switches its addon at once.
    var settings = false

    var body: some View {
        // More air between groups than between rows, so each group reads as one.
        VStack(alignment: .leading, spacing: Theme.Space.m + Theme.Space.xs) {
            if model.list == nil {
                if let failure = model.failure {
                    StateLine(symbol: "triangle-alert", text: failure, action: ("Try again", { Task { await model.load() } }))
                } else {
                    Label("Reading the list", glyph: "clock")
                        .textFont(Theme.type.body)
                        .foregroundStyle(Theme.Palette.muted)
                }
            }
            ForEach(DefaultsRules.groups, id: \.id) { group in
                let rows = model.rows.filter { $0.group == group.id }
                if !rows.isEmpty {
                    VStack(alignment: .leading, spacing: Theme.Space.xs) {
                        // Over the marks: the rows' own inset.
                        Text(caps: group.title)
                            .textFont(Theme.type.caption, weight: .semibold, relativeTo: .caption)
                            .tracking(1.6)
                            .foregroundStyle(Theme.Palette.muted)
                            .accessibilityAddTraits(.isHeader)
                            .padding(.leading, Theme.Space.s)
                            .padding(.bottom, 2)
                        ForEach(rows) { row($0) }
                        // Under the catalogs, where their switches are.
                        if group.id == "catalogs", !settings,
                           DefaultsRules.warns(model.list?.addons ?? [], moved: model.moved, hasCatalog: hasCatalog) {
                            StatusDot(tone: .warn, text: DefaultsRules.catalogWarning)
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
        if settings {
            switchRow(addon, on: addon.installed == .on) { Task { await model.set(addon.key, on: addon.installed != .on) } }
        } else if DefaultsRules.hasSwitch(addon) {
            switchRow(addon, on: model.isOn(addon)) { model.flip(addon) }
        } else {
            DefaultRow(addon: addon) {
                Text(DefaultsRules.presentWord(addon, imported: imported))
                    .textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.muted)
            }
            .accessibilityElement(children: .combine)
        }
    }

    private func switchRow(_ addon: DefaultAddonInfo, on: Bool, flip: @escaping () -> Void) -> some View {
        Button(action: flip) {
            DefaultRow(addon: addon) { SwitchMark(on: on) }
        }
        .buttonStyle(.row)
        // On the button itself: a new element around it would not press it (VoiceOver, Full Keyboard Access).
        .accessibilityLabel("\(addon.name). \(addon.line)")
        .accessibilityValue(on ? "On" : "Off")
        .accessibilityAddTraits(.isToggle)
    }
}

/// One default: its mark, name and line, with a switch or a word at the end.
private struct DefaultRow<End: View>: View {
    let addon: DefaultAddonInfo
    @ViewBuilder let end: End

    var body: some View {
        HStack(alignment: .center, spacing: Theme.Space.s) {
            DefaultMark(addon: addon)
            VStack(alignment: .leading, spacing: 4) {
                Text(addon.name).textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                Text(addon.line)
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 0)
            end
        }
        .multilineTextAlignment(.leading)
        .foregroundStyle(Theme.Palette.text)
        // The mark sets the height: a little air above and below it.
        .padding(.horizontal, Theme.Space.s)
        .padding(.vertical, Theme.Space.xs + 2)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
    }
}

/// A 56 pt tile leading each row: the addon's bundled logo, else its
/// initial on the neutral tile, also while it loads and when it fails (an
/// SVG logo is one: the image decoder reads bitmaps only).
private struct DefaultMark: View {
    let addon: DefaultAddonInfo

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous)
        ArtImage(url: addon.logo, mode: .fit, maxPixels: 168, fallback: DefaultsRules.initial(addon.name))
            .padding(addon.logo == nil ? 0 : 6)
            .frame(width: 56, height: 56)
            .background(shape.fill(Theme.Palette.surface))
            .clipShape(shape)
            .overlay(shape.strokeBorder(Theme.Palette.edge, lineWidth: 1))
            .accessibilityHidden(true)
    }
}

/// A page of recommended addons: the Home card's Review (the missing ones,
/// switched on; Continue installs them and goes back) or Settings (all
/// nine, each switch acting at once).
struct RecommendedAddonsPage: View {
    @State private var model: DefaultsStep
    private let hasCatalog: Bool
    private var settings: Bool { model.scope == .all }
    @FocusState private var onContinue: Bool
    @Environment(\.metrics) private var metrics
    @Environment(\.dismiss) private var dismiss

    init(core: CoreClient, scope: DefaultsRules.Scope, hasCatalog: Bool = false) {
        _model = State(initialValue: DefaultsStep(core: core, scope: scope))
        self.hasCatalog = hasCatalog
    }

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: DefaultsRules.title)
                Text(settings ? DefaultsRules.settingsLead : DefaultsRules.reviewLead)
                    .textFont(Theme.type.body)
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
                RecommendedAddons(model: model, hasCatalog: hasCatalog, settings: settings)
                if !settings {
                    Button(model.busy ? "Adding" : "Continue") {
                        Task { if await model.install() { dismiss() } }
                    }
                    .buttonStyle(PillButtonStyle(prominent: true))
                    .disabled(model.busy || model.list == nil)
                    .focused($onContinue)
                    .padding(.top, Theme.Space.s)
                }
            }
            .frame(maxWidth: DefaultsRules.column(wide: metrics.wide, tv: onTV) ?? .infinity, alignment: .leading)
            .frame(maxWidth: .infinity)
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

/// A colored dot with its word, so state never rests on color alone.
struct StatusDot: View {
    enum Tone { case ok, warn, bad, idle }
    let tone: Tone
    let text: String

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Circle().fill(color).frame(width: 9, height: 9).alignmentGuide(.firstTextBaseline) { $0[.bottom] - 1 }
            Text(text)
                .textFont(Theme.type.callout, relativeTo: .callout)
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

/// The tvOS build: the steps take the TV column.
#if os(tvOS)
private let onTV = true
#else
private let onTV = false
#endif
