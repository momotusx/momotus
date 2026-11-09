import Network
import SwiftUI

/// First run as Poster Wall (plans/first-screens.md, design/exploration/first-screens/round2.html combo A): Welcome
/// over a drifting wall of the default catalog's posters, then sheets over the same wall, held still: Login, the profile
/// form (only after sign-in or Not now), one-page setup, then Home. The order is `FirstRunRules`, held equal to the web's
/// and Android's by apps/shared/fixtures/first-run-vectors.json. Settings opens the setup check alone as a page.
struct OnboardingScreen: View {
    /// `tvSignIn`: a TV shows the phone's QR (plans/tv-qr-login.md 5.5); `check` is Settings' page.
    enum Step: String, Equatable { case welcome, signIn, tvSignIn, profile, setup, check }

    let core: CoreClient
    /// Ends the flow: Home opens as the root.
    let close: (() -> Void)?
    private let start: Step?

    init(core: CoreClient, start: Step? = nil, close: (() -> Void)? = nil) {
        self.core = core
        self.start = start
        self.close = close
    }

    var body: some View {
        if start == .check {
            SetupCheckPage(core: core)
        } else {
            FirstRun(core: core, close: close)
        }
    }
}

/// The flow itself.
private struct FirstRun: View {
    typealias Step = OnboardingScreen.Step

