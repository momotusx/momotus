import SwiftUI

private typealias W = SyncWords

/// Account, the sync page (plans/m7-sync.md): not joined, the three ways in (pair,
/// start an account, use a kit); joined, this device's state with every fact
/// of the last round and the 12 characters to compare, the devices with
/// Remove and Report lost, Add a device, Make a recovery kit, stopping, and
/// deleting the account.
struct SyncScreen: View {
    /// A provider to link at once (Settings' Connect Google).
    var connect: String?
    @Environment(SyncModel.self) private var sync
    @Environment(CoreClient.self) private var core
    @Environment(\.metrics) private var metrics
    /// Left a gone server here: sign-in opens (a TV's with the phone).
    @State private var signingIn = false

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: W.title)
                Text(W.lede)
                    .textFont(Theme.type.body)
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
                if sync.keyUnreadable {
                    KeyUnreadable()
                } else if sync.joined {
                    JoinedSync(connect: connect) { signingIn = true }
                } else {
                    NotJoined()
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
        .navigationDestination(isPresented: $signingIn) { signIn }
    }

    /// The build's sign-in, where Sign in again leads; without one, pairing.
    @ViewBuilder private var signIn: some View {
        if let server = AccountServer.default {
            #if os(tvOS)
            PairingScreen(core: core, autostart: server, heading: AccountWords.shared.tvSignIn, lede: AccountWords.shared.tvSignInHow)
            #else
            LaterSignIn(server: server)
            #endif
        } else {
            PairingScreen(core: core)
        }
    }
}

/// The three ways into an account, and why this device left one.
private struct NotJoined: View {
    @Environment(SyncModel.self) private var sync
    @Environment(CoreClient.self) private var core

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            if let problem = sync.problem, problem.code == "accountGone" {
                // Deleted on another device: said once, and OK puts it away.
                Said(tone: .warn, text: problem.words)
                Button(W.ok) { sync.dismissGone() }
                    .buttonStyle(PillButtonStyle(prominent: true))
            } else if let problem = sync.problem {
                Said(tone: .bad, text: problem.words)
            } else if let notice = sync.notice {
                Said(tone: .ok, text: notice)
            }
            if let left = sync.keyLeft {
                Said(tone: .bad, text: left == .afterLeave ? W.keyLeft : W.keyLeftover)
                Button { sync.deleteKeyAgain() } label: { Label(W.retry, glyph: "refresh-cw") }
                    .buttonStyle(PillButtonStyle(prominent: true))
            }
            // The build's hosted relay: sign in (a TV: with the phone), the library added to the account.
            if let server = AccountServer.default {
                #if os(tvOS)
                NavigationLink {
                    PairingScreen(core: core, autostart: server, heading: AccountWords.shared.tvSignIn, lede: AccountWords.shared.tvSignInHow)
                } label: {
                    SettingsRow(symbol: "qr-code", title: AccountWords.shared.tvSignIn, detail: AccountWords.shared.tvSignInHow)
                }
                .buttonStyle(.row)
                #else
                NavigationLink { LaterSignIn(server: server) } label: {
                    SettingsRow(symbol: "door-open", title: AccountWords.shared.signInTitle, detail: AccountWords.shared.libraryAdded)
                }
                .buttonStyle(.row)
                #endif
            }
            NavigationLink { PairingScreen(core: core) } label: {
                SettingsRow(symbol: "qr-code", title: W.pairTitle, detail: W.pairText)
            }
            .buttonStyle(.row)
            NavigationLink { SyncJoinScreen(kit: false) } label: {
                SettingsRow(symbol: "plus", title: W.startTitle, detail: W.startText)
            }
            .buttonStyle(.row)
            NavigationLink { SyncJoinScreen(kit: true) } label: {
                SettingsRow(symbol: "life-buoy", title: W.recoverTitle, detail: W.recoverText)
            }
            .buttonStyle(.row)
        }
    }
}

