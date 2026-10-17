import SwiftUI

/// Installed addons in their order (earlier ones answer first), with a link
/// field to install more. Addon URLs can hold API keys and are never shown.
struct AddonsScreen: View {
    let addons: LiveRecord<AddonsSnapshot>
    @State private var link = ""
    @State private var installing = false
    @State private var removing: AddonInfo?
    @Environment(\.metrics) private var metrics

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: "Addons")
                Text("Addons bring catalogs, details, streams and subtitles. Order matters: earlier addons answer first.")
                    .font(.text(Theme.type.body))
                    .foregroundStyle(Theme.Palette.muted)
                install
                if let failure = addons.failure {
                    StateLine(symbol: "exclamationmark.triangle", text: failure)
                }
                let list = addons.snapshot?.addons ?? []
                ForEach(Array(list.enumerated()), id: \.element.id) { index, addon in
                    AddonCard(
                        addon: addon, first: index == 0, last: index == list.count - 1,
                        toggle: { act("setAddonEnabled", addon, ["enabled": !addon.enabled]) },
                        move: { offset in act("moveAddon", addon, ["to": index + offset]) },
                        remove: { removing = addon })
                }
            }
            .frame(maxWidth: 940, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .confirmationDialog(
            "Remove \(removing?.name ?? "this addon")?", isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }),
            titleVisibility: .visible
        ) {
            Button("Remove", role: .destructive) {
                if let addon = removing { act("removeAddon", addon, [:]) }
            }
        } message: {
            Text("Its catalogs, details and streams go away. You can install it again from its link.")
        }
    }

    private var install: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("ADDON LINK")
                .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
                .tracking(1.6)
                .foregroundStyle(Theme.Palette.muted)
            HStack(spacing: 12) {
                TextField("https://example.org/manifest.json or stremio://", text: $link)
                    .textFieldStyle(.plain)
                    .font(.text(Theme.type.body))
                    .autocorrectionDisabled()
                    #if os(iOS)
                    .textInputAutocapitalization(.never)
                    .keyboardType(.URL)
                    #endif
                    .onSubmit(installLink)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
                    .background(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous).fill(Theme.Palette.surface))
                    .overlay(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous).strokeBorder(Theme.Palette.line))
                Button(action: installLink) {
                    Label(installing ? "Installing" : "Install", systemImage: "plus")
                }
                .buttonStyle(PillButtonStyle(prominent: true))
                .disabled(link.trimmingCharacters(in: .whitespaces).isEmpty || installing)
            }
        }
    }

    private func installLink() {
        let url = link.trimmingCharacters(in: .whitespaces)
        guard !url.isEmpty else { return }
        installing = true
        Task {
            if await addons.perform("installAddon", ["url": url]) { link = "" }
            installing = false
        }
    }

    private func act(_ op: String, _ addon: AddonInfo, _ fields: [String: any Sendable]) {
        var all = fields
        all["url"] = addon.url
        Task { await addons.perform(op, all) }
    }
}

/// One addon: logo, name and version, host, description, what it provides, actions.
private struct AddonCard: View {
    let addon: AddonInfo
    let first: Bool
    let last: Bool
    let toggle: () -> Void
    let move: (Int) -> Void
    let remove: () -> Void
    @Environment(\.metrics) private var metrics

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top, spacing: Theme.Space.s) {
                logo
                VStack(alignment: .leading, spacing: 4) {
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text(addon.name).font(.text(Theme.type.headline + 1, weight: .semibold, relativeTo: .headline))
                        Text(addon.version).font(.text(Theme.type.caption + 1, relativeTo: .caption)).foregroundStyle(Theme.Palette.muted)
                    }
                    Text(addon.host).font(.mono(Theme.type.caption + 1)).foregroundStyle(Theme.Palette.muted)
                    if let description = addon.description {
                        Text(description)
                            .font(.text(Theme.type.callout, relativeTo: .callout))
                            .foregroundStyle(Theme.Palette.muted)
                            .lineLimit(2)
                    }
                    Text(provides)
                        .font(.text(Theme.type.callout, weight: .medium, relativeTo: .callout))
                        .foregroundStyle(Theme.Palette.text)
                }
                .opacity(addon.enabled ? 1 : 0.55)
                Spacer(minLength: 0)
            }
            FlowRow(spacing: 8) {
                Button(action: toggle) {
                    Label(addon.enabled ? "On" : "Off", systemImage: "power")
                }
                .accessibilityLabel(addon.enabled ? "Turn \(addon.name) off" : "Turn \(addon.name) on")
                Button { move(-1) } label: { Label("Up", systemImage: "arrow.up") }.disabled(first)
                Button { move(1) } label: { Label("Down", systemImage: "arrow.down") }.disabled(last)
                Button(action: remove) { Label("Remove", systemImage: "trash") }
            }
            .buttonStyle(PillButtonStyle())
            .controlSize(.small)
        }
        .foregroundStyle(Theme.Palette.text)
        .padding(Theme.Space.s)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous).fill(Theme.Palette.surface))
    }

    private var logo: some View {
        RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous)
            .fill(Theme.Palette.raised)
            .overlay {
                if addon.logo != nil {
                    ArtImage(url: addon.logo, mode: .fit, maxPixels: 160).padding(6)
                } else {
                    Text(addon.name.prefix(1).uppercased()).font(.display(Theme.type.title3))
                }
            }
            .frame(width: 48, height: 48)
            .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous))
            .accessibilityHidden(true)
    }

    /// "Catalogs, Details for movie, series"
    private var provides: String {
        let words = addon.provides.map { kind in
            switch kind {
            case "catalog": "Catalogs"
            case "meta": "Details"
            case "stream": "Streams"
            case "subtitles": "Subtitles"
            case "addon_catalog": "Addon lists"
            default: kind.capitalized
            }
        }
        let types = addon.types.isEmpty ? "" : " for \(addon.types.joined(separator: ", "))"
        return words.joined(separator: ", ") + types
    }
}
