import SwiftUI

/// The Account page's sign-in pieces (plans/accounts.md 4.3): the identities
/// it signs in with (Link another, Unlink on the owner only), Save a passkey
/// here, and Apple's sheet as the re-authentication for an unlink or a delete.
/// Built from the app's own pieces; the design pick restyles them.
enum AccountServerOf {
    /// The relay this device joined: the one its sync used, else the build's.
    @MainActor static var joined: String? {
        UserDefaults.standard.string(forKey: SyncRecord.serverKey) ?? AccountServer.default
    }
}

/// Signed in with: each identity, Link another and Unlink (owner, D16).
struct SignedInWith: View {
    @Environment(SyncModel.self) private var sync
    @Environment(ProfileSession.self) private var profiles
    @State private var links: [IdentityLink]?
    @State private var providers: [String] = []
    @State private var error: String?
    @State private var busy = false
    /// An unlink whose Apple step failed: Unlink anyway, without a receipt.
    @State private var anyway: IdentityLink?
    /// A link refused as linked elsewhere: Use it for this account instead, with its ticket.
    @State private var elsewhere: (ticket: String, provider: String)?

    private var words: AccountWords { .shared }
    private var owner: Bool { SyncRules.mayDeleteAccount(profiles.snapshot) }

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Text(words.signedInWith).display(Theme.type.title2).foregroundStyle(Theme.Palette.text).accessibilityAddTraits(.isHeader)
            if let error { Said(tone: .bad, text: error) }
            ForEach(links ?? []) { link in row(link) }
            #if !os(tvOS)
            if owner, !unlinked.isEmpty {
                Text(words.linkAnother)
                    .font(.text(Theme.type.callout, weight: .semibold, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
                FlowRow(spacing: Theme.Space.xs) {
                    ForEach(unlinked, id: \.self) { id in
                        Button { Task { await link(id) } } label: {
                            Label(AccountRules.fill(words.continueWith, ["provider": AccountRules.providerName(id)]), glyph: "link-2")
                        }
                        .buttonStyle(PillButtonStyle())
                        .disabled(busy)
                    }
                }
            }
            if let elsewhere {
                FlowRow(spacing: Theme.Space.xs) {
                    Button(words.linkedElsewhereActions[0]) { Task { await move(elsewhere.ticket) } }
                        .buttonStyle(PillButtonStyle(prominent: true))
                    Button(words.linkedElsewhereActions[1]) { self.elsewhere = nil; error = nil }
                        .buttonStyle(PillButtonStyle())
                }
            }
            #endif
        }
        .task { await load() }
        #if !os(tvOS)
        // A sign-in another account holds, met after a join by QR: offered here.
        .onChange(of: sync.heldLink, initial: true) { takeHeld() }
        #endif
    }

    /// Providers the relay offers that this account does not sign in with yet.
    private var unlinked: [String] {
        let linked = Set((links ?? []).map(\.provider))
        return providers.filter { !linked.contains($0) }
    }