/// The Keychain did not answer: this device may be in an account, so none
/// of the ways in shows (each would replace it unseen), only Try again.
struct KeyUnreadable: View {
    @Environment(SyncModel.self) private var sync

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Said(tone: .bad, text: W.keyUnreadable)
            Button { sync.readKeyAgain() } label: { Label(W.retry, glyph: "refresh-cw") }
                .buttonStyle(PillButtonStyle(prominent: true))
        }
    }
}

private struct JoinedSync: View {
    let connect: String?
    /// Left a gone server here: open sign-in.
    let signInAgain: () -> Void
    @Environment(SyncModel.self) private var sync
    @Environment(ProfileSession.self) private var profiles
    @Environment(\.metrics) private var metrics
    /// The list could not be fetched, in words.
    @State private var listError: String?
    /// The member a Remove waits on (the second step).
    @State private var removing: MemberInfo?
    /// A line under the list or a member: what just happened.
    @State private var said: (id: String, tone: StatusDot.Tone, text: String)?
    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            state
            kitCallouts
            SignedInWith(connect: connect)
            devices
            links
            // A TV's Sign out always removes the library: the owner's only (D13).
            // A gone server's one way out is Sign in again, on the state card.
            if (!isTV || SyncRules.mayDeleteAccount(profiles.snapshot)) && sync.problem?.code != "relayGone" { LeaveSync() }
            if SyncRules.mayDeleteAccount(profiles.snapshot) { DeleteSync() }
        }
        .task { await load() }
        // A new warning is said once; a round that repeats it says nothing.
        .onChange(of: warnings) { old, new in
            if let words = SyncRules.announcement(old: old, new: new) { announce(words) }
        }
        .confirmationDialog(
            removing.map { "\(W.remove) \(SyncRules.memberName($0))?" } ?? "",
            isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }),
            titleVisibility: .visible, presenting: removing
        ) { member in
            Button("\(W.remove) \(SyncRules.memberName(member))", role: .destructive) { Task { await remove(member) } }
            Button(W.cancel, role: .cancel) {}
        } message: { member in
            Text(member.passkey ? AccountWords.shared.removePasskeyAsk
                 : member.kit ? W.removeKitAsk : W.removeDeviceAsk(SyncRules.memberName(member), kits: SyncRules.kitsMade(by: member.id, in: members)))
        }
    }

    private var failedSince: String? { W.failedSince(sync.failed) }

    /// The list may be out of date: a read after a round failed (the page's own read says why itself).
    private var listStale: Bool { sync.listFailed && listError == nil && sync.devices != nil }

    private var warnings: [String] {
        SyncRules.warnings(problem: sync.problem?.words, failed: failedSince, notes: sync.lastReport?.notes ?? [], listStale: listStale)
    }

    private var isTV: Bool {
        #if os(tvOS)
        true
        #else
        false
        #endif
    }

    private var members: [MemberInfo] { SyncRules.sortMembers(sync.devices?.members ?? []) }
    /// No kit the user holds: none in the list but those that went unseen here.
    private var noKit: Bool { sync.devices != nil && SyncRules.noKit(members, unseen: sync.kits.unseen) }

    /// Only the owner makes a kit, a copy of the whole account key (D17).
    private var mayMakeKit: Bool { SyncRules.mayMakeKit(profiles.snapshot) }

    /// A kit made here waits to be saved, one went unseen, or the account has none.
    @ViewBuilder private var kitCallouts: some View {
        #if !os(tvOS)
        if sync.kits.held != nil {
            VStack(alignment: .leading, spacing: Theme.Space.xs) {
                Callout(glyph: "key-round", text: W.kitWaiting)
                if mayMakeKit {
                    NavigationLink { RecoveryKitScreen() } label: { Label(W.showKitAgain, glyph: "key-round") }
                        .buttonStyle(PillButtonStyle(prominent: true))
                }
            }
        }
        #endif
        if sync.recovered || noKit || !sync.kits.unseen.isEmpty {
            VStack(alignment: .leading, spacing: Theme.Space.xs) {
                Callout(glyph: sync.kits.unseen.isEmpty ? "key-round" : "triangle-alert", text: kitLine)
                #if !os(tvOS)
                if sync.kits.held == nil && mayMakeKit {
                    NavigationLink { RecoveryKitScreen() } label: {
                        Label(sync.kits.unseen.isEmpty ? W.kitTitle : W.makeNewKit, glyph: "key-round")
                    }
                    .buttonStyle(PillButtonStyle())
                }
                #endif
            }
        }
        if sync.kits.unknownUnseen {
            VStack(alignment: .leading, spacing: Theme.Space.xs) {
                Callout(glyph: "triangle-alert", text: W.kitUnseenUnknown)
                Button { sync.checkedUnknownKit() } label: { Label(W.checked, glyph: "check") }
                    .buttonStyle(PillButtonStyle())
            }
        }
    }

    private var kitLine: String {
        if !sync.kits.unseen.isEmpty { return sync.kits.expired ? W.kitExpired : W.kitUnseen }
        if sync.recovered { return isTV ? W.tvAfterRecover : W.afterRecover }
        return W.noKit
    }

    /// The latest device list's 12 characters: a round's own value can be a step behind it.
    private var fingerprint: String? { sync.devices?.fingerprint ?? sync.lastReport?.fingerprint }

    private func load() async {
        do {
            try await sync.loadDevices()
            listError = nil
        } catch {
            listError = SyncProblem(error, action: .devices).words
        }
    }

    // MARK: This device

    private var state: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Said(tone: stateTone, text: stateText)
            ForEach(sync.lastReport?.notes ?? [], id: \.self) { note in
                Label(note, glyph: "triangle-alert")
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
            // Kept past clean rounds until dismissed: a refused edit stays here only.
            if let failedSince {
                Label(failedSince, glyph: "triangle-alert")
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
                Button(W.dismiss) { sync.dismissFailed() }.buttonStyle(PillButtonStyle())
            }
            FlowRow(spacing: Theme.Space.xs) {
                Button { sync.request() } label: { Label(sync.syncing ? W.syncing : W.syncNow, glyph: "refresh-cw") }
                    .buttonStyle(PillButtonStyle(prominent: true))
                    .disabled(sync.syncing)
                if let problem = sync.problem { fixes(problem) }
            }
            // The sync server will not let this device leave (a different list it may not
            // settle, another server, lost entries): stopping here only is the way out.
            if sync.problem?.code == "relayGone" {
                SignInAgain(then: signInAgain)
            } else if let code = sync.problem?.code, ["fork", "wrongRelay", "relayBehind", "notJoined"].contains(code) {
                LeaveHere()
            }
            if let fingerprint {
                VStack(alignment: .leading, spacing: Theme.Space.xs) {
                    Text(caps: W.compareLabel)
                        .textFont(Theme.type.caption, weight: .semibold, relativeTo: .caption)
                        .tracking(1.6)
                        .foregroundStyle(Theme.Palette.muted)
                    Text(SyncRules.fingerprintGroups(fingerprint))
                        .font(.mono(Theme.type.title2))
                        .foregroundStyle(Theme.Palette.text)
                        .accessibilityLabel("\(W.compareLabel): \(fingerprint.map(String.init).joined(separator: " "))")
                    Text(W.compareHelp)
                        .textFont(Theme.type.callout, relativeTo: .callout)
                        .foregroundStyle(Theme.Palette.muted)
                        .fixedSize(horizontal: false, vertical: true)
                    if listStale { Said(tone: .warn, text: W.listStale) }
                }
                .padding(.top, Theme.Space.xs)
            }
        }
        .padding(Theme.Space.s)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
    }

    /// Green only for a round with nothing to say: a refused, unreadable,
    /// waiting or blocked change is a warning, never "synced" in green.
    private var stateTone: StatusDot.Tone {
        if sync.problem != nil { return .bad }
        if sync.syncing { return .warn }
        guard let report = sync.lastReport, sync.lastSynced != nil else { return .idle }
        return report.notes.isEmpty ? .ok : .warn
    }

    /// "Synced at 3:15: 2 sent, 5 received", or the problem in words.
    private var stateText: String {
        if let problem = sync.problem { return problem.words }
        if sync.syncing { return W.syncing }
        guard let at = sync.lastSynced, let report = sync.lastReport else { return W.notSynced }
        let sent = W.count(report.pushed, "change sent", "changes sent")
        let received = W.count(report.applied, "change received", "changes received")
        return "Synced at \(at.formatted(date: .omitted, time: .shortened)): \(sent), \(received)."
    }

    /// The fix a round's problem calls for.
    @ViewBuilder private func fixes(_ problem: SyncProblem) -> some View {
        switch problem.code {
        case "fork":
            // The server's list is never taken unseen: both lists first, on their own page.
            NavigationLink { ForkReviewScreen() } label: { Label(W.compare, glyph: "monitor-smartphone") }
                .buttonStyle(PillButtonStyle())
        default:
            EmptyView()
        }
    }

    // MARK: Devices

    @ViewBuilder private var devices: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Text(W.devices).display(Theme.type.title2).foregroundStyle(Theme.Palette.text).accessibilityAddTraits(.isHeader)
            if let said, said.id == "list" { Said(tone: said.tone, text: said.text) }
            if let listError { Said(tone: .bad, text: listError) }
            if sync.devices == nil, listError == nil { Said(tone: .warn, text: W.loadingDevices) }
            ForEach(members) { member in row(member) }
        }
    }

    private func row(_ member: MemberInfo) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            HStack(alignment: .top, spacing: Theme.Space.s) {
                Glyph(member.kit ? "key-round" : "monitor-smartphone").frame(width: 24)
                VStack(alignment: .leading, spacing: 2) {
                    // The tag beside the name while it fits on one line, else under it (large text).
                    ViewThatFits(in: .horizontal) {
                        HStack(spacing: Theme.Space.xs) { memberName(member); thisDeviceTag(member) }
                        VStack(alignment: .leading, spacing: 0) { memberName(member); thisDeviceTag(member) }
                    }
                    Text(W.addedLine(kit: member.kit, addedMs: member.addedMs, by: SyncRules.sponsorName(member, in: members)))
                        .textFont(Theme.type.callout, relativeTo: .callout)
                        .foregroundStyle(Theme.Palette.muted)
                    ForEach(accountLines(member), id: \.self) { line in
                        Text(line)
                            .textFont(Theme.type.callout, relativeTo: .callout)
                            .foregroundStyle(Theme.Palette.muted)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                .accessibilityElement(children: .combine)
            }
            if let said, said.id == member.id { Said(tone: said.tone, text: said.text) }
            if !member.thisDevice {
                FlowRow(spacing: Theme.Space.xs) {
                    Button { Task { await askRemove(member) } } label: { Label(W.remove, glyph: "trash") }
                        .buttonStyle(PillButtonStyle())
                        .accessibilityLabel("\(W.remove) \(SyncRules.memberName(member))")
                        .disabled(busy)
                    if !member.kit {
                        NavigationLink { ReportLostScreen(lost: member) } label: { Label(W.reportLost, glyph: "shield-alert") }
                            .buttonStyle(PillButtonStyle())
                            .accessibilityLabel("\(W.reportLost): \(SyncRules.memberName(member))")
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

    /// Remove asks only on a list read now: the kits it names are the ones that go.
    private func askRemove(_ member: MemberInfo) async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        do {
            let list = try await sync.loadDevices()
            listError = nil
            // Gone from the fresh list already: its row goes with it, nothing to ask.
            removing = list.members.first { $0.id == member.id }
        } catch {
            said = (member.id, .bad, W.removeStale)
        }
    }

    /// A kit's "Last used", a device's "Joined with a passkey", and on this
    /// phone or tablet the note to sign out before giving it away (3.2).
    private func accountLines(_ member: MemberInfo) -> [String] {
        var lines: [String] = []
        if member.kit, let used = AccountRules.lastUsed(member, members: members) { lines.append(used) }
        if !member.kit, member.viaPasskey {
            lines.append(AccountRules.fill(AccountWords.shared.joinedWithPasskey, ["date": AccountRules.date(member.addedMs)]))
        }
        if member.thisDevice, member.form == "phone" || member.form == "tablet", members.contains(where: \.passkey) {
            lines.append(AccountWords.shared.givingAway)
        }
        return lines
    }

    private func memberName(_ member: MemberInfo) -> some View {
        Text(SyncRules.memberName(member))
            .textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
    }

    @ViewBuilder private func thisDeviceTag(_ member: MemberInfo) -> some View {
        if member.thisDevice {
            Text(W.thisDevice)
                .textFont(Theme.type.caption, weight: .semibold, relativeTo: .caption)
                .foregroundStyle(Theme.Palette.muted)
                .lineLimit(1)
                .fixedSize()
        }
    }

    private func remove(_ member: MemberInfo) async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        do {
            try await sync.removeDevice(member.id)
            said = ("list", .ok, "\(W.removed): \(SyncRules.memberName(member))")
        } catch {
            said = (member.id, .bad, SyncProblem(error, action: .remove).words)
        }
    }

    // MARK: Add a device, make a kit

    @ViewBuilder private var links: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            #if os(tvOS)
            Elsewhere(glyph: "qr-code", title: W.addTitle, text: W.tvAdd)
            if mayMakeKit { Elsewhere(glyph: "key-round", title: W.kitTitle, text: W.tvKit) }
            #elseif os(macOS)
            Elsewhere(glyph: "qr-code", title: AccountWords.shared.signInAnother, text: AccountWords.shared.usePhoneToSignIn)
            if mayMakeKit {
                SavePasskeyRow()
                NavigationLink { RecoveryKitScreen() } label: { SettingsRow(symbol: "key-round", title: W.kitTitle, detail: W.kitText) }
                    .buttonStyle(.row)
            }
            #else
            NavigationLink { AddDeviceScreen() } label: { SettingsRow(symbol: "qr-code", title: AccountWords.shared.signInAnother, detail: W.addText) }
                .buttonStyle(.row)
            if mayMakeKit {
                SavePasskeyRow()
                NavigationLink { RecoveryKitScreen() } label: { SettingsRow(symbol: "key-round", title: W.kitTitle, detail: W.kitText) }
                    .buttonStyle(.row)
            }
            #endif
        }
    }
}

