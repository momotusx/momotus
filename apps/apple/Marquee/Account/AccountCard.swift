import SwiftUI

/// Home's one account card (plans/accounts.md 4.3), by priority: a scheduled
/// deletion, the account deleted, an announcement, the second-way-in nudge.
/// On every device, TVs included. Built from the app's own pieces.
struct AccountCard: View {
    /// `top`: Home's first place, above the hero, for the urgent cards (`AccountRules.urgent`, the owner
    /// 2026-11-02); the other place, under the hero, holds the rest and "Signed in from".
    var top = false
    /// Sign in, or the nudge: the Account page.
    let openAccount: () -> Void
    @Environment(SyncModel.self) private var sync
    @Environment(ProfileSession.self) private var profiles
    @Environment(AccountWipe.self) private var wipe
    @State private var busy = false
    @State private var error: String?
    /// Remove from this device with no lock to check with (a TV, no passcode): one more press.
    @State private var askAgain = false
    /// "Not you? Remove this passkey" asks once what removing it does.
    @State private var removingPasskey = false

    private var words: AccountWords { .shared }
    private var owner: Bool { profiles.snapshot?.current?.isOwner == true }
    private var kids: Bool { profiles.snapshot?.current?.kids == true }
    private var members: [MemberInfo] { sync.devices?.members ?? [] }

    private var card: AccountRules.HomeCard? {
        // Incognito reaches no account page: the core refuses every account op there.
        if profiles.snapshot?.incognito == true { return nil }
        let shown = (sync.lastReport?.announcements ?? []).filter { AccountRules.shows($0, kids: kids) }
        return AccountRules.homeCard(
            deletion: sync.joined ? sync.lastReport?.deletion : nil,
            gone: !sync.joined && sync.problem?.code == "accountGone",
            announcements: sync.joined ? shown : [],
            secondWayIn: sync.joined && AccountRules.needsSecondWayIn(members),
            owner: owner, kids: kids)
    }

    private var isDeletion: Bool {
        if case .deletion = card { true } else { false }
    }

    var body: some View {
        // Right after a pairing, "Signed in from" the device that approved it,
        // for 8 s: a line nothing focuses (plans/tv-qr-login.md 5.5).
        // A scheduled deletion still comes first.
        if top {
            if let card, AccountRules.urgent(card), sync.joinedBy == nil || isDeletion { box(card) }
        } else if let by = sync.joinedBy, !isDeletion {
            Said(tone: .ok, text: AccountRules.fill(words.tvSignedInFrom, ["device": by]))
                .task {
                    try? await Task.sleep(for: .seconds(8))
                    sync.joinedBy = nil
                }
        } else if let card, !AccountRules.urgent(card) {
            box(card)
        }
    }

    private func box(_ card: AccountRules.HomeCard) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            content(card)
            if let error { Said(tone: .bad, text: error) }
        }
        .padding(Theme.Space.s)
        .frame(maxWidth: 760, alignment: .leading)
        // A solid surface with no edge: content, not glass (design/DECISION.md, Surfaces).
        .background(Theme.Palette.surface, in: RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
    }

    @ViewBuilder private func content(_ card: AccountRules.HomeCard) -> some View {
        switch card {
        case .deletion(let deletion):
            Callout(glyph: "triangle-alert", text: AccountRules.deletionText(deletion))
            FlowRow(spacing: Theme.Space.xs) {
                Button(words.deletionCancel) { run { try await sync.cancelDeletion(unlink: false) } }
                    .buttonStyle(PillButtonStyle(prominent: true))
                if let provider = deletion.provider {
                    Button(AccountRules.deletionCancelUnlink(provider)) {
                        run { try await sync.cancelDeletion(unlink: true) }
                    }
                    .buttonStyle(PillButtonStyle())
                }
            }
            .disabled(busy)
        case .gone:
            Callout(glyph: "info", text: words.accountGone)
            if askAgain {
                Text(noLockWords)
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.text)
                    .fixedSize(horizontal: false, vertical: true)
                FlowRow(spacing: Theme.Space.xs) {
                    Button(SyncWords.cancel) { askAgain = false }
                        .buttonStyle(PillButtonStyle(prominent: true))
                    Button(words.accountGoneActions[1]) { remove(askedAgain: true) }
                        .buttonStyle(DestructiveTextStyle())
                }
                .disabled(busy)
            } else {
                FlowRow(spacing: Theme.Space.xs) {
                    Button(words.accountGoneActions[0], action: openAccount)
                        .buttonStyle(PillButtonStyle(prominent: true))
                    // The removal empties every profile's library: the owner's only (D13).
                    if owner {
                        Button(words.accountGoneActions[1]) { remove(askedAgain: false) }
                            .buttonStyle(PillButtonStyle())
                    }
                    Button(words.accountGoneActions[2]) { sync.dismissGone() }
                        .buttonStyle(PillButtonStyle())
                }
                .disabled(busy)
            }
        case .announcement(let notice):
            Callout(glyph: AccountRules.isSecurityNotice(notice.kind) ? "shield-alert" : "info", text: AccountRules.text(notice, members: members))
            if removingPasskey, let member = notice.member {
                // A passkey nobody here added: what removing it does, then the OS prompt.
                Text(words.removePasskeyAsk)
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.text)
                    .fixedSize(horizontal: false, vertical: true)
                FlowRow(spacing: Theme.Space.xs) {
                    Button(SyncWords.cancel) { removingPasskey = false }
                        .buttonStyle(PillButtonStyle(prominent: true))
                    Button(SyncWords.remove) {
                        run {
                            try await sync.removeDevice(member)
                            try await sync.ackAnnouncements([notice.id])
                            removingPasskey = false
                        }
                    }
                    .buttonStyle(DestructiveTextStyle())
                }
                .disabled(busy)
            } else {
                FlowRow(spacing: Theme.Space.xs) {
                    if AccountRules.showsOK(notice, owner: owner) {
                        Button(words.announceOk) { run { try await sync.ackAnnouncements([notice.id]) } }
                            .buttonStyle(PillButtonStyle(prominent: true))
                    }
                    if notice.kind == "passkey", owner, notice.member != nil {
                        Button(words.announceNotYou) { removingPasskey = true }
                            .buttonStyle(DestructiveTextStyle())
                    }
                }
                .disabled(busy)
            }
        case .secondWayIn:
            Button(action: openAccount) { Label(words.secondWayIn, glyph: "key-round") }
                .buttonStyle(PillButtonStyle())
        }
    }

    /// Remove from this device: the OS prompt as for a sign-out, then steps 2 and 3.
    private func remove(askedAgain: Bool) {
        let ids = profiles.snapshot?.profiles.map(\.id) ?? []
        run {
            do {
                try await wipe.removeHere(profiles: ids, disconnectDebrid: true, askedAgain: askedAgain)
            } catch let refusal as CoreError where refusal.code == "noLockAsk" || refusal.code == "unconfirmed" {
                askAgain = refusal.code == "noLockAsk"
                return
            }
            askAgain = false
            sync.dismissGone()
        }
    }

    private var noLockWords: String {
        #if os(tvOS)
        SyncWords.tvSure
        #else
        SyncWords.noLockRemove
        #endif
    }

    private func run(_ work: @escaping () async throws -> Void) {
        guard !busy else { return }
        busy = true
        error = nil
        Task {
            defer { busy = false }
            do {
                try await work()
            } catch {
                let code = (error as? CoreError)?.code ?? ""
                self.error = AccountRules.trouble(code)?.text ?? SyncProblem(error, action: .devices).words
            }
        }
    }
}