    private func row(_ link: IdentityLink) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            HStack(alignment: .top, spacing: Theme.Space.s) {
                Glyph("link-2").frame(width: 24)
                VStack(alignment: .leading, spacing: 2) {
                    Text(AccountRules.providerAccount(link.provider))
                        .font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                    Text(AccountRules.fill(words.linkedOn, ["date": AccountRules.date(link.createdMs)]))
                        .font(.text(Theme.type.callout, relativeTo: .callout))
                        .foregroundStyle(Theme.Palette.muted)
                }
                .accessibilityElement(children: .combine)
            }
            if owner {
                // The only sign-in left: what signing in is without it (4.3).
                if links?.count == 1 {
                    Text(words.unlinkLast)
                        .font(.text(Theme.type.callout, relativeTo: .callout))
                        .foregroundStyle(Theme.Palette.muted)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(.leading, 24 + Theme.Space.s)
                }
                FlowRow(spacing: Theme.Space.xs) {
                    Button { Task { await unlink(link) } } label: { Label(words.unlink, glyph: "x") }
                        .buttonStyle(PillButtonStyle())
                        .disabled(busy)
                    if anyway == link {
                        Button { Task { await unlink(link, receipt: nil) } } label: { Label(words.unlinkAnyway, glyph: "x") }
                            .buttonStyle(DestructiveTextStyle())
                            .disabled(busy)
                    }
                }
                .padding(.leading, 24 + Theme.Space.s)
            }
        }
        .foregroundStyle(Theme.Palette.text)
        .padding(Theme.Space.s)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
    }

    private func load() async {
        do {
            links = try await sync.identities()
            if let server = AccountServerOf.joined { providers = AccountRules.orderedProviders((try? await sync.relayInfo(server: server)) ?? []) }
        } catch {
            self.error = SyncProblem(error, action: .devices).words
        }
    }

    /// Unlink: an Apple link runs Apple's sheet first (its receipt; the sheet
    /// is the re-authentication); when that cannot run, Unlink anyway.
    private func unlink(_ link: IdentityLink) async {
        #if !os(tvOS)
        if link.provider == "apple" {
            do {
                let receipt = try await AppleReauth.receipt(sync: sync)
                await unlink(link, receipt: receipt)
            } catch {
                if case SignInSheets.Failure.cancelled = error { return }
                anyway = link
            }
            return
        }
        #endif
        await unlink(link, receipt: nil)
    }

    private func unlink(_ link: IdentityLink, receipt: String?) async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        error = nil
        do {
            try await sync.unlinkIdentity(link: link.link, receipt: receipt)
            anyway = nil
            await load()
        } catch {
            self.error = AccountRules.trouble((error as? CoreError)?.code ?? "")?.text ?? SyncProblem(error, action: .devices).words
        }
    }

    #if !os(tvOS)
    /// Link another: the provider's sheet or page, then its ticket goes to this account.
    private func link(_ provider: String) async {
        guard !busy, let server = AccountServerOf.joined else { return }
        busy = true
        defer { busy = false }
        error = nil
        do {
            let ticket = try await AccountTickets.ticket(provider: provider, server: server, sync: sync)
            do {
                try await sync.linkIdentity(ticket: ticket)
                await load()
            } catch let refusal as CoreError where refusal.code == "linkedElsewhere" {
                elsewhere = (ticket, provider)
                error = AccountRules.trouble("linkedElsewhere", provider: provider)?.text
            }
        } catch {
            if case SignInSheets.Failure.cancelled = error { return }
            self.error = AccountRules.trouble((error as? CoreError)?.code ?? "", provider: provider)?.text ?? error.localizedDescription
        }
    }

    /// The held sign-in (linkedElsewhere after a join by QR), on the owner: its words and
    /// "Use it for this account instead".
    private func takeHeld() {
        guard owner, let held = sync.heldLink else { return }
        sync.heldLink = nil
        elsewhere = (held.ticket, held.provider)
        error = AccountRules.trouble("linkedElsewhere", provider: held.provider)?.text
    }

    private func move(_ ticket: String) async {
        busy = true
        defer { busy = false }
        do {
            try await sync.linkIdentity(ticket: ticket, move: true)
            elsewhere = nil
            error = nil
            await load()
        } catch {
            self.error = AccountRules.trouble((error as? CoreError)?.code ?? "")?.text ?? error.localizedDescription
        }
    }
    #endif
}

