import CoreImage
import CoreImage.CIFilterBuiltins
import SwiftUI

/// One-way import from a Stremio account. Opening the view starts the sign-in:
/// the user links this device on Stremio's site with a code, never typing a
/// password here.
struct ImportScreen: View {
    @State private var importer: LiveRecord<ImportSnapshot>
    @State private var attempt = 0
    @Environment(\.metrics) private var metrics
    @Environment(\.openURL) private var openURL
    let core: CoreClient

    init(core: CoreClient) {
        self.core = core
        _importer = State(initialValue: LiveRecord("import", core: core))
    }

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: "Import from Stremio")
                Text("Brings your addons, library and progress over once. Nothing is sent back to Stremio.")
                    .font(.text(Theme.type.body))
                    .foregroundStyle(Theme.Palette.muted)
                phase
                if let failure = importer.failure {
                    StateLine(symbol: "triangle-alert", text: failure)
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
            Label("Asking Stremio for a sign-in code", glyph: "clock")
                .font(.text(Theme.type.body))
                .foregroundStyle(Theme.Palette.muted)
        case .waitingForLink(let code, let link, _)?:
            HStack(alignment: .top, spacing: Theme.Space.l) {
                VStack(alignment: .leading, spacing: Theme.Space.s) {
                    Text("Open the link on any device signed in to Stremio and enter this code:")
                        .font(.text(Theme.type.body))
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
                            .font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                            .foregroundStyle(Theme.Palette.text)
                        #else
                        Button { openURL(url) } label: { Label(url.host() ?? link, glyph: "external-link") }
                            .buttonStyle(PillButtonStyle(prominent: true))
                        #endif
                    }
                    Label("Waiting for the link. This page moves on by itself.", glyph: "clock")
                        .font(.text(Theme.type.callout, relativeTo: .callout))
                        .foregroundStyle(Theme.Palette.muted)
                }
                if metrics.wide { QRCode(text: link).frame(width: 180, height: 180) }
            }
        case .importing(let step)?:
            Label(step, glyph: "download")
                .font(.text(Theme.type.body))
                .foregroundStyle(Theme.Palette.text)
        case .done(let summary)?:
            VStack(alignment: .leading, spacing: 8) {
                Label("Imported", glyph: "circle-check").display(Theme.type.title2)
                Text("\(summary.addonsImported) addons, \(summary.titles) titles, \(summary.inProgress) in progress, \(summary.watched) watched.")
                ForEach(summary.addonsSkipped, id: \.self) { Label($0, glyph: "circle-minus") }
                if summary.seriesUnmatched > 0 {
                    Text("Watched marks for \(summary.seriesUnmatched) series could not be matched to episodes.")
                }
            }
            .font(.text(Theme.type.body))
            .foregroundStyle(Theme.Palette.text)
        case .expired?:
            StateLine(symbol: "clock-alert", text: "The code expired before it was linked.", action: ("New code", restart))
        case .failed(let reason)?:
            StateLine(symbol: "triangle-alert", text: reason, action: ("Try again", restart))
        }
    }

    /// A new view starts a new sign-in.
    private func restart() {
        importer = LiveRecord("import", core: core)
        attempt += 1
    }
}

/// A QR code drawn locally from text, crisp at any size.
struct QRCode: View {
    let text: String

    var body: some View {
        if let image = Self.render(text) {
            Image(decorative: image, scale: 1)
                .interpolation(.none)
                .resizable()
                .scaledToFit()
                .padding(10)
                .background(RoundedRectangle(cornerRadius: Theme.Radius.s).fill(.white))
                .accessibilityLabel("QR code")
        }
    }

    private static func render(_ text: String) -> CGImage? {
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(text.utf8)
        filter.correctionLevel = "M"
        guard let output = filter.outputImage else { return nil }
        return CIContext().createCGImage(output, from: output.extent)
    }
}