/// Stop syncing: queued edits go first; with edits unsent, sync again or stop anyway.
private struct LeaveSync: View {
    @Environment(SyncModel.self) private var sync
    @Environment(AccountWipe.self) private var wipe
    @Environment(ProfileSession.self) private var profiles
    /// D13: the library goes too when asked; off at first, always on a TV.
    @State private var removeLibrary = AccountRules.removesLibraryAtFirst
    /// And each debrid connection, ticked by default.
    @State private var disconnectDebrid = true
    @State private var unsent = false
    /// The sync server would not or could not let it leave: stopping here only still works.
    @State private var refused = false
    @State private var error: String?
    @State private var busy = false
    /// No lock to ask with: the second step, for a stop with or without `force`.
    @State private var again: Bool?
    #if os(tvOS)
    @FocusState private var cancelFocused: Bool
    #endif

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Text(W.leaveTitle).display(Theme.type.title2).foregroundStyle(Theme.Palette.text).accessibilityAddTraits(.isHeader)
            Text(W.leaveText)
                .textFont(Theme.type.callout, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.muted)
            if owner {
                #if !os(tvOS)
                switchRow(AccountWords.shared.removeLibraryAsk, on: $removeLibrary)
                #else
                Text(AccountWords.shared.removeLibraryAsk)
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.text)
                #endif
                if removes {
                    switchRow(AccountWords.shared.removeLibraryDebrid, on: $disconnectDebrid)
                }
            }
            if let error { Said(tone: .bad, text: error) }
            if let problem = wipe.problem { Said(tone: .bad, text: problem) }
            #if os(tvOS)
            if let force = again {
                // Its own step, Cancel first: one press never stops syncing on a TV.
                Text(W.tvSure)
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.text)
                FlowRow(spacing: Theme.Space.xs) {
                    Button(W.cancel) { again = nil }
                        .buttonStyle(PillButtonStyle(prominent: true))
                        .focused($cancelFocused)
                    Button { Task { await leave(force: force, askedAgain: true) } } label: { Label(force ? W.leaveAnyway : W.leaveButton, glyph: "log-out") }
                        .buttonStyle(DestructiveTextStyle())
                }
                .onAppear { cancelFocused = true }
            }
            #endif
            FlowRow(spacing: Theme.Space.xs) {
                if unsent {
                    Button { unsent = false; error = nil; sync.request() } label: { Label(W.syncAgain, glyph: "refresh-cw") }
                        .buttonStyle(PillButtonStyle())
                    Button { Task { await leave(force: true) } } label: { Label(W.leaveAnyway, glyph: "log-out") }
                        .buttonStyle(DestructiveTextStyle())
                } else {
                    Button { Task { await leave(force: false) } } label: { Label(W.leaveButton, glyph: "log-out") }
                        .buttonStyle(DestructiveTextStyle())
                }
            }
            .disabled(busy || again != nil)
            if refused { LeaveHere() }
        }
        #if !os(tvOS)
        // `presenting`: the stop it asks about is fixed when it opens, whatever clears `again` first.
        .confirmationDialog(W.leaveTitle, isPresented: Binding(get: { again != nil }, set: { if !$0 { again = nil } }), titleVisibility: .visible, presenting: again) { force in
            Button(force ? W.leaveAnyway : W.leaveButton, role: .destructive) {
                Task { await leave(force: force, askedAgain: true) }
            }
            Button(W.cancel, role: .cancel) {}
        } message: { _ in
            Text(W.noLockAsk)
        }
        #endif
    }

    /// The removal is the owner profile's only (D13); a TV always removes.
    private var owner: Bool { SyncRules.mayDeleteAccount(profiles.snapshot) }

    /// D13's choices with the app's own switch (`SwitchMark`), a row the whole of which flips it.
    private func switchRow(_ title: String, on: Binding<Bool>) -> some View {
        Button { on.wrappedValue.toggle() } label: {
            HStack(spacing: Theme.Space.s) {
                Text(title)
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.text)
                    .fixedSize(horizontal: false, vertical: true)
                Spacer(minLength: 0)
                SwitchMark(on: on.wrappedValue)
            }
            .multilineTextAlignment(.leading)
            .padding(.horizontal, Theme.Space.s)
            .padding(.vertical, Theme.Space.xs)
            .frame(maxWidth: .infinity, alignment: .leading)
            .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
        }
        .buttonStyle(.row)
        .accessibilityLabel(title)
        .accessibilityValue(on.wrappedValue ? "On" : "Off")
        .accessibilityAddTraits(.isToggle)
    }

    private var isTV: Bool {
        #if os(tvOS)
        true
        #else
        false
        #endif
    }

    private var removes: Bool { owner && AccountRules.removesLibrary(asked: removeLibrary, tv: isTV) }

    private func leave(force: Bool, askedAgain: Bool = false) async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        error = nil
        again = nil
        do {
            if removes {
                let ids = profiles.snapshot?.profiles.map(\.id) ?? []
                try await wipe.signOutAndRemove(profiles: ids, disconnectDebrid: disconnectDebrid, force: force, askedAgain: askedAgain)
            } else {
                try await sync.signOut(force: force, askedAgain: askedAgain)
            }
        } catch {
            let problem = SyncProblem(error, action: .leave)
            // No lock to ask with (or a TV): the page asks a second time.
            if problem.code == "noLockAsk" {
                again = force
                return
            }
            unsent = problem.code == "unsent"
            // Not for edits that wait or a prompt the user cancelled.
            refused = problem.code != "unsent" && problem.code != "unconfirmed" && problem.code != "noPasscode"
            self.error = problem.words
        }
    }
}

