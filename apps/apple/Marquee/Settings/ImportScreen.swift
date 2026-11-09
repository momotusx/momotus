import SwiftUI

/// One-way import from a Stremio account. Opening the view starts the sign-in:
/// the user links this device on Stremio's site with a code, never typing a
/// password here. In first run it is a sheet over the poster wall (plans/first-screens.md 3.5): Continue after a
/// finished import, and Skip import while it waits, failed or expired.
struct ImportScreen: View {
    @State private var importer: LiveRecord<ImportSnapshot>
    @State private var attempt = 0
    @FocusState private var onContinue: Bool
    @Environment(\.metrics) private var metrics
    @Environment(\.openURL) private var openURL
    let core: CoreClient
    /// First run: Continue after a finished import.
    let finished: (() -> Void)?
    /// First run's sheet: Skip import, and the sheet draws the page's title and frame.
    let skip: (() -> Void)?

    init(core: CoreClient, finished: (() -> Void)? = nil, skip: (() -> Void)? = nil) {
        self.core = core
        self.finished = finished
        self.skip = skip
        _importer = State(initialValue: LiveRecord("import", core: core))
    }

    private static let title = "Import from Stremio"
    private static let lead = "Brings your addons, library and progress over once. Nothing is sent back to Stremio."

    var body: some View {
        if skip != nil {
            sheet.task(id: [importer.taskId, attempt]) { importer.start() }
        } else {
            page
        }
    }

    /// First run's sheet: the page's words under the sheet's own heading.
    private var sheet: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            SheetHead(title: Self.title, line: Self.lead)
            phase
            if let failure = importer.failure {
                StateLine(symbol: "triangle-alert", text: failure, action: ("Try again", restart))
                skipButton
            }
        }
    }

    @ViewBuilder private var skipButton: some View {
        if let skip {
            Button(FirstRunWords.shared.skipImport, action: skip)
                .buttonStyle(PillButtonStyle())
        }
    }

    private var page: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: Self.title)
                Text(Self.lead)
                    .textFont(Theme.type.body)
                    .foregroundStyle(Theme.Palette.muted)
                phase
                if let failure = importer.failure {
                    StateLine(symbol: "triangle-alert", text: failure, action: ("Try again", restart))
                }
            }
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .topScrim()
        .task(id: [importer.taskId, attempt]) { importer.start() }
    }

    @ViewBuilder private var phase: some View {
        switch importer.snapshot {
        case nil, .starting?:
            // Not while the view could not open: the reason and Try again show instead.
            if importer.failure == nil {
                Label("Asking Stremio for a sign-in code", glyph: "clock")
                    .textFont(Theme.type.body)
                    .foregroundStyle(Theme.Palette.muted)
            }
        case .waitingForLink(let code, let link, _)?:
            HStack(alignment: .top, spacing: Theme.Space.l) {
                VStack(alignment: .leading, spacing: Theme.Space.s) {
                    Text("Open the link on any device signed in to Stremio and enter this code:")
                        .textFont(Theme.type.body)
                        .foregroundStyle(Theme.Palette.text)
                    Text(code)
                        .font(.mono(Theme.type.hero))
                        .tracking(6)
                        .foregroundStyle(Theme.Palette.text)
                        .selectableText()
                    if let url = URL(string: link) {
                        #if os(tvOS)
                        // A TV has no browser: the address to type on a phone or computer, beside its QR.
                        Label(url.host() ?? link, glyph: "external-link")
                            .textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                            .foregroundStyle(Theme.Palette.text)
                        #else
                        Button { openURL(url) } label: { Label(url.host() ?? link, glyph: "external-link") }
                            .buttonStyle(PillButtonStyle(prominent: true))
                        #endif
                    }
                    Label("Waiting for the link. This page moves on by itself.", glyph: "clock")
                        .textFont(Theme.type.callout, relativeTo: .callout)
                        .foregroundStyle(Theme.Palette.muted)
                    skipButton
                }
                if metrics.wide { QRCode(text: link).frame(width: 180, height: 180) }
            }
        case .importing(let step)?:
            Label(step, glyph: "download")
                .textFont(Theme.type.body)
                .foregroundStyle(Theme.Palette.text)
        case .done(let summary)?:
            VStack(alignment: .leading, spacing: 8) {
                Label("Imported", glyph: "circle-check").display(Theme.type.title2)
                Text("\(summary.addonsImported) addons, \(summary.titles) titles, \(summary.inProgress) in progress, \(summary.watched) watched.")
                ForEach(summary.addonsSkipped, id: \.self) { Label($0, glyph: "circle-minus") }
                if summary.seriesUnmatched > 0 {
                    Text("Watched marks for \(summary.seriesUnmatched) series could not be matched to episodes.")
                }
                if let finished {
                    Button("Continue", action: finished)
                        .buttonStyle(PillButtonStyle(prominent: true))
                        .focused($onContinue)
                        .onAppear { onContinue = true }
                        .padding(.top, Theme.Space.s)
                }
            }
            .textFont(Theme.type.body)
            .foregroundStyle(Theme.Palette.text)
        case .expired?:
            StateLine(symbol: "clock-alert", text: "The code expired before it was linked.", action: ("New code", restart))
            skipButton
        case .failed(let reason)?:
            StateLine(symbol: "triangle-alert", text: reason, action: ("Try again", restart))
            skipButton
        }
    }

    /// A new view starts a new sign-in.
    private func restart() {
        importer = LiveRecord("import", core: core)
        attempt += 1
    }
}
