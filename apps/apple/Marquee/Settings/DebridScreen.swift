import Foundation
import SwiftUI

/// Debrid accounts by service, as records (plans/c10-handoff.md, phase 3a):
/// sign in with a code (Real-Debrid, TorBox, AllDebrid), or with a key; the
/// records live in the Keychain behind the debrid helper, which stores what
/// the core hands over and disconnects with a tombstone first.
struct DebridScreen: View {
    let core: CoreClient
    @Environment(DebridSession.self) private var debrid
    @Environment(\.metrics) private var metrics
    @Environment(\.openURL) private var openURL
    /// The sign-in by code under way, and its service.
    /// `before` is the account's generation when it started: done only counts once that moved on.
    @State private var login: (service: String, view: LiveRecord<DebridLoginSnapshot>, before: String?)?
    /// Cards with the key field open (Premiumize always has it).
    @State private var keyFields: Set<String> = []
    @State private var drafts: [String: String] = [:]
    @State private var said: (service: String, text: String)?
    @State private var busy: String?
    @FocusState private var focus: DebridScreenFocus?

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: "Debrid services")
                Text("A debrid account plays torrents as fast direct links. Accounts stay in this device's Keychain and are never shown again or synced.")
                    .font(.text(Theme.type.body))
                    .foregroundStyle(Theme.Palette.muted)
                ForEach(DebridWords.services) { service in
                    card(service)
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
        .task(id: login?.view.taskId) { login?.view.start() }
        // A sign-in closes once its own record is the account's and stored; the card then says Connected.
        .onChange(of: loginFinished) { _, finished in
            if finished, let service = login?.service { closeLogin(service) }
        }
    }

    private var snapshot: DebridSnapshot? { debrid.view.snapshot }

    private var loginFinished: Bool {
        guard let login, case .done? = login.view.snapshot else { return false }
        let account = snapshot?.accounts.first { $0.service == login.service }
        let pending = snapshot?.pending.contains { $0.service == login.service } ?? false
        return DebridWords.signedIn(done: true, before: login.before, now: account?.generation, pending: pending)
    }

    private func card(_ service: DebridWords.Service) -> some View {
        let account = snapshot?.accounts.first { $0.service == service.id }
        let pending = snapshot?.pending.contains { $0.service == service.id } ?? false
        return VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline) {
                Text(service.name).font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                Spacer()
                let state = DebridWords.state(account, pending: pending, stored: !debrid.health.unstored.contains(service.id))
                Text(state ?? "Not connected")
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(state == "Connected" ? Theme.Palette.ok : Theme.Palette.muted)
            }
            if let login, login.service == service.id {
                LoginPanel(phase: login.view.snapshot, failure: login.view.failure,
                           stored: !debrid.health.unstored.contains(service.id),
                           again: { begin(service.id) }, close: { closeLogin(service.id) }, focus: $focus)
            } else {
                FlowRow(spacing: 12) {
                    if DebridWords.codeServices.contains(service.id) {
                        Button { begin(service.id) } label: { Label("Sign in with a code", glyph: "qr-code") }
                            .buttonStyle(PillButtonStyle(prominent: account == nil))
                            .focused($focus, equals: .signIn(service.id))
                            .accessibilityLabel("Sign in to \(service.name) with a code")
                        // A connected account takes a new key without a disconnect, and one
                        // that asks for a new sign-in can take a key instead.
                        if !keyFields.contains(service.id) {
                            Button("Use a key instead") { keyFields.insert(service.id) }
                                .buttonStyle(PillButtonStyle())
                        }
                    } else if account != nil && account?.state != "reconnect" && !keyFields.contains(service.id) {
                        Button("Replace the key") { keyFields.insert(service.id) }
                            .buttonStyle(PillButtonStyle())
                            .accessibilityLabel("Replace the \(service.name) key")
                    }
                    if let account {
                        Button("Disconnect") { disconnect(account, service) }
                            .buttonStyle(PillButtonStyle())
                            .disabled(busy == service.id)
                            .accessibilityLabel("Disconnect \(service.name)")
                    }
                }
                // Premiumize signs in with a key only: its field shows while it is not connected.
                if keyFields.contains(service.id)
                    || (!DebridWords.codeServices.contains(service.id) && (account == nil || account?.state == "reconnect")) {
                    keyField(service)
                }
            }
            if let said, said.service == service.id {
                Label(said.text, glyph: "triangle-alert")
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.text)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .foregroundStyle(Theme.Palette.text)
        .padding(Theme.Space.s)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
    }

    private func keyField(_ service: DebridWords.Service) -> some View {
        let draft = (drafts[service.id] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        return VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 10) {
                SecureField("API key", text: Binding(get: { drafts[service.id] ?? "" }, set: { drafts[service.id] = $0 }))
                    .textFieldStyle(.plain)
                    .font(.mono(Theme.type.body))
                    .padding(.horizontal, 14)
                    .padding(.vertical, 10)
                    .focused($focus, equals: .key(service.id))
                    .fieldChrome(focused: focus == .key(service.id))
                // Save is the primary only once its field holds a key.
                Button("Save") { save(service, draft) }
                    .buttonStyle(PillButtonStyle(prominent: !draft.isEmpty))
                    .disabled(draft.isEmpty || busy == service.id)
                    .accessibilityLabel("Save \(service.name) key")
            }
            #if !os(tvOS)
            if let url = URL(string: service.keyPage) {
                Button("Get a key") { openURL(url) }
                    .buttonStyle(.plain)
                    .font(.text(Theme.type.callout, weight: .medium, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
                    .accessibilityLabel("Get \("AEIOU".contains(service.name.prefix(1)) ? "an" : "a") \(service.name) key")
            }
            #else
            Text("Get a key at \(DebridWords.address(service.keyPage))")
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.muted)
            #endif
        }
    }

    /// Opens a sign-in by code; a second one for the service starts over.
    private func begin(_ service: String) {
        said = nil
        let view = LiveRecord<DebridLoginSnapshot>("debridLogin", params: ["service": service], core: core)
        view.start()
        login = (service, view, snapshot?.accounts.first { $0.service == service }?.generation)
        // Into the sign-in once it is on screen.
        Task { focus = .panel }
    }

    /// Closing the panel closes the view; a record already collected still arrives and is stored.
    private func closeLogin(_ service: String) {
        login = nil
        Task { focus = .signIn(service) }
    }

    private func save(_ service: DebridWords.Service, _ key: String) {
        busy = service.id
        said = nil
        Task {
            defer { busy = nil }
            do {
                // The record arrives through the debrid view; the card says Saving, then Connected.
                try await debrid.setKey(service.id, key)
                drafts[service.id] = nil
                keyFields.remove(service.id)
            } catch is CancellationError {
            } catch {
                said = (service.id, error.localizedDescription)
            }
        }
    }

    private func disconnect(_ account: DebridAccount, _ service: DebridWords.Service) {
        busy = service.id
        said = nil
        Task {
            defer { busy = nil }
            do {
                try await debrid.disconnect(account)
            } catch {
                said = (service.id, DebridWords.notSaved)
            }
        }
    }
}

