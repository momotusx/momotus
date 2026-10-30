import Foundation
import SwiftUI

/// A Settings page opened by value (the debug launch hooks).
enum SettingsPage: Hashable {
    case findAddons
    case pairing
    case sync
    case kit
    case compare
    case trakt
    case streamChoice
}

/// Settings A1 (the owner's pick of 2026-10-28, design/DECISION.md) in sections (plans/settings-sections.md, "On
/// A1"). The profile sits on a film's backdrop (the first Continue Watching title with one, else Home's first hero;
/// a kids profile only its own Continue Watching), with the film's logo at the right; under it a section menu (TV
/// and wide screens) beside the chosen section, centered in the content column. The phone lists the sections under
/// the header, each opening its own page (`only`). Sources (addons) and the few playback choices worth making;
/// everything else has a device-aware default.
struct SettingsScreen: View {
    let core: CoreClient
    /// Home's view, whose Continue Watching or hero lends the header its art.
    let home: HomeModel
    /// The phone's page of one section; nil for Settings itself.
    let only: SettingsSection?
    /// TV and wide: the section shown beside the menu.
    @State private var chosen: SettingsSection = .sources
    @State private var prefs: LiveRecord<Prefs>
    @State private var addons: LiveRecord<AddonsSnapshot>
    /// Nuvio plugin repositories (R7): the row shows only where plugins run (full builds, not kids).
    @State private var plugins: PluginsAnswer?
    @Environment(\.metrics) private var metrics
    #if !os(tvOS)
    @AppStorage(HeroTrailers.previewsKey) private var previewsOn = true
    #endif
    @Environment(SyncModel.self) private var sync
    @Environment(ProfileSession.self) private var profiles
    @Environment(DebridSession.self) private var debrid
    @Environment(TraktModel.self) private var trakt
    /// The page's scroll (the menu holds under the bar and goes back to the column's top).
    @State private var position = ScrollPosition(edge: .top)
    @State private var offset: CGFloat = 0
    @State private var columnTop: CGFloat = 0
    @State private var listHeight: CGFloat = 0
    @State private var menuHeight: CGFloat = 0
    /// Where the menu holds and a section lands: under the window's top bar.
    @State private var stickTop: CGFloat = 0
    @FocusState private var focus: SettingsFocus?
    #if !os(tvOS)
    @Environment(DownloadKeeper.self) private var keeper: DownloadKeeper?
    #endif

    /// TV: the profile block, a menu entry, or a section's first row.
    enum SettingsFocus: Hashable {
        case profile
        case menu(SettingsSection)
        case row(SettingsSection)
    }

    init(core: CoreClient, home: HomeModel, only: SettingsSection? = nil) {
        self.core = core
        self.home = home
        self.only = only
        _prefs = State(initialValue: LiveRecord("settings", core: core))
        _addons = State(initialValue: LiveRecord("addons", core: core))
    }

    private var sizes: SettingsRules.Sizes { tvForm ? .tv : metrics.wide ? .wide : .phone }
    private var sections: [SettingsSection] { SettingsSection.shown(tv: tvForm, downloads: downloadsShown) }
    /// Settings' Downloads (plans/offline-downloads.md 3.7): iOS and macOS, never a kids profile.
    private var downloadsShown: Bool {
        #if os(tvOS)
        false
        #else
        keeper?.snapshot != nil && DownloadGate.controls(tv: DownloadGate.tv, kids: kids)
        #endif
    }
    private var me: ProfileInfo? { profiles.snapshot?.current }
    private var kids: Bool { me?.kids == true }
    /// Settings' Connect Google: the relay's providers and the account's links, read while joined.
    @State private var offered: [String] = []
    @State private var linked: [String]?

    /// Sign in shows signed out, where the build has a sync service (the Account page's own rule).
    private var signInServer: String? {
        sync.joined || sync.keyUnreadable ? nil : AccountServer.default
    }

    /// Connect Google is the owner's (D16), never a TV's (it links nothing there).
    private func readLinks() async {
        guard !tvForm, sync.joined, SyncRules.mayDeleteAccount(profiles.snapshot), let server = AccountServerOf.joined else {
            linked = nil
            return
        }
        offered = (try? await sync.relayInfo(server: server)) ?? []
        linked = (try? await sync.identities())?.map(\.provider)
    }
    private var art: Tile? { SettingsRules.art(rows: home.rows, heroes: home.header.heroes, kids: kids) }
    private let space = "settings-page"