/// Delete the account for every device: one confirmation that names what
/// goes, then the OS prompt (none without a passcode, or on a TV: the
/// confirmation is the step). On tvOS the confirmation is its own step with
/// Cancel focused, as Leave's: one press never deletes the account.
private struct DeleteSync: View {
    @Environment(SyncModel.self) private var sync
    /// Apple signs in to this account: its sheet re-authenticates the delete.
    @State private var appleLinked = false
    /// Apple's step failed: the next press deletes anyway, after the OS prompt.
    @State private var anyway = false
    /// Apple's receipt, good for 10 minutes: a delete that failed after it tries again with it.
    @State private var receipt: (value: String, until: Date)?
    @State private var asking = false
    @State private var error: String?
    @State private var busy = false
    #if os(tvOS)
    @FocusState private var cancelFocused: Bool
    /// The row's own button: Cancel hands the focus back to it (the tvOS walk, ADT-F1).
    @FocusState private var rowFocused: Bool
    #endif

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Text(W.deleteTitle).display(Theme.type.title2).foregroundStyle(Theme.Palette.text).accessibilityAddTraits(.isHeader)
            Text(W.deleteText)
                .textFont(Theme.type.callout, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
            if let error { Said(tone: .bad, text: error) }
            #if os(tvOS)
            if asking {
                Text(W.deleteAsk)
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.text)
                    .fixedSize(horizontal: false, vertical: true)
                FlowRow(spacing: Theme.Space.xs) {
                    Button(W.cancel) { asking = false }
                        .buttonStyle(PillButtonStyle(prominent: true))
                        .focused($cancelFocused)
                        // While the delete runs, Cancel cannot hide a step whose deletion still lands.
                        .disabled(busy)
                    Button { Task { await delete() } } label: { Label(busy ? W.deleting : W.deleteButton, glyph: "trash") }
                        .buttonStyle(DestructiveTextStyle())
                        .disabled(busy)
                }
                .onAppear { cancelFocused = true }
            }
            #endif
            Button { asking = true } label: { Label(busy ? W.deleting : anyway ? AccountWords.shared.deleteAnyway : W.deleteButton, glyph: "trash") }
                .buttonStyle(DestructiveTextStyle())
                .disabled(busy || isTV && asking)
                #if os(tvOS)
                .focused($rowFocused)
                #endif
        }
        #if os(tvOS)
        // Disabled while the step shows, the row cannot take the focus in the
        // same update that closes it: it takes it once it is enabled again.
        .onChange(of: asking) { _, now in if !now { rowFocused = true } }
        #endif
        // Apple signs in to it: its sheet, not the OS prompt, re-authenticates.
        .task { appleLinked = (try? await sync.identities())?.contains { $0.provider == "apple" } ?? false }
        #if !os(tvOS)
        .confirmationDialog(W.deleteTitle + "?", isPresented: $asking, titleVisibility: .visible) {
            Button(W.deleteButton, role: .destructive) { Task { await delete() } }
            Button(W.cancel, role: .cancel) {}
        } message: {
            Text(W.deleteAsk)
        }
        #endif
    }

    private var isTV: Bool {
        #if os(tvOS)
        true
        #else
        false
        #endif
    }

    private func delete() async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        error = nil
        #if !os(tvOS)
        if appleLinked, !anyway {
            let held: String
            if let receipt, receipt.until > .now {
                held = receipt.value
            } else {
                do {
                    held = try await AppleReauth.receipt(sync: sync)
                    receipt = (held, .now.addingTimeInterval(10 * 60))
                } catch {
                    if case SignInSheets.Failure.cancelled = error { return }
                    // Apple's step did not run: Delete anyway, behind the OS prompt.
                    anyway = true
                    self.error = SyncProblem(error, action: .delete).words
                    return
                }
            }
            do {
                try await sync.deleteAccount(receipt: held)
                receipt = nil
            } catch {
                // A good receipt is tried again on the next press; one the relay refused is not.
                if (error as? CoreError)?.code == "receiptRefused" {
                    receipt = nil
                    anyway = true
                }
                self.error = SyncProblem(error, action: .delete).words
            }
            return
        }
        #endif
        do {
            try await sync.deleteAccount()
        } catch {
            self.error = SyncProblem(error, action: .delete).words
        }
    }
}

