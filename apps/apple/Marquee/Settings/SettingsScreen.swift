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

/// Sources (addons) and the few playback choices worth making; everything
/// else has a device-aware default.
struct SettingsScreen: View {
    let core: CoreClient
    @State private var prefs: LiveRecord<Prefs>
    @State private var addons: LiveRecord<AddonsSnapshot>
    @Environment(\.metrics) private var metrics
    @Environment(SyncModel.self) private var sync
    @Environment(ProfileSession.self) private var profiles
    @Environment(DebridSession.self) private var debrid
    @Environment(TraktModel.self) private var trakt

    init(core: CoreClient) {
        self.core = core
        _prefs = State(initialValue: LiveRecord("settings", core: core))
        _addons = State(initialValue: LiveRecord("addons", core: core))
    }

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: "Settings")
                if let snapshot = profiles.snapshot, let me = snapshot.current {
                    section("Profiles")
                    NavigationLink {
                        ProfilePicker(mode: .push, core: core)
                    } label: {
                        ProfileRow(stored: me.avatar, name: me.name,
                                   detail: snapshot.profiles.count > 1 ? "Switch profile" : "Add a profile for someone else, or switch")
                    }
                    .buttonStyle(.row)
                    NavigationLink {
                        EditProfile(profileId: me.id)
                    } label: {
                        SettingsRow(symbol: "pencil", title: "Edit profile", detail: "Name, picture and PIN.")
                    }
                    .buttonStyle(.row)
                    if me.isOwner {
                        NavigationLink {
                            ManageProfiles(core: core)
                        } label: {
                            SettingsRow(symbol: "users", title: "Manage profiles", detail: "Add, edit and remove profiles on this account.")
                        }
                        .buttonStyle(.row)
                    }
                }
                section("Sources").padding(.top, Theme.Space.s)
                NavigationLink {
                    RecommendedAddonsPage(core: core, scope: .all)
                } label: {
                    SettingsRow(symbol: "sparkle", title: DefaultsRules.title, detail: DefaultsRules.settingsDetail)
                }
                .buttonStyle(.row)
                NavigationLink {
                    FindAddonsScreen(core: core)
                } label: {
                    SettingsRow(symbol: "package-search", title: FindAddonsWords.title, detail: FindAddonsWords.lede)
                }
                .buttonStyle(.row)
                NavigationLink {
                    AddonsScreen(addons: addons)
                } label: {
                    SettingsRow(symbol: "puzzle", title: "Addons", detail: addonsSummary)
                }
                .buttonStyle(.row)
                NavigationLink {
                    ImportScreen(core: core)
                } label: {
                    SettingsRow(symbol: "download", title: "Import from Stremio", detail: "Addons, library and progress, with a code. No password typed here.")
                }
                .buttonStyle(.row)
                NavigationLink {
                    DebridScreen(core: core)
                } label: {
                    SettingsRow(symbol: "zap", title: "Debrid services", detail: debrid.inUse ?? "Real-Debrid, TorBox, AllDebrid or Premiumize, for fast direct links.")
                }
                .buttonStyle(.row)
                // An account, as Debrid is (the web's order).
                NavigationLink {
                    TraktScreen()
                } label: {
                    SettingsRow(symbol: "history", title: TraktWords.title, detail: TraktRules.rowLine(trakt.status, clock: TraktClock.time))
                }
                .buttonStyle(.row)
                NavigationLink {
                    HomeRowsScreen(core: core)
                } label: {
                    SettingsRow(symbol: "list-video", title: "Home rows", detail: "Which catalogs Home shows, and in what order.")
                }
                .buttonStyle(.row)
                NavigationLink {
                    OnboardingScreen(core: core, start: .check)
                } label: {
                    SettingsRow(symbol: "monitor-check", title: "Setup check", detail: "Network, display, catalogs, sources and subtitles at a glance.")
                }
                .buttonStyle(.row)
                section("Devices and sync").padding(.top, Theme.Space.s)
                NavigationLink {
                    SyncScreen()
                } label: {
                    SettingsRow(symbol: sync.joined ? "refresh-cw" : "qr-code", title: SyncWords.title, detail: syncSummary)
                }
                .buttonStyle(.row)
                if let p = prefs.snapshot {
                    section("Playback").padding(.top, Theme.Space.s)
                    NavigationLink {
                        StreamChoiceScreen(core: core)
                    } label: {
                        SettingsRow(symbol: "layers", title: StreamChoice.words.settings.title, detail: StreamChoice.summary(p.streams))
                    }
                    .buttonStyle(.row)
                    playback(p)
                }
                if let failure = prefs.failure ?? addons.failure {
                    StateLine(symbol: "triangle-alert", text: failure, action: ("Try again", { prefs.retry(); addons.retry() }))
                }
                section("About").padding(.top, Theme.Space.s)
                Text("Marquee \(Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? ""), core \(core.version)")
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
                // The licences ship in the app: LICENSE-lucide.txt and each font's OFL.txt.
                Text("Icons: Lucide 1.58.0, ISC licence. Fonts: Inter and IBM Plex Mono, SIL Open Font License 1.1.")
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
                    .modifier(ReadableOnTV())
            }
            .frame(maxWidth: 880, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .topScrim()
        .task(id: prefs.taskId) { prefs.start() }
        .task(id: addons.taskId) { addons.start() }
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

    private func section(_ title: String) -> some View {
        Text(title)
            .display(Theme.type.title2)
            .foregroundStyle(Theme.Palette.text)
            .accessibilityAddTraits(.isHeader)
    }

    /// "Synced 14:02", or what is wrong or waits first.
    private var syncSummary: String {
        let problem = sync.keyUnreadable ? SyncWords.keyUnreadable : sync.keyLeft == .afterLeave ? SyncWords.keyLeft : sync.keyLeft == .earlierInstall ? SyncWords.keyLeftover : sync.problem?.words
        return SyncWords.summary(joined: sync.joined, problem: problem, failed: SyncWords.failedSince(sync.failed),
                                 notes: sync.lastReport?.notes ?? [], at: sync.lastSynced)
    }

    /// "4 installed. Streams from Torrentio, Sample Source."
    private var addonsSummary: String {
        guard let list = addons.snapshot?.addons else { return "Loading" }
        let streams = list.filter { $0.enabled && $0.provides.contains("stream") }.map(\.name)
        let count = list.count == 1 ? "1 installed." : "\(list.count) installed."
        return streams.isEmpty ? "\(count) No stream source yet." : "\(count) Streams from \(streams.joined(separator: ", "))."
    }

    /// The quality, size and audio choices live on Stream choice; subtitles and Up next stay here.
    private func playback(_ p: Prefs) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            subtitles(p)
            upNext(p)
            Text("Languages are listed in the order you turn them on; the first one wins.")
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.muted)
        }
    }

    private func subtitles(_ p: Prefs) -> ChipRow<String> {
        let chips = Language.all.map { Chip(id: $0.subtitle, title: $0.name) }
        return ChipRow(label: "Subtitles", chips: chips, isSelected: { p.subtitleLanguages.contains($0) }) { value in
            set(["subtitleLanguages": toggled(p.subtitleLanguages, value)])
        }
    }

    private func upNext(_ p: Prefs) -> ChipRow<Bool> {
        let chips = [Chip(id: true, title: "Starts on its own"), Chip(id: false, title: "Wait for me")]
        return ChipRow(label: "Up next", chips: chips, isSelected: { $0 == p.autoplayNext }) { value in
            set(["autoplayNext": value])
        }
    }

    /// Adds a language at the end, or removes it.
    private func toggled(_ list: [String], _ code: String) -> [String] {
        list.contains(code) ? list.filter { $0 != code } : list + [code]
    }

    private func set(_ patch: [String: any Sendable]) {
        Task { await prefs.perform("setPrefs", ["patch": patch]) }
    }
}

/// The languages offered as chips: ISO 639-1 for audio, the 639-2 codes
/// subtitle addons use, and the name in the user's language.
struct Language {
    let audio: String
    let subtitle: String

    var name: String { Locale.current.localizedString(forLanguageCode: audio)?.capitalized ?? audio }

    static let all = [
        Language(audio: "en", subtitle: "eng"), Language(audio: "tr", subtitle: "tur"),
        Language(audio: "es", subtitle: "spa"), Language(audio: "fr", subtitle: "fre"),
        Language(audio: "de", subtitle: "ger"), Language(audio: "it", subtitle: "ita"),
        Language(audio: "pt", subtitle: "por"), Language(audio: "ja", subtitle: "jpn"),
    ]
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
                Text(title).font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                Text(detail)
                    .font(.text(Theme.type.callout, relativeTo: .callout))
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