    var body: some View {
        GeometryReader { page in
            let pad = SettingsRules.pad(width: page.size.width, gutter: metrics.gutter, column: sizes.column)
            let stick = page.safeAreaInsets.top + Theme.Space.m
            ScrollView(.vertical) {
                VStack(alignment: .leading, spacing: 0) {
                    if let only {
                        // A section's own page (the phone): its title, then its groups.
                        ScreenTitle(text: only.title)
                            .padding(.horizontal, 20)
                            .padding(.top, page.safeAreaInsets.top + 8)
                        list.padding(.horizontal, 16).padding(.top, 18)
                    } else {
                        header(pad: pad, top: page.safeAreaInsets.top)
                        if metrics.wide {
                            HStack(alignment: .top, spacing: sizes.gap) {
                                menu
                                    .frame(width: sizes.menu)
                                    .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { menuHeight = $0 }
                                    .offset(y: SettingsRules.menuShift(columnTop: columnTop - offset, stick: stick, room: listHeight - menuHeight))
                                    #if os(tvOS)
                                    .focusSection()
                                    #endif
                                list
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { listHeight = $0 }
                                    #if os(tvOS)
                                    .focusSection()
                                    #endif
                            }
                            .padding(.horizontal, pad)
                            .padding(.top, tvForm ? 32 : 12)
                            .onGeometryChange(for: CGFloat.self) { $0.frame(in: .named(space)).minY } action: { columnTop = $0 }
                        } else {
                            list.padding(.horizontal, 16).padding(.top, 18)
                            sectionList.padding(.horizontal, 16).padding(.top, kids ? 18 : 30)
                        }
                    }
                }
                .padding(.bottom, Theme.Space.l)
                .coordinateSpace(.named(space))
            }
            .scrollPosition($position)
            .onScrollGeometryChange(for: CGFloat.self) { $0.contentOffset.y + $0.contentInsets.top } action: { offset = $1 }
            .onChange(of: stick, initial: true) { _, now in stickTop = now }
            .scrollIndicators(.hidden)
            // The header's art runs under the status bar, the Mac's toolbar and the TV's bar.
            .ignoresSafeArea(edges: .top)
            #if os(tvOS)
            // The header scrolls away as the remote moves down into the menu, and comes back on the profile.
            .onChange(of: focus) { _, now in
                switch now {
                case .profile: position.scrollTo(edge: .top)
                case .menu: if offset < columnTop - stick { position.scrollTo(y: columnTop - stick) }
                default: break
                }
            }
            .defaultFocus($focus, .profile)
            #endif
        }
        .background(Theme.Palette.bg)
        .topScrim(offset > 1)
        .task(id: prefs.taskId) { prefs.start() }
        .task(id: addons.taskId) { addons.start() }
        .task { plugins = try? await core.call("plugins", as: PluginsAnswer.self) }
        #if DEBUG
        .task {
            guard only == nil, let section = DebugHooks.settingsSection else { return }
            try? await Task.sleep(for: .seconds(4))
            jump(section)
        }
        #endif
        .navigationDestination(for: SettingsPage.self) { page in
            switch page {
            case .findAddons: FindAddonsScreen(core: core)
            case .pairing: PairingScreen(core: core)
            case .sync: SyncScreen()
            case .kit: RecoveryKitScreen()
            case .compare: ForkReviewScreen()
            case .trakt: TraktScreen()
            case .streamChoice: StreamChoiceScreen(core: core)
            }
        }
    }

    /// What the list shows: the one section of a section page; beside the menu, the chosen one; the phone's
    /// Settings, only the header's edits (the sections are its rows).
    private func shows(_ s: SettingsSection) -> Bool {
        if let only { return s == only }
        return metrics.wide ? s == chosen : s == .profiles
    }

    /// The phone: one row per section, each opening its page.
    private var sectionList: some View {
        SettingsGroup(sizes: sizes) {
            ForEach(sections.filter { $0 != .profiles }) { s in
                NavigationLink {
                    SettingsScreen(core: core, home: home, only: s)
                } label: {
                    A1Row(tone: s.tone, glyph: s.glyph, title: s.title, detail: s.summary, sizes: sizes)
                }
                .buttonStyle(LitRowStyle(radius: sizes.radius))
            }
        }
    }

    // MARK: header