/// Stop syncing here only: nothing sent, this device forgets the account,
/// after a step that says what is lost. When the core says this device does
/// not hold the account any more (`wrongRecord`), only forgetting its key here is left.
struct LeaveHere: View {
    @Environment(SyncModel.self) private var sync
    @State private var asking = false
    @State private var forgetting = false
    @State private var error: String?
    @State private var notHere = false
    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            Text(notHere ? W.forgetHereText : W.leaveHereText)
                .textFont(Theme.type.callout, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
            if let error { Said(tone: .bad, text: error) }
            if notHere {
                Button { forgetting = true } label: { Label(W.forgetHere, glyph: "trash") }
                    .buttonStyle(DestructiveTextStyle())
            } else {
                Button { asking = true } label: { Label(W.leaveHere, glyph: "log-out") }
                    .buttonStyle(DestructiveTextStyle())
                    .disabled(busy)
            }
        }
        .confirmationDialog(W.leaveHereAsk, isPresented: $asking, titleVisibility: .visible) {
            Button(W.leaveHere, role: .destructive) { Task { await go() } }
            Button(W.cancel, role: .cancel) {}
        } message: {
            Text(W.leaveHereText)
        }
        .confirmationDialog(W.forgetHere + "?", isPresented: $forgetting, titleVisibility: .visible) {
            Button(W.forgetHere, role: .destructive) { sync.forgetHere() }
            Button(W.cancel, role: .cancel) {}
        } message: {
            Text(W.forgetHereText)
        }
    }

    private func go() async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        error = nil
        do {
            // Its step before ("Stop syncing here only?") was the second ask.
            try await sync.leaveHere(askedAgain: true)
        } catch {
            let problem = SyncProblem(error, action: .leave)
            notHere = problem.code == "wrongRecord"
            self.error = problem.words
        }
    }
}