    let core: CoreClient
    let close: (() -> Void)?
    /// Nil until the profiles and the account are known: the wall alone meanwhile.
    @State private var step: Step?
    /// Login came from "I already use Momotus".
    @State private var returning = false
    /// The account was made in this first run: the profile form's line, and Back leaves the app from it.
    @State private var made = false
    /// After a join: "Getting your profiles" while the first pull runs.
    @State private var pulling = false
    /// Start watching with Bring my Stremio library: the import's sheet.
    @State private var importing = false
    // The profile form.
    @State private var typed = ""
    @State private var pressed: String?
    // The setup sheet.
    @State private var bring = false
    @State private var best = false
    /// A chip pressed, or a language picked under More.
    @State private var chip: String?
    @State private var picked: String?
    @State private var languages: [LanguageChoice] = []
    @State private var prefs: LiveRecord<Prefs>
    @State private var prefsSaved = false
    @State private var showLanguages = false
    @State private var showReview = false
    @State private var busy = false
    @State private var failure: String?
    /// Pause motion's choice, kept on this device.
    @State private var paused = UserDefaults.standard.bool(forKey: WallMotion.pausedKey)
    @FocusState private var onStart: Bool
    @FocusState private var onContinue: Bool
    @FocusState private var onNotNow: Bool
    @Environment(\.metrics) private var metrics
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.accessibilityReduceMotion) private var systemReduceMotion
    @Environment(SyncModel.self) private var sync
    @Environment(ProfileSession.self) private var profiles

    private var words: FirstRunWords { .shared }

    init(core: CoreClient, close: (() -> Void)?) {
        self.core = core
        self.close = close
        _prefs = State(initialValue: LiveRecord("settings", core: core))
    }

    var body: some View {
        ZStack {
            PosterWall(posters: WallOnce.shared.posters, columns: FirstRunRules.columns(form),
                       running: WallMotion.runs(step: shown, reduceMotion: reduceMotion, scenePhase: scenePhase, paused: paused),
                       dimmed: step != .welcome, wide: metrics.wide)
            Group {
                if let step { screen(step) }
            }
            .transition(.opacity)
        }
        // The step change honours Reduce Motion (closes OnboardingScreen's old :100).
        .animation(reduceMotion ? nil : Theme.Motion.panel, value: step)
        .animation(reduceMotion ? nil : Theme.Motion.panel, value: importing)
        #if !os(iOS)
        // Escape on the Mac, Menu on the TV: Back per the plan's table (5); nil leaves it to the system.
        .onExitCommand(perform: backAction)
        #endif
        .task { if step == nil { go(FirstRunRules.next(entry(.start))) } }
        // The wall is asked as first run opens and again at each step while none came (WallOnce).
        .task(id: step) { await WallOnce.shared.get { [core] in try await core.call("firstRunWall", as: [String].self) } }
        .task(id: prefs.taskId) { prefs.start() }
        .task(id: step == .setup) { if step == .setup, languages.isEmpty { await loadLanguages() } }
        .navigationDestination(isPresented: $showLanguages) {
            LanguagePicker(title: words.subtitlesTitle, audio: false, core: core, isOn: { $0 == selectedChip }, toggle: { code in
                picked = code
                chip = code
                showLanguages = false
            })
        }
        .navigationDestination(isPresented: $showReview) { RecommendedAddonsPage(core: core, scope: .all) }
    }

    private var form: FirstRunRules.Form { FirstRunRules.form(wide: metrics.wide, tv: tvForm) }
    private var reduceMotion: Bool { systemReduceMotion || WallFlags.reduceMotion }

    /// The step as the rules name it: what the wall and Back read.
    private var shown: FirstRunStep {
        switch step {
        case .welcome?, nil: .welcome
        case .signIn?, .tvSignIn?: .login
        case .profile?: .profile
        case .setup?: importing ? .importing : .setup
        case .check?: .none
        }
    }

    @ViewBuilder private func screen(_ step: Step) -> some View {
        switch step {
        case .welcome: welcome
        case .signIn: signIn
        case .tvSignIn: tvSignIn
        case .profile: profileSheet
        case .setup: importing ? AnyView(importSheet) : AnyView(setupSheet)
        case .check: EmptyView()
        }
    }

    // MARK: rules

    private var owner: ProfileInfo? { profiles.snapshot?.profiles.first(where: \.isOwner) }

    private func entry(_ after: FirstRunRules.After) -> FirstRunRules.Entry {
        let snapshot = profiles.snapshot
        // The cover opens only while the profile's addons are fresh.
        return FirstRunRules.Entry(
            fresh: true, kidsSession: snapshot?.current?.kids == true, server: AccountServer.default != nil, inAccount: sync.joined,
            profileStepDone: snapshot?.stepDone ?? true, profiles: snapshot?.profiles.count ?? 1,
            pin: snapshot?.profiles.contains(where: \.hasPin) == true, locked: snapshot?.locked == true, after: after)
    }

    private func go(_ next: FirstRunStep) {
        failure = nil
        switch next {
        case .welcome: step = .welcome
        case .login:
            #if os(tvOS)
            step = .tvSignIn
            #else
            step = .signIn
            #endif
        case .profile: step = .profile
        case .setup:
            importing = false
            step = .setup
        case .importing: importing = true
        case .who, .home, .none: close?()
        }
    }

    private var backAction: (() -> Void)? {
        guard let target = FirstRunRules.back(from: shown, server: AccountServer.default != nil, made: made || sync.joined) else { return nil }
        return { go(target) }
    }

    /// A join pulled an account: "Getting your profiles" while the first round runs (15 s at most), then Who's
    /// watching? (two or more profiles, or a PIN) or Home (D2).
    private func afterJoin(provider: String?) async {
        pulling = true
        let deadline = ContinuousClock.now + FirstRunRules.pullWait
        while ContinuousClock.now < deadline, sync.lastReport == nil, sync.problem == nil, !Task.isCancelled {
            try? await Task.sleep(for: .milliseconds(250))
        }
        pulling = false
        if FirstRunRules.next(entry(.joined)) == .who {
            profiles.askAfterJoin(line: FirstRunRules.whoLine(provider: provider, device: sync.joinedBy))
        }
        close?()
    }

    // MARK: Welcome

    private var welcome: some View {
        let wide = metrics.wide
        return VStack(alignment: wide ? .center : .leading, spacing: 0) {
            // The lockup: the bird 52 (1.45 of 36), the name beside it.
            Wordmark(size: 36)
            Text(words.welcomeTitle)
                .textFont(wide ? (tvForm ? 64 : 60) : 36, weight: .bold, relativeTo: .largeTitle)
                .foregroundStyle(.white)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityAddTraits(.isHeader)
                .padding(.top, 22)
            Text(words.welcomeLead)
                .textFont(wide ? 20 : 17, relativeTo: .body)
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: wide ? 620 : nil)
                .padding(.top, 12)
            welcomeActions.padding(.top, 26)
        }
        .multilineTextAlignment(wide ? .center : .leading)
        .frame(maxWidth: wide ? 760 : .infinity, alignment: wide ? .center : .leading)
        .padding(.horizontal, 24)
        .padding(.bottom, wide ? 84 : 40)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
        .overlay(alignment: .topTrailing) { pauseToggle }
        .defaultFocus($onStart, true)
    }

    private var welcomeActions: some View {
        let wide = metrics.wide
        let acts = wide ? AnyLayout(HStackLayout(spacing: 12)) : AnyLayout(VStackLayout(spacing: 12))
        return acts {
            Button { go(FirstRunRules.next(entry(.getStarted))) } label: { actionLabel(words.getStarted) }
                .buttonStyle(PillButtonStyle(prominent: true))
                .focused($onStart)
            // No sync server: no Login, and nothing to already use.
            if AccountServer.default != nil {
                Button {
                    returning = true
                    go(FirstRunRules.next(entry(.alreadyUse)))
                } label: { actionLabel(words.alreadyUse) }
                    .buttonStyle(PillButtonStyle())
            }
        }
    }

    /// A Welcome or sheet action: 54 high, the column's width on the phone, at least 220 wide elsewhere.
    private func actionLabel(_ title: String) -> some View {
        Text(title)
            .frame(minWidth: metrics.wide ? 220 - 2 * PillButtonStyle.horizontal : nil, maxWidth: metrics.wide ? nil : .infinity,
                   minHeight: FirstRunRules.Size.primary - 2 * PillButtonStyle.vertical)
    }

    /// Pause motion / Play motion (WCAG 2.2.2, D4): Welcome only, top right inside the safe area.
    private var pauseToggle: some View {
        Button {
            paused.toggle()
            UserDefaults.standard.set(paused, forKey: WallMotion.pausedKey)
        } label: {
            Glyph(paused ? "play" : "pause", size: 20)
                .foregroundStyle(.white)
                .frame(width: FirstRunRules.Size.toggle, height: FirstRunRules.Size.toggle)
                .background(Circle().fill(.black.opacity(0.45)))
                .contentShape(Circle())
        }
        .buttonStyle(FocusPlainStyle(shape: Circle()))
        .accessibilityLabel(WallMotion.toggleLabel(paused: paused))
        .accessibilityAddTraits(.isToggle)
        .padding(.top, 8)
        .padding(.trailing, 16)
    }

    // MARK: Login

    @ViewBuilder private var signIn: some View {
        #if os(tvOS)
        tvSignIn
        #else
        if pulling {
            WallSheet(sheet: .login) { gettingProfiles }
        } else if let server = AccountServer.default {
            WallSheet(sheet: .login, back: { go(.welcome) }) {
                SignInScreen(server: server, sync: sync, lead: returning ? words.loginReturning : words.loginLine, returning: returning,
                             notNow: { go(FirstRunRules.next(entry(.notNow))) },
                             joined: { provider in Task { await afterJoin(provider: provider) } },
                             created: {
                                 made = true
                                 go(FirstRunRules.next(entry(.created)))
                             })
            }
        }
        #endif
    }

    private var gettingProfiles: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            SheetHead(title: words.gettingProfiles)
            ProgressView().controlSize(.large).tint(Theme.Palette.text)
        }
    }

    /// A TV's Login (plans/tv-qr-login.md 5.5): the phone's QR over the wall; Not now, its one focus stop, goes on to
    /// the profile form. A TV that joined follows the join's rule.
    @ViewBuilder private var tvSignIn: some View {
        if pulling {
            WallSheet(sheet: .login) { gettingProfiles }
        } else if let server = AccountServer.default {
            PairingScreen(core: core, done: { Task { await afterJoin(provider: nil) } }, autostart: server,
                          heading: AccountWords.shared.tvSignIn, lede: AccountWords.shared.tvScanHow,
                          more: [
                              PairingMore(title: AccountWords.shared.notNow, glyph: "chevron-right") {
                                  sync.endPairing()
                                  go(FirstRunRules.next(entry(.notNow)))
                              },
                          ],
                          doneOnJoin: true)
        }
    }

    // MARK: Make your first profile

    private var profileSheet: some View {
        let stored = owner?.avatar ?? ""
        let opening = ProfileRules.parseAvatar(stored)
        return WallSheet(sheet: .profile) {
            VStack(alignment: .leading, spacing: 0) {
                SheetHead(title: words.profileTitle, line: FirstRunRules.profileLine(made: made || sync.joined))
                HStack(spacing: 16) {
                    ProfileAvatar(ProfileRules.Avatar(glyph: opening.glyph, color: pressed ?? opening.color),
                                  name: typed.isEmpty ? (owner?.name ?? words.namePlaceholder) : typed, size: 72)
                    nameField
                }
                .padding(.top, 22)
                swatches(selected: pressed ?? opening.color).padding(.top, 16)
                if let failure {
                    StatusDot(tone: .bad, text: failure).padding(.top, Theme.Space.s)
                }
                Button { Task { await finishProfile() } } label: {
                    Text(words.continue).frame(maxWidth: .infinity, minHeight: FirstRunRules.Size.primary - 2 * PillButtonStyle.vertical)
                }
                .buttonStyle(PillButtonStyle(prominent: true))
                .disabled(busy)
                .focused($onContinue)
                .padding(.top, 22)
            }
        }
        // TV starts on Continue (the field by up); the phone, iPad and Mac focus nothing, so no keyboard covers it.
        .defaultFocus($onContinue, FirstRunRules.profileFocus(tv: tvForm) == .continueButton)
    }

    /// The Name field: "Main" as its placeholder and no prefill (the relay sends no name); empty keeps the name.
    private var nameField: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(words.nameLabel)
                .textFont(12, weight: .medium, relativeTo: .caption)
                .foregroundStyle(Color.white.opacity(0.64))
                .accessibilityHidden(true)
            TextField(words.nameLabel, text: $typed, prompt: Text(words.namePlaceholder))
                .textFieldStyle(.plain)
                .textFont(18, relativeTo: .body)
                .foregroundStyle(.white)
                #if os(iOS)
                .textContentType(.nickname)
                .submitLabel(.done)
                #endif
                .onChange(of: typed) { _, now in if now.count > 30 { typed = String(now.prefix(30)) } }
        }
        .padding(.horizontal, 16)
        .frame(maxWidth: .infinity, minHeight: 60, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 16, style: .continuous).fill(.white.opacity(0.1)))
        .overlay(RoundedRectangle(cornerRadius: 16, style: .continuous).strokeBorder(.white.opacity(0.55), lineWidth: 1.5))
    }

    private func swatches(selected: String?) -> some View {
        HStack(spacing: 12) {
            ForEach(FirstRunRules.swatches, id: \.self) { colour in
                let on = colour == selected
                Button { pressed = colour } label: {
                    Circle()
                        .fill(ProfileAvatar.color(colour))
                        .frame(width: FirstRunRules.Size.swatch, height: FirstRunRules.Size.swatch)
                        .overlay {
                            if on { Circle().strokeBorder(.white, lineWidth: 2).padding(-5) }
                        }
                        .contentShape(Circle())
                }
                .buttonStyle(FocusPlainStyle(shape: Circle()))
                .accessibilityLabel("\(words.colourLabel): \(colour.capitalized)")
                .accessibilityAddTraits(on ? .isSelected : [])
            }
        }
        .padding(.horizontal, 5)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(words.colourLabel)
    }

    /// Continue: `finishProfileStep` with only what the user typed or pressed (an untouched form sends both empty and
    /// writes nothing), then the setup sheet. One press, one write.
    private func finishProfile() async {
        guard !busy else { return }
        busy = true
        failure = nil
        defer { busy = false }
        do {
            try await core.perform("finishProfileStep", FirstRunRules.profilePayload(typed: typed, pressed: pressed, stored: owner?.avatar ?? ""))
            go(FirstRunRules.next(entry(.profileDone)))
        } catch {
            failure = ErrorWords.of(error)
        }
    }

    // MARK: Setup

    private var profileName: String {
        let name = typed.trimmingCharacters(in: .whitespacesAndNewlines)
        return name.isEmpty ? (owner?.name ?? words.namePlaceholder) : name
    }

    /// The device's language as the core lists it, or the system's while the list is not in (nil: unknown or unnamed).
    private var deviceLanguage: String? {
        if !languages.isEmpty { return SubtitleStepRules.listed(languages) }
        return DeviceCaps.language.flatMap { code in Language.all.contains { $0.audio == code } ? code : nil }
    }

    private var chipRule: (chips: [String], selected: String) {
        FirstRunRules.chips(device: deviceLanguage, chosen: picked.map { [$0] } ?? (prefs.snapshot?.subtitleLanguages ?? []))
    }

    private var selectedChip: String { chip ?? chipRule.selected }

    private var setupSheet: some View {
        let q = StreamChoice.words.question
        let wide = metrics.wide
        // Wide forms: three columns at 1.15 / 1 / 1 (round2.html `.g3`); the phone stacks them.
        let columns = wide ? AnyLayout(ShareColumns.setup) : AnyLayout(VStackLayout(alignment: .leading, spacing: 22))
        return WallSheet(sheet: .setup) {
            VStack(alignment: .leading, spacing: 0) {
                SheetHead(title: AccountRules.fill(words.setupTitle, ["profile": profileName]), line: wide ? words.setupLine : nil,
                          link: words.skip, linkDisabled: busy) { Task { await skipSetup() } }
                columns {
                    section(words.bringTitle) {
                        SheetOption(title: words.bringLibrary, line: wide ? words.bringLibraryLine : nil, on: bring) { bring = true }
                        SheetOption(title: words.startFresh, line: wide ? words.startFreshLine : nil, on: !bring) { bring = false }
                    }
                    section(q.title) {
                        HStack(spacing: 0) {
                            segment(q.fast.label, glyph: "zap", on: !best) { best = false }
                            segment(q.best.label, glyph: "gem", on: best) { best = true }
                        }
                        .padding(4)
                        .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(.white.opacity(0.08)))
                        Text(best ? q.best.text : q.fast.text)
                            .textFont(13, relativeTo: .footnote)
                            .foregroundStyle(Color.white.opacity(0.64))
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    section(words.subtitlesTitle) {
                        FlowRow(spacing: 8) {
                            ForEach(chipRule.chips, id: \.self) { code in chipView(code) }
                        }
                    }
                }
                .padding(.top, 4)
                if let failure {
                    HStack(spacing: Theme.Space.s) {
                        StatusDot(tone: .bad, text: failure)
                        Button(words.tryAgain) { Task { await startWatching() } }
                            .buttonStyle(PillButtonStyle())
                            .disabled(busy)
                    }
                    .padding(.top, Theme.Space.m)
                }
                setupFooter
            }
        }
        // TV: every answer is pre-chosen, so OK on Start watching alone finishes first run.
        .defaultFocus($onContinue, true)
    }

    private func section<Content: View>(_ title: String, @ViewBuilder _ content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            SheetSection(title: title).padding(.top, 18)
            content()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(title)
    }

    private func segment(_ title: String, glyph: String, on: Bool, pick: @escaping () -> Void) -> some View {
        Button(action: pick) {
            Label(title, glyph: glyph)
                .labelStyle(PillLabelStyle())
                .textFont(16, weight: .semibold, relativeTo: .body)
                .foregroundStyle(on ? Theme.Palette.onPrimary : Theme.Palette.muted)
                .frame(maxWidth: .infinity, minHeight: FirstRunRules.Size.segment)
                .background(RoundedRectangle(cornerRadius: 11, style: .continuous).fill(on ? Color.white : .clear))
                .contentShape(RoundedRectangle(cornerRadius: 11, style: .continuous))
        }
        .buttonStyle(FocusPlainStyle(shape: RoundedRectangle(cornerRadius: 11, style: .continuous)))
        .accessibilityAddTraits(on ? .isSelected : [])
    }

    @ViewBuilder private func chipView(_ code: String) -> some View {
        switch code {
        case FirstRunRules.none:
            SheetChip(title: words.noSubtitles, on: selectedChip == code) { chip = code }
        case FirstRunRules.more:
            // Today's language picker; the last chip.
            SheetChip(title: words.moreLanguages, glyph: "search", on: false) { showLanguages = true }
        default:
            SheetChip(title: languages.first { $0.code == code }?.name ?? Language.name(code), flag: TrackPicker.flag(code),
                      on: selectedChip == code) { chip = code }
        }
    }

    private var setupFooter: some View {
        let wide = metrics.wide
        let sources = HStack(spacing: 10) {
            Glyph("layers", size: 16).accessibilityHidden(true)
            Text(words.sourcesOn).textFont(14, relativeTo: .callout)
            Button(words.review) { showReview = true }
                .buttonStyle(.textLink)
                .textFont(14, weight: .semibold, relativeTo: .callout)
                .foregroundStyle(tvForm ? Theme.Palette.text : Theme.Palette.key)
                .frame(minHeight: FirstRunRules.Size.link)
                .contentShape(Rectangle())
        }
        .foregroundStyle(Theme.Palette.muted)
        let start = Button { Task { await startWatching() } } label: {
            Text(words.startWatching).frame(minWidth: wide ? 240 - 2 * PillButtonStyle.horizontal : nil, maxWidth: wide ? nil : .infinity,
                                            minHeight: FirstRunRules.Size.primary - 2 * PillButtonStyle.vertical)
        }
        .buttonStyle(PillButtonStyle(prominent: true))
        .disabled(busy)
        .focused($onContinue)
        return Group {
            if wide {
                VStack(spacing: 22) {
                    Rectangle().fill(Theme.Palette.line).frame(height: 1)
                    HStack {
                        sources
                        Spacer(minLength: Theme.Space.m)
                        start
                    }
                }
                .padding(.top, 28)
            } else {
                VStack(alignment: .leading, spacing: 14) {
                    sources
                    start
                }
                .padding(.top, 22)
            }
        }
    }

    private func loadLanguages() async {
        let device: [String: any Sendable] = DeviceCaps.language.map { ["device": $0] } ?? [:]
        languages = (try? await core.call("languages", device, as: [LanguageChoice].self)) ?? []
    }

    /// Start watching: one `setPrefs` (the shown chip always, the stream patch only for Best quality), then the
    /// recommended sources, or the import's sheet first. A failure keeps every answer and says which part failed; a
    /// part that worked is not sent again.
    private func startWatching() async {
        guard !busy else { return }
        busy = true
        failure = nil
        defer { busy = false }
        if !prefsSaved {
            let patch = FirstRunRules.setupPatch(chip: selectedChip, best: best ? StreamChoice.patch(StreamChoice.words.question.best) : nil)
            do {
                try await core.perform("setPrefs", ["patch": patch])
                prefsSaved = true
            } catch {
                failure = words.setupPrefsFailed
                return
            }
        }
        let next = FirstRunRules.afterSetup(bring: bring)
        if next == .importing {
            go(.importing)
        } else {
            await installSources()
        }
    }

    /// Skip (D8): the recommended sources, no prefs, then Home.
    private func skipSetup() async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        await installSources()
    }

    /// Every absent recommended source on, then Home; a failure keeps the sheet with its line.
    private func installSources() async {
        if await DefaultsStep(core: core, scope: .missing).install() {
            go(.home)
        } else {
            failure = words.setupSourcesFailed
        }
    }

    // MARK: Import

    private var importSheet: some View {
        WallSheet(sheet: .importing) {
            VStack(alignment: .leading, spacing: Theme.Space.s) {
                WallBack { go(.setup) }
                // Done brings the defaults the import did not bring and opens Home; Skip import the same without it.
                ImportScreen(core: core, finished: { Task { await installSources() } }, skip: { Task { await installSources() } })
                if let failure { StatusDot(tone: .bad, text: failure) }
            }
        }
    }
}

