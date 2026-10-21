import SwiftUI

/// Home's one account card (plans/accounts.md 4.3), by priority: a scheduled
/// deletion, the account deleted, an announcement, the second-way-in nudge.
/// On every device, TVs included. Built from the app's own pieces.
struct AccountCard: View {
    /// Sign in, or the nudge: the Account page.
    let openAccount: () -> Void
    @Environment(SyncModel.self) private var sync
    @Environment(ProfileSession.self) private var profiles
    @Environment(AccountWipe.self) private var wipe
    @State private var busy = false
    @State private var error: String?
    /// Remove from this device with no lock to check with (a TV, no passcode): one more press.
    @State private var askAgain = false

    private var words: AccountWords { .shared }
    private var owner: Bool { profiles.snapshot?.current?.isOwner == true }
    private var kids: Bool { profiles.snapshot?.current?.kids == true }
    private var members: [MemberInfo] { sync.devices?.members ?? [] }

    private var card: AccountRules.HomeCard? {
        let shown = (sync.lastReport?.announcements ?? []).filter { AccountRules.shows($0, kids: kids) }
        return AccountRules.homeCard(
            deletion: sync.joined ? sync.lastReport?.deletion : nil,
            gone: !sync.joined && sync.problem?.code == "accountGone",
            announcements: sync.joined ? shown : [],
            secondWayIn: sync.joined && AccountRules.needsSecondWayIn(members),
            owner: owner, kids: kids)
    }

    var body: some View {
        if let card {
            VStack(alignment: .leading, spacing: Theme.Space.xs) {
                content(card)
                if let error { Said(tone: .bad, text: error) }
            }
            .padding(Theme.Space.s)
            .frame(maxWidth: 760, alignment: .leading)
            .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
        }
    }

    @ViewBuilder private func content(_ card: AccountRules.HomeCard) -> some View {
        switch card {
        case .deletion(let deletion):
            Callout(glyph: "triangle-alert", text: AccountRules.deletionText(deletion))
            FlowRow(spacing: Theme.Space.xs) {
                Button(words.deletionCancel) { run { try await sync.cancelDeletion(unlink: false) } }
                    .buttonStyle(PillButtonStyle(prominent: true))
                if let provider = deletion.provider {
                    Button(AccountRules.fill(words.deletionCancelUnlink, ["provider": AccountRules.providerName(provider)])) {
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
                    .font(.text(Theme.type.callout, relativeTo: .callout))
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
            FlowRow(spacing: Theme.Space.xs) {
                if AccountRules.showsOK(notice, owner: owner) {
                    Button(words.announceOk) { run { try await sync.ackAnnouncements([notice.id]) } }
                        .buttonStyle(PillButtonStyle(prominent: true))
                }
                // A passkey nobody here added: remove it at once.
                if notice.kind == "passkey", owner, let member = notice.member {
                    Button(words.announceNotYou) {
                        run {
                            try await sync.removeDevice(member)
                            try await sync.ackAnnouncements([notice.id])
                        }
                    }
                    .buttonStyle(DestructiveTextStyle())
                }
            }
            .disabled(busy)
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