    /// The profile on the film's backdrop: one height from the first frame, art or none.
    @ViewBuilder private func header(pad: CGFloat, top: CGFloat) -> some View {
        let height = sizes.header + top
        if metrics.wide {
            HStack(alignment: .bottom, spacing: tvForm ? 40 : 24) {
                VStack(alignment: .leading, spacing: tvForm ? 28 : 18) {
                    if let me {
                        profileBlock(me)
                        if SettingsRules.canEdit(kids: kids) {
                            HStack(spacing: tvForm ? 20 : 12) { editButtons(me) }
                                .padding(.leading, sizes.avatar + (tvForm ? 40 : 28))
                        }
                    }
                }
                Spacer(minLength: 0)
                if let logo = art?.logo {
                    SettingsLogo(url: logo, size: sizes.logo)
                }
            }
            .padding(.horizontal, pad)
            .padding(.bottom, sizes.foot)
            .settingsHeader(height: height) { SettingsArt(url: art?.background, wide: true).frame(height: height) }
        } else {
            VStack(alignment: .leading, spacing: 18) {
                Text("Settings")
                    .display(34)
                    .foregroundStyle(.white)
                    .accessibilityAddTraits(.isHeader)
                    .padding(.leading, 4)
                if let me { profileCard(me) }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, sizes.foot)
            .settingsHeader(height: height) { SettingsArt(url: art?.background, wide: false).frame(height: height) }
        }
    }

    /// TV and wide: the avatar, the name (the page's second heading) and its line; the whole block opens the picker.
    private func profileBlock(_ me: ProfileInfo) -> some View {
        NavigationLink {
            ProfilePicker(mode: .push, core: core)
        } label: {
            ProfileBlock(me: me, many: (profiles.snapshot?.profiles.count ?? 1) > 1, sizes: sizes)
        }
        .buttonStyle(ProfileBlockStyle())
        .focused($focus, equals: .profile)
    }

    @ViewBuilder private func editButtons(_ me: ProfileInfo) -> some View {
        NavigationLink {
            EditProfile(profileId: me.id)
        } label: {
            Label("Edit profile", glyph: "user-pen", size: tvForm ? 26 : 16)
        }
        .buttonStyle(PillButtonStyle())
        if SettingsRules.canManage(kids: kids, owner: me.isOwner) {
            NavigationLink {
                ManageProfiles(core: core)
            } label: {
                Label("Manage profiles", glyph: "users", size: tvForm ? 26 : 16)
            }
            .buttonStyle(PillButtonStyle())
        }
    }