/// Settings' setup check, alone as a page: every line has a dot, a word and, when something is missing, its fix.
private struct SetupCheckPage: View {
    let core: CoreClient
    @State private var addons: LiveRecord<AddonsSnapshot>
    /// The defaults the check's fixes read, again whenever the list changes.
    @State private var fixer: DefaultsStep
    @State private var pingMs: Int?
    @State private var pinged = false
    @State private var online = true
    @State private var busy: String?
    @State private var note: (ok: Bool, text: String)?
    @Environment(\.metrics) private var metrics

    init(core: CoreClient) {
        self.core = core
        _addons = State(initialValue: LiveRecord("addons", core: core))
        _fixer = State(initialValue: DefaultsStep(core: core, scope: .all))
    }

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.l) { checkStep }
                .frame(maxWidth: DefaultsRules.column(wide: metrics.wide, tv: onTV) ?? .infinity, alignment: .leading)
                .frame(maxWidth: .infinity)
                .padding(.horizontal, metrics.gutter)
                .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .topScrim()
        .background(Theme.Palette.bg)
        .task(id: addons.taskId) { addons.start() }
        .task(id: addons.snapshot) { await fixer.load() }
        .task { await pingLoop() }
        .task { await watchNetwork() }
    }

    private var checkStep: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            VStack(alignment: .leading, spacing: 10) {
                Text("Setup check")
                    .display(metrics.wide ? 44 : Theme.type.title1 + 4)
                    .foregroundStyle(Theme.Palette.text)
                    .accessibilityAddTraits(.isHeader)
                Text("Every line says what it found, and what to do when something is missing.")
                    .textFont(Theme.type.body)
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
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
