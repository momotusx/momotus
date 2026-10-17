import Foundation
import SwiftUI

/// Debrid accounts by service. Keys live in the Keychain (this device only)
/// and are handed to the core with `setDebridKeys`; the core never stores them.
enum DebridKeys {
    struct Service: Identifiable, Sendable {
        let id: String
        let name: String
        let keyPage: String
    }

    static let services = [
        Service(id: "realdebrid", name: "Real-Debrid", keyPage: "https://real-debrid.com/apitoken"),
        Service(id: "torbox", name: "TorBox", keyPage: "https://torbox.app/settings"),
        Service(id: "alldebrid", name: "AllDebrid", keyPage: "https://alldebrid.com/apikeys"),
        Service(id: "premiumize", name: "Premiumize", keyPage: "https://www.premiumize.me/account"),
    ]

    static func key(_ service: String) -> String? {
        Keychain.read("debrid.\(service)").map { String(decoding: $0, as: UTF8.self) }
    }

    /// Stores or removes a key; false when the Keychain refused it.
    @discardableResult
    static func set(_ service: String, _ key: String?) -> Bool {
        guard let key, !key.isEmpty else {
            return Keychain.delete("debrid.\(service)")
        }
        return Keychain.write("debrid.\(service)", Data(key.utf8))
    }

    /// Hands every stored key to the core, in the services' order; the first is the default.
    /// Returns how many services the core now uses.
    @MainActor @discardableResult
    static func handOver(to core: CoreClient) async throws -> Int {
        let keys = services.compactMap { service in key(service.id).map { [service.id, $0] } }
        struct Stores: Decodable { let stores: Int }
        return try await core.request("setDebridKeys", ["keys": keys]) {
            struct Envelope: Decodable { let result: Stores }
            return try JSONDecoder().decode(Envelope.self, from: $0).result.stores
        }
    }
}

/// One key field per service; saving hands the set to the core at once.
struct DebridScreen: View {
    let core: CoreClient
    @State private var drafts: [String: String] = [:]
    @State private var saved: [String: Bool] = [:]
    /// What the last change did, under its service's row; `ok` when it went through, here and in the core.
    @State private var said: (service: String, text: String, ok: Bool)?
    /// The key field being typed in.
    @FocusState private var typing: String?
    @Environment(\.metrics) private var metrics
    @Environment(\.openURL) private var openURL

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: "Debrid services")
                Text("A debrid account plays torrents as fast direct links. Keys stay in this device's Keychain and are never shown again or synced.")
                    .font(.text(Theme.type.body))
                    .foregroundStyle(Theme.Palette.muted)
                ForEach(DebridKeys.services) { service in
                    row(service)
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
        .onAppear {
            for service in DebridKeys.services { saved[service.id] = DebridKeys.key(service.id) != nil }
        }
    }

    private func row(_ service: DebridKeys.Service) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(service.name).font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                if saved[service.id] == true {
                    Label("Key saved", glyph: "lock").font(.text(Theme.type.callout, relativeTo: .callout)).foregroundStyle(Theme.Palette.ok)
                }
                Spacer()
                if let url = URL(string: service.keyPage) {
                    Button("Get a key") { openURL(url) }
                        .buttonStyle(.plain)
                        .font(.text(Theme.type.callout, weight: .medium, relativeTo: .callout))
                        .foregroundStyle(Theme.Palette.muted)
                        // Spoken with the service, so a list of controls says whose key it is.
                        .accessibilityLabel("Get \("AEIOU".contains(service.name.prefix(1)) ? "an" : "a") \(service.name) key")
                }
            }
            HStack(spacing: 10) {
                SecureField(saved[service.id] == true ? "Replace the saved key" : "API key", text: Binding(
                    get: { drafts[service.id] ?? "" }, set: { drafts[service.id] = $0 }))
                    .focused($typing, equals: service.id)
                    .textFieldStyle(.plain)
                    .font(.mono(Theme.type.body))
                    .padding(.horizontal, 14)
                    .padding(.vertical, 10)
                    .background(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous).fill(Theme.Palette.surface))
                    .overlay(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous).strokeBorder(Theme.Palette.control))
                // Save is the primary only once its field holds a key.
                let draft = (drafts[service.id] ?? "").trimmingCharacters(in: .whitespaces)
                Button("Save") { apply(service.id, drafts[service.id]) }
                    .buttonStyle(PillButtonStyle(prominent: !draft.isEmpty))
                    .disabled(draft.isEmpty)
                    .accessibilityLabel("Save \(service.name) key")
                if saved[service.id] == true {
                    Button("Remove") { apply(service.id, nil) }
                        .buttonStyle(PillButtonStyle())
                        .accessibilityLabel("Remove \(service.name) key")
                }
            }
            if let said, said.service == service.id {
                Label(said.text, glyph: said.ok ? "circle-check" : "triangle-alert")
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(said.ok ? Theme.Palette.muted : Theme.Palette.text)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .foregroundStyle(Theme.Palette.text)
        .padding(Theme.Space.s)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
    }

    private func apply(_ service: String, _ key: String?) {
        let trimmed = key?.trimmingCharacters(in: .whitespacesAndNewlines)
        let removing = trimmed?.isEmpty ?? true
        // The keyboard goes, so the result under the row is in view.
        typing = nil
        // A refusal changes nothing: the core keeps the keys it has, so nothing is handed over.
        guard DebridKeys.set(service, trimmed) else {
            said = (service, DebridWords.refused(removing: removing), false)
            return
        }
        drafts[service] = nil
        saved[service] = !removing
        Task {
            do {
                let count = try await DebridKeys.handOver(to: core)
                said = (service, DebridWords.outcome(removed: removing, inUse: count), true)
            } catch {
                said = (service, DebridWords.outcome(removed: removing, inUse: nil, refused: error.localizedDescription), false)
            }
        }
    }
}
