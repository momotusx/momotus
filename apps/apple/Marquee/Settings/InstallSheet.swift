import Observation
import SwiftUI

// The addon install sheet (plans/addon-install-sheet.md): every install a
// person starts outside onboarding asks first. The words and the Adds line
// are apps/shared/install-sheet.json (InstallSheetTests checks them).

/// The sheet's words, and its lines from what the addon says about itself.
enum InstallSheetRules {
    static let title = "Install {name}?"
    static let from = "From {host}"
    static let adds = "Adds {what}."
    static let addsFor = "Adds {what} for {kinds}."
    static let note = "It comes from a third party. Momotus does not check it."
    static let install = "Install"
    static let cancel = "Cancel"
    static let checking = "Checking"

    /// Every word by its key in the shared file, for the parity test.
    static let table: [String: String] = [
        "title": title, "from": from, "adds": adds, "addsFor": addsFor, "note": note,
        "install": install, "cancel": cancel, "checking": checking,
    ]

    /// A manifest resource in plain words; other resources are left out.
    static let resources: [String: String] = [
        "stream": "sources", "subtitles": "subtitles", "catalog": "catalogs",
        "meta": "title details", "addon_catalog": "addon lists",
    ]

    /// "Adds sources and subtitles for movies and series.", or nil when it
    /// names no resource the app has words for.
    static func addsLine(provides: [String], types: [String]) -> String? {
        let what = unique(provides.compactMap { resources[$0] })
        guard !what.isEmpty else { return nil }
        // A manifest's types as written: " Movie " is movies, an empty one nothing.
        let types = types.map { $0.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() }.filter { !$0.isEmpty }
        let kinds = unique(types.map(ListedAddon.kindWord))
        let words = adds.replacingOccurrences(of: "{what}", with: DefaultsRules.namesLine(what))
        // Addon lists are lists of addons, not of movies.
        guard !kinds.isEmpty, what != [resources["addon_catalog"]] else { return words }
        return addsFor
            .replacingOccurrences(of: "{what}", with: DefaultsRules.namesLine(what))
            .replacingOccurrences(of: "{kinds}", with: DefaultsRules.namesLine(kinds))
    }

    /// The sheet for one addon; its Install runs `run`.
    static func ask(name: String, host: String, provides: [String], types: [String], run: @escaping @MainActor () -> Void) -> InstallAsk {
        let lines = [from.replacingOccurrences(of: "{host}", with: host), addsLine(provides: provides, types: types), note]
        return InstallAsk(title: title.replacingOccurrences(of: "{name}", with: name), lines: lines.compactMap { $0 }, run: run)
    }

    private static func unique(_ words: [String]) -> [String] {
        var seen = Set<String>()
        return words.filter { seen.insert($0).inserted }
    }
}

/// One question for the sheet: its title, its lines, and what Install runs.
struct InstallAsk: Identifiable {
    let id = UUID()
    let title: String
    let lines: [String]
    let run: @MainActor () -> Void
}

/// The question waiting for an answer. Install runs it once; Cancel, or
/// closing the sheet any other way, runs nothing.
@MainActor @Observable
final class InstallGate {
    private(set) var asking: InstallAsk?
    /// Install does nothing this long after the sheet shows, so the press
    /// that opened it (a held or repeated Return, Enter or OK) never answers
    /// it, as on the web.
    @ObservationIgnored var armAfter: Duration = .milliseconds(400)
    @ObservationIgnored private var shown = ContinuousClock.now

    func ask(_ ask: InstallAsk) {
        asking = ask
        shown = .now
    }

    /// The sheet is on screen: Install arms `armAfter` from now.
    func presented() { shown = .now }

    func confirm() {
        guard let run = asking?.run, ContinuousClock.now - shown >= armAfter else { return }
        asking = nil
        run()
    }

    func cancel() { asking = nil }
}

extension View {
    /// The install sheet for `gate`'s question: a sheet on iOS and macOS, a
    /// card over the dimmed page on tvOS. `closed` runs once it has gone
    /// (focus back to what opened it).
    func installSheet(_ gate: InstallGate, closed: (() -> Void)? = nil) -> some View {
        let shown = Binding(get: { gate.asking }, set: { if $0 == nil { gate.cancel() } })
        let sheet = { (ask: InstallAsk) in
            ConfirmSheet(title: ask.title, lines: ask.lines, action: InstallSheetRules.install, cancel: InstallSheetRules.cancel,
                         confirm: gate.confirm, dismiss: gate.cancel)
                .onAppear { gate.presented() }
        }
        #if os(tvOS)
        return fullScreenCover(item: shown, onDismiss: closed, content: sheet)
        #else
        return self.sheet(item: shown, onDismiss: closed, content: sheet)
        #endif
    }
}