/// Where focus goes on the Debrid page: a card's sign-in button, or the open sign-in.
enum DebridScreenFocus: Hashable { case signIn(String), panel, key(String) }

/// A sign-in by code: the code large, the address, its QR, the time left;
/// then why not, with a new code.
private struct LoginPanel: View {
    let phase: DebridLoginSnapshot?
    let failure: String?
    /// The Keychain took the service's last record write.
    let stored: Bool
    let again: () -> Void
    let close: () -> Void
    var focus: FocusState<DebridScreenFocus?>.Binding
    @Environment(\.openURL) private var openURL

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            switch phase {
            case nil, .starting?:
                if let failure {
                    Text(failure).font(.text(Theme.type.body))
                    buttons(again: true)
                } else {
                    Label(DebridWords.login(.starting), glyph: "clock").font(.text(Theme.type.body))
                    buttons(again: false)
                }
            case let .code(userCode, url, qr, expiresMs)?:
                HStack(alignment: .top, spacing: Theme.Space.s) {
                    QRCode(text: qr).frame(width: 160, height: 160).accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 8) {
                        Text("On a phone or computer, go to").font(.text(Theme.type.callout, relativeTo: .callout)).foregroundStyle(Theme.Palette.muted)
                        Text(DebridWords.address(url)).font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                        Text("and enter").font(.text(Theme.type.callout, relativeTo: .callout)).foregroundStyle(Theme.Palette.muted)
                        Text(userCode)
                            .font(.mono(Theme.type.title1))
                            .tracking(2)
                            .accessibilityLabel("Code \(userCode.map(String.init).joined(separator: " "))")
                        // The time left, from the code's own deadline: a new code starts its own clock.
                        TimelineView(.periodic(from: .now, by: 1)) { context in
                            Text(DebridWords.goodFor(expiresMs - Int64(context.date.timeIntervalSince1970 * 1000)))
                                .font(.text(Theme.type.callout, relativeTo: .callout).monospacedDigit())
                                .foregroundStyle(Theme.Palette.muted)
                        }
                    }
                }
                #if os(iOS)
                if let page = URL(string: url) {
                    Button { openURL(page) } label: { Label("Open the page", glyph: "external-link") }
                        .buttonStyle(PillButtonStyle(prominent: true))
                }
                #endif
                buttons(again: false)
            case .done?:
                // Through on the service's side; the panel closes once the record is stored.
                Label(DebridWords.doneWords(stored: stored), glyph: stored ? "clock" : "triangle-alert").font(.text(Theme.type.body))
            case let other?:
                Text(DebridWords.login(other)).font(.text(Theme.type.body)).fixedSize(horizontal: false, vertical: true)
                buttons(again: true)
            }
        }
        .padding(Theme.Space.s)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous).fill(Theme.Palette.surface))
        #if os(tvOS)
        .focusSection()
        #endif
    }

    @ViewBuilder private func buttons(again showAgain: Bool) -> some View {
        HStack(spacing: 12) {
            if showAgain {
                Button("Get a new code", action: again)
                    .buttonStyle(PillButtonStyle(prominent: true))
                    .focused(focus, equals: .panel)
            }
            if showAgain {
                Button("Close", action: close).buttonStyle(PillButtonStyle())
            } else {
                Button("Close", action: close)
                    .buttonStyle(PillButtonStyle())
                    .focused(focus, equals: .panel)
            }
        }
    }
}
