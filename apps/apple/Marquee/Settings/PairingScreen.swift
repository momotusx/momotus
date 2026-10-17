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
                        StateLine(symbol: "triangle-alert", text: failure, action: ("Try again", { self.pairing = nil }))
                    }
                } else if Self.previewJoined {
                    JoinedState(core: core, stored: true)
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
        .topScrim()
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
                Label("Show pairing code", glyph: "qr-code")
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
                .overlay(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous).strokeBorder(Theme.Palette.control))
        }
    }

    @ViewBuilder private func phase(_ pairing: LiveRecord<PairingSnapshot>) -> some View {
        switch pairing.snapshot {
        case nil, .starting?:
            Label("Asking the sync server for a code", glyph: "clock")
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
                    Button { pairing.command(["type": "confirm"]) } label: { Label("They match", glyph: "check") }
                        .buttonStyle(PillButtonStyle(prominent: true))
                    Button { pairing.command(["type": "reject"]) } label: { Label("They differ", glyph: "x") }
                        .buttonStyle(PillButtonStyle())
                }
            }
        case .joined(let record)?:
            JoinedState(core: core, stored: stored)
                .task(id: record) {
                    stored = SyncRecord.store(record)
                    if stored == true { await sync.didJoin() }
                }
        case .expired?:
            StateLine(symbol: "clock-alert", text: "The code expired before it was scanned.", action: ("New code", { self.pairing = nil }))
        case .failed(let reason)?:
            StateLine(symbol: "triangle-alert", text: reason, action: ("Try again", { self.pairing = nil }))
        }
    }

    #if DEBUG
    private static var previewJoined: Bool { DebugHooks.pairedPreview }
    #else
    private static let previewJoined = false
    #endif
}

/// After joining: the first sync, then what this device holds now (read from
/// the library and addons views, since the pairing result has no counts),
/// and Done back to Settings.
private struct JoinedState: View {
    /// nil while saving the joined record, then whether the Keychain kept it.
    let stored: Bool?
    @State private var library: LiveRecord<LibrarySnapshot>
    @State private var addons: LiveRecord<AddonsSnapshot>
    @Environment(SyncModel.self) private var sync
    @Environment(\.metrics) private var metrics
    @Environment(\.dismiss) private var dismiss

    init(core: CoreClient, stored: Bool?) {
        self.stored = stored
        _library = State(initialValue: LiveRecord("library", core: core))
        _addons = State(initialValue: LiveRecord("addons", core: core))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            Group {
                switch stored {
                case true?: Label("This device joined your sync account.", glyph: "circle-check")
                case false?: Label("Joined, but the Keychain refused the account, so it was not kept. Pair again from a signed build.", glyph: "triangle-alert")
                case nil: Label("Saving the account to this device", glyph: "clock")
                }
            }
            .font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
            .foregroundStyle(Theme.Palette.text)
            if stored == true { arrived }
            if stored != nil {
                Button("Done") { dismiss() }
                    .buttonStyle(PillButtonStyle(prominent: true))
            }
        }
        .task(id: library.taskId) { library.start() }
        .task(id: addons.taskId) { addons.start() }
    }

    @ViewBuilder private var arrived: some View {
        if sync.syncing {
            Label("Bringing over your library", glyph: "clock")
                .font(.text(Theme.type.body))
                .foregroundStyle(Theme.Palette.muted)
        } else {
            if let problem = sync.problem {
                StateLine(symbol: "triangle-alert", text: problem.words)
            }
            if let counts {
                VStack(alignment: .leading, spacing: 6) {
                    Text("NOW ON THIS DEVICE")
                        .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
                        .tracking(1.6)
                        .foregroundStyle(Theme.Palette.muted)
                    table(counts)
                }
            }
        }
    }

    /// Titles by type, then addons: what the views hold, never made up.
    private var counts: [(key: String, value: String)]? {
        guard let library = library.snapshot, let addons = addons.snapshot else { return nil }
        var rows = library.kinds.map { (key: $0.label, value: "\($0.count)") }
        if rows.isEmpty { rows.append((key: "Library", value: "Empty")) }
        rows.append((key: "Addons", value: "\(addons.addons.count)"))
        return rows
    }

    /// The title page's facts table: keys dim on the left, values on the right.
    private func table(_ rows: [(key: String, value: String)]) -> some View {
        VStack(spacing: 0) {
            ForEach(Array(rows.enumerated()), id: \.offset) { index, row in
                if index > 0 { Rectangle().fill(Theme.Palette.line).frame(height: 1) }
                HStack(alignment: .firstTextBaseline, spacing: Theme.Space.s) {
                    Text(row.key).foregroundStyle(Theme.Palette.dim)
                    Spacer(minLength: 0)
                    Text(row.value).foregroundStyle(Theme.Palette.text).monospacedDigit()
                }
                .padding(.horizontal, metrics.wide ? 18 : 14)
                .padding(.vertical, metrics.wide ? 12 : 11)
                .accessibilityElement(children: .combine)
            }
        }
        .font(.text(metrics.cardName, weight: .semibold, relativeTo: .footnote))
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
        .frame(maxWidth: 420)
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