    /// The phone: the profile as one glass card, one tap to the picker.
    private func profileCard(_ me: ProfileInfo) -> some View {
        let shape = RoundedRectangle(cornerRadius: 26, style: .continuous)
        return NavigationLink {
            ProfilePicker(mode: .push, core: core)
        } label: {
            HStack(spacing: 14) {
                ProfileAvatar(me.avatar, name: me.name, size: 56)
                VStack(alignment: .leading, spacing: 2) {
                    Text(me.name)
                        .textFont(19, weight: .semibold, relativeTo: .headline)
                        .foregroundStyle(Theme.Palette.text)
                        .accessibilityAddTraits(.isHeader)
                    Text(SettingsRules.profileLine(many: (profiles.snapshot?.profiles.count ?? 1) > 1))
                        .textFont(13, relativeTo: .footnote)
                        .foregroundStyle(Theme.Palette.dim)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 0)
                Glyph("chevron-right", size: 20).foregroundStyle(Color(red: 235 / 255, green: 235 / 255, blue: 245 / 255).opacity(0.5))
            }
            .multilineTextAlignment(.leading)
            .padding(.horizontal, 16)
            .padding(.vertical, 14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(shape.fill(Color(red: 44 / 255, green: 44 / 255, blue: 46 / 255).opacity(0.82)))
            .background(.ultraThinMaterial, in: shape)
            .contentShape(shape)
            .accessibilityElement(children: .combine)
        }
        .buttonStyle(.row)
    }

    // MARK: menu

    /// TV and wide: the sections with their tiles; an entry shows its section beside the menu (and on TV takes the
    /// remote into it), and fills; Profiles goes up to the header.
    private var menu: some View {
        VStack(alignment: .leading, spacing: tvForm ? 8 : 2) {
            // The page's heading, shown as the menu's label.
            Text("Settings")
                .textFont(tvForm ? Theme.type.callout : 15, weight: tvForm ? .bold : .semibold, relativeTo: .headline)
                .foregroundStyle(Theme.Palette.dim)
                .accessibilityAddTraits(.isHeader)
                .padding(.horizontal, tvForm ? 20 : 12)
                .padding(.top, tvForm ? 0 : 8)
                .padding(.bottom, tvForm ? 12 : 10)
            ForEach(sections) { s in
                Button { jump(s) } label: {
                    MenuEntry(section: s, sizes: sizes)
                }
                .buttonStyle(LitRowStyle(radius: tvForm ? Theme.Radius.l : 10, selected: chosen == s))
                .focused($focus, equals: .menu(s))
                .accessibilityAddTraits(chosen == s ? .isSelected : [])
            }
        }
    }

    /// At once: the Mac's scroll view leaves an animated jump where it was.
    private func jump(_ s: SettingsSection) {
        if s == .profiles {
            position.scrollTo(edge: .top)
        } else {
            chosen = s
            if offset > columnTop - stickTop { position.scrollTo(y: max(0, columnTop - stickTop)) }
        }
        #if os(tvOS)
        // The remote goes on into the section; it stays on the menu where there is nothing to press (About).
        if s == .profiles { focus = .profile } else if s != .about { focus = .row(s) }
        #endif
    }

    // MARK: the lists

    /// The shown section's groups (`shows`); on the phone the header's edits are a group of their own.
    private var list: some View {
        VStack(alignment: .leading, spacing: sizes.sectionGap) {
            if let failure = prefs.failure ?? addons.failure {
                StateLine(symbol: "triangle-alert", text: failure, action: ("Try again", { prefs.retry(); addons.retry() }))
            }
            if only == nil, !metrics.wide, let me, SettingsRules.canEdit(kids: kids) {
                section(.profiles) {
                    SettingsGroup(sizes: sizes) {
                        row(.profiles, tone: .blue, glyph: "user-pen", title: "Edit profile", detail: "Name, picture and PIN.") { EditProfile(profileId: me.id) }
                        if SettingsRules.canManage(kids: kids, owner: me.isOwner) {
                            row(nil, tone: .sky, glyph: "users", title: "Manage profiles", detail: "Add, edit and remove profiles on this account.") { ManageProfiles(core: core) }
                        }
                    }
                }
            }
            section(.sources) {
                SettingsGroup(sizes: sizes) {
                    row(.sources, tone: .green, glyph: "sparkles", title: DefaultsRules.title, detail: DefaultsRules.settingsDetail) {
                        RecommendedAddonsPage(core: core, scope: .all)
                    }
                    row(nil, tone: .teal, glyph: "package-search", title: FindAddonsWords.title, detail: FindAddonsWords.lede) { FindAddonsScreen(core: core) }
                    row(nil, tone: .mint, glyph: "puzzle", title: "Addons", detail: addons.snapshot?.summary ?? "Loading") { AddonsScreen(addons: addons) }
                    if let plugins, plugins.available {
                        row(nil, tone: .teal, glyph: "power", title: PluginWords.title, detail: PluginWords.summary(plugins.repos)) { PluginsScreen(core: core) }
                    }
                    row(nil, tone: .sky, glyph: "key-round", title: "Debrid services",
                        detail: debrid.inUse ?? "Real-Debrid, TorBox, AllDebrid or Premiumize, for fast direct links.") { DebridScreen(core: core) }
                    // An account, as Debrid is (the web's order).
                    row(nil, tone: .blue, glyph: "radio", title: TraktWords.title, detail: TraktRules.rowLine(trakt.status, clock: TraktClock.time)) { TraktScreen() }
                    row(nil, tone: .sea, glyph: "download", title: "Import from Stremio",
                        detail: "Addons, library and progress, with a code. No password typed here.") { ImportScreen(core: core) }
                    row(nil, tone: .grey, glyph: "wrench", title: "Setup check",
                        detail: "Network, display, catalogs, sources and subtitles at a glance.") { OnboardingScreen(core: core, start: .check) }
                }
            }
            section(.account) {
                SettingsGroup(sizes: sizes) {
                    // Signed out, on the build's sync service: Sign in here (a TV: with the phone's QR).
                    if let server = signInServer {
                        #if os(tvOS)
                        row(.account, tone: .blue, glyph: "qr-code", title: AccountWords.shared.tvSignIn, detail: AccountWords.shared.tvSignInHow) {
                            PairingScreen(core: core, autostart: server, heading: AccountWords.shared.tvSignIn, lede: AccountWords.shared.tvSignInHow)
                        }
                        #else
                        row(.account, tone: .blue, glyph: "door-open", title: AccountWords.shared.signInTitle, detail: AccountWords.shared.libraryAdded) {
                            LaterSignIn(server: server)
                        }
                        #endif
                    }
                    row(signInServer == nil ? .account : nil, tone: .sky, glyph: "monitor-smartphone", title: SyncWords.title, detail: syncSummary) { SyncScreen() }
                    // The Account page starts the link and shows how it went.
                    if AccountRules.connectGoogle(offered: offered, linked: linked) {
                        row(nil, tone: .blue, glyph: "link-2",
                            title: AccountRules.fill(AccountWords.shared.connectProvider, ["provider": AccountRules.providerName("google")]),
                            detail: AccountRules.fill(AccountWords.shared.connectProviderText, ["providerAccount": AccountRules.providerAccount("google")])) {
                            SyncScreen(connect: "google")
                        }
                    }
                }
                .task(id: sync.joined) { await readLinks() }
            }
            section(.home) {
                SettingsGroup(sizes: sizes) {
                    row(.home, tone: .mint, glyph: "rows-3", title: "Home rows",
                        detail: "Which catalogs Home shows after Continue Watching, and in what order.") { HomeRowsScreen(core: core) }
                }
            }
            section(.playback) {
                VStack(alignment: .leading, spacing: tvForm ? 20 : 16) {
                    SettingsGroup(sizes: sizes) {
                        row(.playback, tone: .teal, glyph: "sliders-horizontal", title: StreamChoice.words.settings.title,
                            detail: prefs.snapshot.map { StreamChoice.summary($0.streams) } ?? "Loading") { StreamChoiceScreen(core: core) }
                    }
                    if let p = prefs.snapshot {
                        panel { playback(p) }
                    }
                }
            }
            #if !os(tvOS)
            if downloadsShown, let keeper, let snapshot = keeper.snapshot {
                section(.downloads) { DownloadSettingsSection(keeper: keeper, snapshot: snapshot) }
            }
            section(.device) {
                panel { trailerPreviews }
            }
            #endif
            section(.about) { panel { about } }
        }
    }

    /// One section, when shown: its title (a heading; a section page has its own) over its groups.
    @ViewBuilder private func section<Content: View>(_ s: SettingsSection, @ViewBuilder content: () -> Content) -> some View {
        if shows(s) {
            VStack(alignment: .leading, spacing: 0) {
                if only == nil {
                    Text(s.title)
                        .modifier(SectionTitle(sizes: sizes, wide: metrics.wide))
                        .accessibilityAddTraits(.isHeader)
                }
                content()
            }
        }
    }

    /// A row that opens its page; `first` marks the section's first row, where a menu entry sends the remote.
    private func row<Destination: View>(
        _ first: SettingsSection?, tone: SettingsTone, glyph: String, title: String, detail: String,
        @ViewBuilder destination: @escaping () -> Destination
    ) -> some View {
        NavigationLink(destination: destination) {
            A1Row(tone: tone, glyph: glyph, title: title, detail: detail, sizes: sizes)
        }
        .buttonStyle(LitRowStyle(radius: sizes.radius))
        .modifier(FirstRow(section: first, focus: $focus))
    }

    /// Choices and notes on the group's `surface`, inset as a row's words.
    private func panel<Content: View>(@ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 12) { content() }
            .padding(.horizontal, SettingsRowLayout.side)
            .padding(.vertical, tvForm ? 28 : 16)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: sizes.radius, style: .continuous).fill(Theme.Palette.surface))
    }

    private var about: some View {
        VStack(alignment: .leading, spacing: 12) {
            Wordmark(size: 20, spoken: false).padding(.bottom, Theme.Space.xs)
            Text("Momotus \(Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? ""), core \(core.version)")
                .modifier(Note())
            if CoreBuild.missingEngine(field: CoreBuild.field, hasEngine: hasEngine()) {
                Text(CoreBuild.aboutLine).modifier(Note())
            }
            // The licences ship in the app: LICENSE-lucide.txt and each font's OFL.txt.
            // TMDB's terms: its logo and this line wherever its data shows.
            HStack(spacing: Theme.Space.xs) {
                // Decorative: the line beside it names TMDB.
                Image(decorative: ScoreMark.tmdb.asset).resizable().aspectRatio(contentMode: .fit).frame(height: 16)
                Text("This product uses the TMDB API but is not endorsed or certified by TMDB.").modifier(Note())
            }
            Text("Icons: Lucide 1.58.0, ISC licence. Fonts: Inter and IBM Plex Mono, SIL Open Font License 1.1; the name is drawn from Unbounded, same licence.")
                .modifier(Note())
            // The playback libraries' texts ship in the app: Resources/Licences.
            Text("Playback: AetherEngine 7.36.0, LGPL-3 with App Store exception. FFmpeg and libzvbi, LGPL-2.1. dav1d, BSD. zimg, WTFPL. LibTrueHD, LibDTSX and LibDovi, MIT and Apache-2.0.")
                .modifier(Note())
                .modifier(ReadableOnTV())
        }
    }

    /// "Synced 14:02", or what is wrong or waits first.
    private var syncSummary: String {
        let problem = sync.keyUnreadable ? SyncWords.keyUnreadable : sync.keyLeft == .afterLeave ? SyncWords.keyLeft : sync.keyLeft == .earlierInstall ? SyncWords.keyLeftover : sync.problem?.words
        return SyncWords.summary(joined: sync.joined, problem: problem, failed: SyncWords.failedSince(sync.failed),
                                 notes: sync.lastReport?.notes ?? [], at: sync.lastSynced)
    }

    /// The quality, size and audio choices live on Stream choice; subtitles and Up next stay here.
    private func playback(_ p: Prefs) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            subtitles(p)
            Text(Language.firstLine(p.subtitleLanguages))
                .textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.text)
            NavigationLink {
                LanguagePicker(title: "Subtitle language", audio: false, core: core,
                               isOn: { tag in (prefs.snapshot?.subtitleLanguages ?? []).contains { Language.tag($0) == tag } }) { tag in
                    set(["subtitleLanguages": Language.toggled(prefs.snapshot?.subtitleLanguages ?? [], tag)])
                }
            } label: { Label("More languages", glyph: "search") }
            .buttonStyle(.textLink)
            subtitleSize(p)
            upNext(p)
            Text("Languages are listed in the order you turn them on; the first one wins. A subtitle you pick while watching moves its language first.")
                .modifier(Note())
        }
    }

    /// The chosen languages first, in order and numbered, then the other common ones.
    private func subtitles(_ p: Prefs) -> ChipRow<String> {
        let chosen = Language.ordered(p.subtitleLanguages)
        let chips = Language.subtitleChips(p.subtitleLanguages).map { tag in
            Chip(id: tag, title: Language.name(tag), detail: chosen.firstIndex(of: tag).map { String($0 + 1) })
        }
        return ChipRow(label: "Subtitle language", chips: chips, isSelected: { chosen.contains($0) }) { tag in
            set(["subtitleLanguages": Language.toggled(p.subtitleLanguages, tag)])
        }
    }

    /// On every device of the profile, over the system caption size.
    private func subtitleSize(_ p: Prefs) -> ChipRow<SubtitleSize> {
        let chips = SubtitleSize.allCases.map { Chip(id: $0, title: CaptionStyle.words($0)) }
        return ChipRow(label: "Subtitle size", chips: chips, isSelected: { $0 == p.subtitleSize }) { value in
            set(["subtitleSize": value.rawValue])
        }
    }

    private func upNext(_ p: Prefs) -> ChipRow<Bool> {
        let chips = [Chip(id: true, title: "Starts on its own"), Chip(id: false, title: "Wait for me")]
        return ChipRow(label: "Up next", chips: chips, isSelected: { $0 == p.autoplayNext }) { value in
            set(["autoplayNext": value])
        }
    }

    #if !os(tvOS)
    /// This device's own switch, never synced, as the web and Android have it (review r1, W4).
    private var trailerPreviews: some View {
        ChipRow(label: "Trailer previews", chips: [Chip(id: true, title: "On"), Chip(id: false, title: "Off")],
                isSelected: { $0 == previewsOn }) { previewsOn = $0 }
    }
    #endif

    private func set(_ patch: [String: any Sendable]) {
        prefs.performNow("setPrefs", ["patch": patch])
    }
}