/// A question with two answers: a title, its lines, the action and Cancel.
/// Generic, so the marketplace's consent sheet can use it too. Off the TV the
/// action is the key-fill primary and Cancel the cancel action (Esc); Return
/// presses neither, so a held one never installs. On TV Cancel comes first,
/// focused as the white primary (DECISION.md, Confirmation).
struct ConfirmSheet: View {
    let title: String
    let lines: [String]
    let action: String
    let cancel: String
    let confirm: @MainActor () -> Void
    let dismiss: @MainActor () -> Void

    var body: some View {
        #if os(tvOS)
        tv
        #elseif os(iOS)
        // The words scroll at large text sizes; the buttons stay pinned low,
        // so Install is never cut at the medium detent.
        ScrollView {
            words.padding(Theme.Space.m)
        }
        .scrollBounceBehavior(.basedOnSize)
        .safeAreaInset(edge: .bottom) {
            buttons
                .padding(.horizontal, Theme.Space.m)
                .padding(.vertical, Theme.Space.s)
                .background(Theme.Palette.bg)
        }
        .background(Theme.Palette.bg)
        .accessibilityAddTraits(.isModal)
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
        .presentationCornerRadius(10)
        #else
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            words
            buttons
        }
        .padding(Theme.Space.m)
        .frame(width: 460)
        .fixedSize(horizontal: false, vertical: true)
        .background(Theme.Palette.bg)
        .accessibilityAddTraits(.isModal)
        #endif
    }

    private var words: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Text(title)
                .textFont(titleSize, weight: .bold, relativeTo: .title2)
                .foregroundStyle(Theme.Palette.text)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityAddTraits(.isHeader)
            ForEach(lines, id: \.self) { line in
                Text(line)
                    .textFont(Theme.type.body)
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    #if os(tvOS)
    /// The TV's question in Title 2 (DECISION.md, Six digits).
    private var titleSize: CGFloat { Theme.type.title2 }
    private enum Focus { case cancel, action }
    @FocusState private var focus: Focus?

    private var tv: some View {
        VStack(alignment: .leading, spacing: Theme.Space.l) {
            words
            HStack(spacing: Theme.Space.s) {
                Button(cancel, action: dismiss)
                    .buttonStyle(PillButtonStyle(prominent: true))
                    .focused($focus, equals: .cancel)
                Button(action, action: confirm)
                    .buttonStyle(PillButtonStyle())
                    .focused($focus, equals: .action)
            }
        }
        .padding(Theme.Space.l)
        .frame(width: 1000)
        // The confirmation card's radius (DECISION.md, Radii).
        .background(Theme.Palette.surface, in: RoundedRectangle(cornerRadius: 24, style: .continuous))
        .accessibilityAddTraits(.isModal)
        .defaultFocus($focus, .cancel)
        .onExitCommand(perform: dismiss)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .presentationBackground(Theme.Palette.scrim)
    }
    #else
    private var titleSize: CGFloat { Theme.type.title3 }

    @ViewBuilder private var buttons: some View {
        #if os(iOS)
        // The phone's page sheet: the action first, both full width.
        VStack(spacing: Theme.Space.s) {
            Button(action: confirm) { Text(action).frame(maxWidth: .infinity) }
                .buttonStyle(PillButtonStyle(key: true))
            Button(action: dismiss) { Text(cancel).frame(maxWidth: .infinity) }
                .buttonStyle(PillButtonStyle())
                .keyboardShortcut(.cancelAction)
        }
        #else
        HStack(spacing: Theme.Space.s) {
            Spacer(minLength: 0)
            Button(cancel, action: dismiss)
                .buttonStyle(PillButtonStyle())
                .keyboardShortcut(.cancelAction)
            Button(action, action: confirm)
                .buttonStyle(PillButtonStyle(key: true))
        }
        #endif
    }
    #endif
}