/// The sync server is gone: leave here (the library stays) and open sign-in, in one press.
struct SignInAgain: View {
    let then: () -> Void
    @Environment(SyncModel.self) private var sync
    @State private var error: String?
    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            if let error { Said(tone: .bad, text: error) }
            Button { Task { await go() } } label: { Label(AccountWords.shared.relayGoneActions[0], glyph: "door-open") }
                .buttonStyle(PillButtonStyle())
                .disabled(busy)
        }
    }

    private func go() async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        error = nil
        do {
            // Leaving ends access, so a device with no passcode still may.
            try await sync.leaveHere(askedAgain: true)
            then()
        } catch {
            self.error = SyncProblem(error, action: .leave).words
        }
    }
}

/// A status line: a coloured dot and its words, so state never rests on colour alone.
struct Said: View {
    let tone: StatusDot.Tone
    let text: String

    var body: some View {
        StatusDot(tone: tone, text: text)
    }
}

/// An aside on the glass surface, with its icon.
struct Callout: View {
    let glyph: String
    let text: String

    var body: some View {
        HStack(alignment: .top, spacing: Theme.Space.s) {
            Glyph(glyph).foregroundStyle(Theme.Palette.text)
            Text(text)
                .textFont(Theme.type.callout, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(Theme.Space.s)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
        .accessibilityElement(children: .combine)
    }
}

/// TV: a row it cannot act on, with the line that says where to do it;
/// focusable so the remote reads it.
struct Elsewhere: View {
    let glyph: String
    let title: String
    let text: String

    var body: some View {
        HStack(alignment: .top, spacing: Theme.Space.s) {
            Glyph(glyph).frame(width: 28)
            VStack(alignment: .leading, spacing: 3) {
                Text(title).textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                Text(text)
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .foregroundStyle(Theme.Palette.text)
        .padding(Theme.Space.s)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
        .accessibilityElement(children: .combine)
        .modifier(ReadableOnTV())
    }
}