/// A row that leads somewhere: icon, title, one line of detail, chevron.
struct SettingsRow: View {
    let symbol: String
    let title: String
    let detail: String
    @State private var hovered = false

    var body: some View {
        HStack(spacing: Theme.Space.s) {
            Glyph(symbol, size: 22)
                .frame(width: 28)
            VStack(alignment: .leading, spacing: 3) {
                Text(title).textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                Text(detail)
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.muted)
            }
            Spacer()
            Glyph("chevron-right").foregroundStyle(Theme.Palette.muted)
        }
        .foregroundStyle(Theme.Palette.text)
        .padding(Theme.Space.s)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
        .brightness(hovered ? 0.04 : 0)
        .contentShape(Rectangle())
        #if !os(tvOS)
        .onHover { hovered = $0 }
        #endif
    }
}

/// TV and wide: the avatar, the name (the page's second heading) and its line. Under TV focus the avatar turns
/// white and rises.
private struct ProfileBlock: View {
    let me: ProfileInfo
    let many: Bool
    let sizes: SettingsRules.Sizes
    @Environment(\.settingsLit) private var lit
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        HStack(spacing: tvForm ? 40 : 28) {
            ProfileAvatar(me.avatar, name: me.name, size: sizes.avatar, lit: lit)
                .overlay(Circle().strokeBorder(Theme.Palette.lineStrong, lineWidth: 1))
                .shadow(color: .black.opacity(lit ? 0.55 : 0.4), radius: lit ? 44 : 20, y: lit ? 44 : 20)
                .scaleEffect(lit && !reduceMotion ? 1.1 : 1)
                .offset(y: lit && !reduceMotion ? -10 : 0)
                .animation(Theme.Motion.focus, value: lit)
            VStack(alignment: .leading, spacing: tvForm ? 12 : 10) {
                Text(me.name)
                    .display(sizes.name)
                    .foregroundStyle(.white)
                    .lineLimit(2)
                    .accessibilityAddTraits(.isHeader)
                Text(SettingsRules.profileLine(many: many))
                    .textFont(sizes.line, weight: .medium, relativeTo: .body)
                    .foregroundStyle(Theme.Palette.muted)
            }
        }
        .multilineTextAlignment(.leading)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
    }
}