#if !os(tvOS)
/// A sign-in's ticket for linking: Apple's own sheet, or the relay's page.
enum AccountTickets {
    @MainActor
    static func ticket(provider: String, server: String, sync: SyncModel) async throws -> String {
        let answer: SignInSnapshot
        if provider == "apple" {
            let start = try await sync.nativeSignInStart(server: server, provider: "apple")
            let token = try await SignInSheets.appleID(nonce: start.nonce)
            answer = try await sync.nativeSignIn(flow: start.flow, idToken: token)
        } else {
            let view = sync.signInView(server: server, provider: provider)
            view.start()
            answer = try await brokered(view, sync: sync)
        }
        switch answer {
        case .found(let ticket, _), .new(let ticket), .receipt(_, let ticket, _): return ticket
        case .failed(let code): throw CoreError(code, code: code)
        default: throw CoreError("unreachable", code: "unreachable")
        }
    }

    /// The relay's page in the browser session, its return to the view, its answer.
    @MainActor
    static func brokered(_ view: LiveRecord<SignInSnapshot>, sync: SyncModel) async throws -> SignInSnapshot {
        guard case .browser(let url, _) = await settle(view, { if case .starting = $0 { false } else { true } }) else {
            return view.snapshot ?? .failed(code: "unreachable")
        }
        guard let address = URL(string: url) else { throw CoreError("not a link", code: "unreachable") }
        let fragment = try await SignInSheets.browser(address)
        try await sync.returned(view, fragment: fragment)
        return await settle(view) { switch $0 { case .found, .new, .failed, .receipt: true; default: false } }
    }

    /// The view's next snapshot that matches; a minute with none is `unreachable`.
    @MainActor
    static func settle(_ view: LiveRecord<SignInSnapshot>, _ matches: (SignInSnapshot) -> Bool) async -> SignInSnapshot {
        for _ in 0..<600 {
            if let now = view.snapshot, matches(now) { return now }
            if view.failure != nil { break }
            try? await Task.sleep(for: .milliseconds(100))
        }
        return .failed(code: "unreachable")
    }
}

/// Apple's sheet as the re-authentication for an unlink or a delete: the
/// relay revokes the Apple tokens and hands back a receipt for that link.
enum AppleReauth {
    @MainActor
    static func receipt(sync: SyncModel) async throws -> String {
        guard let server = AccountServerOf.joined else { throw CoreError("no sync server", code: "unreachable") }
        let start = try await sync.nativeSignInStart(server: server, provider: "apple", revoke: true)
        let apple = try await SignInSheets.appleForRevoke(nonce: start.nonce)
        guard case .receipt(let receipt, _, _) = try await sync.nativeSignIn(flow: start.flow, idToken: apple.idToken, authorizationCode: apple.authorizationCode) else {
            throw CoreError("no receipt", code: "receiptRefused")
        }
        return receipt
    }
}

/// Save a passkey here (owner, D17; 3.1): a passkey already here is tried
/// first ("Your passkey already works on this device"); else `create()`.
struct SavePasskeyRow: View {
    @Environment(SyncModel.self) private var sync
    @Environment(ProfileSession.self) private var profiles
    @State private var said: (tone: StatusDot.Tone, text: String)?
    @State private var busy = false

    var body: some View {
        if PasskeyContract.relyingParty != nil {
            VStack(alignment: .leading, spacing: Theme.Space.xs) {
                Button { Task { await save() } } label: {
                    SettingsRow(symbol: "key-round", title: AccountWords.shared.savePasskeyHere, detail: "")
                }
                .buttonStyle(.row)
                .disabled(busy)
                if let said { Said(tone: said.tone, text: said.text) }
            }
        }
    }

    private func save() async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        said = nil
        do {
            let owner = profiles.snapshot?.profiles.first(where: \.isOwner)?.name ?? DeviceName.current
            if try await PasskeySave.here(sync: sync, userName: "\(owner) (Marquee)") == .alreadyHere {
                said = (.ok, AccountWords.shared.kitHere)
            }
        } catch {
            if case SignInSheets.Failure.cancelled = error { return }
            let code = (error as? CoreError)?.code ?? ""
            said = (.bad, AccountRules.trouble(code)?.text ?? AccountWords.shared.noSecondWayIn)
        }
    }
}
#endif
