import Foundation
import SwiftUI

/// Joins this device to a sync account: a device that already syncs scans
/// the QR, both show the same six digits, the user confirms, and the record
/// it hands over goes to the Keychain. Nothing is kept before confirming.
struct PairingScreen: View {
    let core: CoreClient
    @State private var server = UserDefaults.standard.string(forKey: "syncServer") ?? "https://"
    @State private var name = DeviceName.current
    @State private var pairing: LiveRecord<PairingSnapshot>?
    /// nil while saving the joined record, then whether the Keychain kept it.
    @State private var stored: Bool?
    @Environment(\.metrics) private var metrics
    @Environment(SyncModel.self) private var sync

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: "Pair this device")
                Text("Join your sync account by scanning a code with a device that already syncs. Your library stays end to end encrypted.")
                    .font(.text(Theme.type.body))
                    .foregroundStyle(Theme.Palette.muted)
                if let pairing {
                    phase(pairing)
                    if let failure = pairing.failure {
                        StateLine(symbol: "exclamationmark.triangle", text: failure, action: ("Try again", { self.pairing = nil }))
                    }
                } else {
                    form
                }
            }
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .task(id: pairing?.taskId) { pairing?.start() }
    }

    private var form: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            field("Sync server", text: $server, prompt: "https://sync.example.org")
            field("This device's name", text: $name, prompt: "Living room")
            Button {
                let trimmed = server.trimmingCharacters(in: .whitespaces)
                UserDefaults.standard.set(trimmed, forKey: "syncServer")
                pairing = LiveRecord("pairing", params: ["server": trimmed, "name": name], core: core)
            } label: {
                Label("Show pairing code", systemImage: "qrcode")
            }
            .buttonStyle(PillButtonStyle(prominent: true))
            .disabled(URL(string: server)?.host() == nil || name.isEmpty)
        }
    }

    private func field(_ label: String, text: Binding<String>, prompt: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(label.uppercased())
                .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
                .tracking(1.6)
                .foregroundStyle(Theme.Palette.muted)
            TextField(prompt, text: text)
                .textFieldStyle(.plain)
                .font(.text(Theme.type.body))
                .autocorrectionDisabled()
                #if os(iOS)
                .textInputAutocapitalization(.never)
                #endif
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
                .background(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous).fill(Theme.Palette.surface))
                .overlay(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous).strokeBorder(Theme.Palette.line))
        }
    }

    @ViewBuilder private func phase(_ pairing: LiveRecord<PairingSnapshot>) -> some View {
        switch pairing.snapshot {
        case nil, .starting?:
            Label("Asking the sync server for a code", systemImage: "clock")
                .font(.text(Theme.type.body))
                .foregroundStyle(Theme.Palette.muted)
        case .waiting(let qr, let expiresMs)?:
            VStack(alignment: .leading, spacing: Theme.Space.s) {
                QRCode(text: qr).frame(width: metrics.wide ? 240 : 200, height: metrics.wide ? 240 : 200)
                Text("On a device that already syncs, open Settings, then Add a device, and scan this code.")
                    .font(.text(Theme.type.body))
                    .foregroundStyle(Theme.Palette.text)
                let expires = Date(timeIntervalSince1970: TimeInterval(expiresMs) / 1000)
                Text("The code works until \(expires.formatted(date: .omitted, time: .shortened)).")
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
            }
        case .confirm(let code)?:
            VStack(alignment: .leading, spacing: Theme.Space.s) {
                Text("Does the other device show the same six digits?")
                    .font(.text(Theme.type.body))
                    .foregroundStyle(Theme.Palette.text)
                Text(code)
                    .font(.mono(Theme.type.hero + 8))
                    .tracking(8)
                    .foregroundStyle(Theme.Palette.text)
                HStack(spacing: 12) {
                    Button { pairing.command(["type": "confirm"]) } label: { Label("They match", systemImage: "checkmark") }
                        .buttonStyle(PillButtonStyle(prominent: true))
                    Button { pairing.command(["type": "reject"]) } label: { Label("They differ", systemImage: "xmark") }
                        .buttonStyle(PillButtonStyle())
                }
            }
        case .joined(let record)?:
            Group {
                switch stored {
                case true?: Label("This device joined your sync account.", systemImage: "checkmark.circle")
                case false?: Label("Joined, but the Keychain refused the account, so it was not kept. Pair again from a signed build.", systemImage: "exclamationmark.triangle")
                case nil: Label("Saving the account to this device", systemImage: "clock")
                }
            }
            .font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
            .foregroundStyle(Theme.Palette.text)
            .task(id: record) {
                stored = SyncRecord.store(record)
                if stored == true { await sync.didJoin() }
            }
        case .expired?:
            StateLine(symbol: "clock.badge.xmark", text: "The code expired before it was scanned.", action: ("New code", { self.pairing = nil }))
        case .failed(let reason)?:
            StateLine(symbol: "exclamationmark.triangle", text: reason, action: ("Try again", { self.pairing = nil }))
        }
    }
}

/// The sync account's keystore record. The core never stores it: the app
/// keeps it in the Keychain and hands it back per call.
enum SyncRecord {
    @discardableResult
    static func store(_ record: JSONValue) -> Bool {
        guard let data = try? JSONEncoder().encode(record) else { return false }
        return Keychain.write("syncRecord", data)
    }

    static func load() -> JSONValue? {
        Keychain.read("syncRecord").flatMap { try? JSONDecoder().decode(JSONValue.self, from: $0) }
    }

    static func delete() {
        Keychain.delete("syncRecord")
    }
}

/// How other devices in the sync account will see this one.
enum DeviceName {
    @MainActor static var current: String {
        #if os(macOS)
        return Host.current().localizedName ?? "Mac"
        #else
        return UIDevice.current.name
        #endif
    }
}