/// A menu entry: the section's tile and its word.
private struct MenuEntry: View {
    let section: SettingsSection
    let sizes: SettingsRules.Sizes
    @Environment(\.settingsLit) private var lit

    var body: some View {
        HStack(spacing: tvForm ? 20 : 12) {
            IconTile(tone: section.tone, glyph: section.glyph, size: sizes.menuTile)
            Text(section.title)
                .textFont(tvForm ? Theme.type.body : 16, weight: .semibold, relativeTo: .headline)
                .foregroundStyle(lit ? Theme.Palette.onPrimary : Theme.Palette.text)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, tvForm ? 20 : 12)
        .frame(maxWidth: .infinity, minHeight: sizes.menuRow, alignment: .leading)
        .contentShape(Rectangle())
    }
}

/// A section's title over its group: the phone's quiet group title, the wide screens' heading.
private struct SectionTitle: ViewModifier {
    let sizes: SettingsRules.Sizes
    let wide: Bool

    func body(content: Content) -> some View {
        if wide {
            content
                .display(sizes.heading)
                .foregroundStyle(Theme.Palette.text)
                .padding(.top, tvForm ? 8 : 10)
                .padding(.bottom, tvForm ? 20 : 12)
        } else {
            content
                .textFont(sizes.heading, weight: .semibold, relativeTo: .subheadline)
                .foregroundStyle(Theme.Palette.dim)
                .padding(.horizontal, 16)
                .padding(.bottom, 8)
        }
    }
}

/// A quiet line in a panel.
private struct Note: ViewModifier {
    func body(content: Content) -> some View {
        content
            .textFont(Theme.type.callout, relativeTo: .callout)
            .foregroundStyle(Theme.Palette.muted)
            .fixedSize(horizontal: false, vertical: true)
    }
}

/// A section's first row takes the remote when its menu entry is pressed; the other rows bind no focus value.
private struct FirstRow: ViewModifier {
    let section: SettingsSection?
    let focus: FocusState<SettingsScreen.SettingsFocus?>.Binding

    func body(content: Content) -> some View {
        if let section { content.focused(focus, equals: .row(section)) } else { content }
    }
}
