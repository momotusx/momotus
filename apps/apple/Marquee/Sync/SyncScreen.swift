import SwiftUI

private typealias W = SyncWords

/// Devices and sync (plans/m7-sync.md): not joined, the three ways in (pair,
/// start an account, use a kit); joined, this device's state with every fact
/// of the last round and the 12 characters to compare, the devices with
/// Remove and Report lost, Add a device, Make a recovery kit, and stopping.
struct SyncScreen: View {
    @Environment(SyncModel.self) private var sync
    @Environment(\.metrics) private var metrics

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: W.title)
                Text(W.lede)
                    .font(.text(Theme.type.body))
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
                if sync.joined { JoinedSync() } else { NotJoined() }
            }
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .topScrim()
    }
}

/// The three ways into an account, and why this device left one.
private struct NotJoined: View {
    @Environment(SyncModel.self) private var sync
    @Environment(CoreClient.self) private var core

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            if let problem = sync.problem { Said(tone: .bad, text: problem.words) }
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

private struct JoinedSync: View {
    @Environment(SyncModel.self) private var sync
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
            if sync.recovered || noKit {
                Callout(glyph: "key-round", text: sync.recovered ? (isTV ? W.tvAfterRecover : W.afterRecover) : W.noKit)
            }
            devices
            links
            LeaveSync()
        }
        .task { await load() }
        .confirmationDialog(
            removing.map { "\(W.remove) \(SyncRules.memberName($0))?" } ?? "",
            isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }),
            titleVisibility: .visible, presenting: removing
        ) { member in
            Button("\(W.remove) \(SyncRules.memberName(member))", role: .destructive) { Task { await remove(member) } }
            Button(W.cancel, role: .cancel) {}
        } message: { member in
            Text(member.kit ? W.removeKitAsk : W.removeDeviceAsk(SyncRules.memberName(member)))
        }
    }

    private var isTV: Bool {
        #if os(tvOS)
        true
        #else
        false
        #endif
    }

    private var members: [MemberInfo] { SyncRules.sortMembers(sync.devices?.members ?? []) }
    private var noKit: Bool { sync.devices != nil && !members.contains(where: \.kit) }

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
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
            FlowRow(spacing: Theme.Space.xs) {
                Button { sync.request() } label: { Label(sync.syncing ? W.syncing : W.syncNow, glyph: "refresh-cw") }
                    .buttonStyle(PillButtonStyle(prominent: true))
                    .disabled(sync.syncing)
                if let problem = sync.problem { fixes(problem) }
            }
            if let fingerprint {
                VStack(alignment: .leading, spacing: Theme.Space.xs) {
                    Text(caps: W.compareLabel)
                        .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
                        .tracking(1.6)
                        .foregroundStyle(Theme.Palette.muted)
                    Text(SyncRules.fingerprintGroups(fingerprint))
                        .font(.mono(Theme.type.title2))
                        .foregroundStyle(Theme.Palette.text)
                        .accessibilityLabel("\(W.compareLabel): \(fingerprint.map(String.init).joined(separator: " "))")
                    Text(W.compareHelp)
                        .font(.text(Theme.type.callout, relativeTo: .callout))
                        .foregroundStyle(Theme.Palette.muted)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .padding(.top, Theme.Space.xs)
            }
        }
        .padding(Theme.Space.s)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
    }

    private var stateTone: StatusDot.Tone {
        if sync.problem != nil { return .bad }
        if sync.syncing { return .warn }
        return sync.lastSynced == nil ? .idle : .ok
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
            Button {
                Task {
                    do {
                        try await sync.useServerRoster()
                        await load()
                    } catch {
                        said = ("list", .bad, SyncProblem(error, action: .devices).words)
                    }
                }
            } label: { Label(W.useServer, glyph: "monitor-smartphone") }
                .buttonStyle(PillButtonStyle())
        case "wrongRelay", "relayBehind", "notJoined":
            // Forgetting is destructive, so it reads as such and never wears the accent.
            Button(W.forgetHere) { sync.forgetHere() }.buttonStyle(DestructiveTextStyle())
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
                    HStack(spacing: Theme.Space.xs) {
                        Text(SyncRules.memberName(member))
                            .font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                        if member.thisDevice {
                            Text(W.thisDevice)
                                .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
                                .foregroundStyle(Theme.Palette.muted)
                        }
                    }
                    Text(W.addedLine(kit: member.kit, addedMs: member.addedMs, by: SyncRules.sponsorName(member, in: members)))
                        .font(.text(Theme.type.callout, relativeTo: .callout))
                        .foregroundStyle(Theme.Palette.muted)
                }
                .accessibilityElement(children: .combine)
            }
            if let said, said.id == member.id { Said(tone: said.tone, text: said.text) }
            if !member.thisDevice {
                FlowRow(spacing: Theme.Space.xs) {
                    Button { removing = member } label: { Label(W.remove, glyph: "trash") }
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
            Elsewhere(glyph: "key-round", title: W.kitTitle, text: W.tvKit)
            #else
            NavigationLink { AddDeviceScreen() } label: { SettingsRow(symbol: "qr-code", title: W.addTitle, detail: W.addText) }
                .buttonStyle(.row)
            NavigationLink { RecoveryKitScreen() } label: { SettingsRow(symbol: "key-round", title: W.kitTitle, detail: W.kitText) }
                .buttonStyle(.row)
            #endif
        }
    }
}

/// Stop syncing: queued edits go first; with edits unsent, sync again or stop anyway.
private struct LeaveSync: View {
    @Environment(SyncModel.self) private var sync
    @State private var unsent = false
    /// Leaving needs the sync server; when it cannot be reached, the account can be forgotten here only.
    @State private var unreachable = false
    @State private var error: String?
    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Text(W.leaveTitle).display(Theme.type.title2).foregroundStyle(Theme.Palette.text).accessibilityAddTraits(.isHeader)
            Text(W.leaveText)
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.muted)
            if let error { Said(tone: .bad, text: error) }
            FlowRow(spacing: Theme.Space.xs) {
                if unsent {
                    Button { unsent = false; error = nil; sync.request() } label: { Label(W.syncAgain, glyph: "refresh-cw") }
                        .buttonStyle(PillButtonStyle())
                    Button { Task { await leave(force: true) } } label: { Label(W.leaveAnyway, glyph: "log-out") }
                        .buttonStyle(DestructiveTextStyle())
                } else {
                    Button { Task { await leave(force: false) } } label: { Label(W.leaveButton, glyph: "log-out") }
                        .buttonStyle(DestructiveTextStyle())
                    if unreachable {
                        Button { sync.forgetHere() } label: { Label(W.forgetHere, glyph: "trash") }
                            .buttonStyle(DestructiveTextStyle())
                    }
                }
            }
            .disabled(busy)
            if unreachable {
                Text(W.forgetHereText)
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private func leave(force: Bool) async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        error = nil
        do {
            try await sync.signOut(force: force)
        } catch {
            let problem = SyncProblem(error, action: .leave)
            unsent = problem.code == "unsent"
            unreachable = problem.code == nil
            self.error = problem.words
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
                .font(.text(Theme.type.callout, relativeTo: .callout))
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
                Text(title).font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                Text(text)
                    .font(.text(Theme.type.callout, relativeTo: .callout))
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
