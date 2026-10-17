import SwiftUI

/// The sync account on this device: last round, problems with their fix,
/// the devices in the account, a recovery kit, and leaving.
struct SyncScreen: View {
    @Environment(SyncModel.self) private var sync
    @Environment(\.metrics) private var metrics
    @State private var kit: String?
    @State private var confirmLeave = false

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: "Sync")
                status
                if let problem = sync.problem { problemView(problem) }
                if let notes = sync.lastReport?.notes, !notes.isEmpty {
                    VStack(alignment: .leading, spacing: 6) {
                        ForEach(notes, id: \.self) { Label($0, systemImage: "info.circle") }
                    }
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
                }
                devices
                actions
            }
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .task { await sync.loadDevices() }
        .sheet(item: Binding(get: { kit.map(KitText.init) }, set: { kit = $0?.text })) { shown in
            RecoveryKitSheet(kit: shown.text) { kit = nil }
        }
        .confirmationDialog("Leave the sync account on this device?", isPresented: $confirmLeave, titleVisibility: .visible) {
            Button("Leave", role: .destructive) { Task { await sync.signOut(force: false) } }
        } message: {
            Text("Queued changes are sent first. Your library stays on this device.")
        }
    }

    private var status: some View {
        VStack(alignment: .leading, spacing: 6) {
            if sync.syncing {
                Label("Syncing", systemImage: "arrow.triangle.2.circlepath")
            } else if let at = sync.lastSynced, let report = sync.lastReport {
                Label("Synced \(at.formatted(date: .omitted, time: .shortened)): \(report.pushed) sent, \(report.applied) changes received",
                      systemImage: "checkmark.circle")
            } else {
                Label(sync.joined ? "Not synced yet in this session" : "This device is not in a sync account", systemImage: "circle.dashed")
            }
            if let fingerprint = sync.lastReport?.fingerprint ?? sync.devices?.fingerprint {
                Text("Fingerprint \(grouped(fingerprint)). It is the same on every device of the account.")
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
            }
        }
        .font(.text(Theme.type.headline, weight: .medium, relativeTo: .headline))
        .foregroundStyle(Theme.Palette.text)
    }

    @ViewBuilder private func problemView(_ problem: SyncProblem) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Label(problem.words, systemImage: "exclamationmark.triangle")
                .font(.text(Theme.type.body))
                .foregroundStyle(Theme.Palette.text)
            switch problem.code {
            case "fork":
                Button("Use the server's list") { Task { await sync.useServerRoster() } }.buttonStyle(PillButtonStyle(prominent: true))
            case "wrongRelay", "relayBehind", "notJoined":
                Button("Forget it on this device") { sync.forgetHere() }.buttonStyle(PillButtonStyle(prominent: true))
                Text("Your library stays. Pair again to join an account.")
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
            case "unsent":
                HStack(spacing: 12) {
                    Button("Sync now") { Task { await sync.syncNow() } }.buttonStyle(PillButtonStyle(prominent: true))
                    Button("Leave anyway") { Task { await sync.signOut(force: true) } }.buttonStyle(PillButtonStyle())
                }
            default:
                EmptyView()
            }
        }
        .padding(Theme.Space.s)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous).fill(Theme.Palette.surface))
    }

    @ViewBuilder private var devices: some View {
        if let list = sync.devices {
            VStack(alignment: .leading, spacing: 10) {
                Text("Devices").font(.display(Theme.type.title2)).foregroundStyle(Theme.Palette.text)
                ForEach(list.members) { member in
                    HStack(spacing: Theme.Space.s) {
                        Image(systemName: member.kit ? "key" : "laptopcomputer.and.iphone")
                            .frame(width: 24)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(memberName(member) + (member.thisDevice ? " (this device)" : ""))
                                .font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                            Text(memberDetail(member, all: list.members))
                                .font(.text(Theme.type.callout, relativeTo: .callout))
                                .foregroundStyle(Theme.Palette.muted)
                        }
                    }
                    .foregroundStyle(Theme.Palette.text)
                }
                if list.rotationPending {
                    Label("A key change is waiting to finish.", systemImage: "clock")
                        .font(.text(Theme.type.callout, relativeTo: .callout))
                        .foregroundStyle(Theme.Palette.muted)
                }
            }
        }
    }

    private var actions: some View {
        FlowRow(spacing: 12) {
            Button { Task { await sync.syncNow() } } label: { Label("Sync now", systemImage: "arrow.triangle.2.circlepath") }
                .buttonStyle(PillButtonStyle(prominent: true))
                .disabled(sync.syncing || !sync.joined)
            #if !os(tvOS)
            Button { Task { kit = await sync.createRecoveryKit() } } label: { Label("Create a recovery kit", systemImage: "key") }
                .buttonStyle(PillButtonStyle())
                .disabled(!sync.joined)
            #endif
            Button { confirmLeave = true } label: { Label("Leave sync", systemImage: "rectangle.portrait.and.arrow.right") }
                .buttonStyle(PillButtonStyle())
                .disabled(!sync.joined)
        }
    }

    private func memberName(_ member: MemberInfo) -> String {
        member.name ?? (member.kit ? "Recovery kit" : "Unnamed device")
    }

    /// "Added Oct 17 by Desk" or "Started the account".
    private func memberDetail(_ member: MemberInfo, all: [MemberInfo]) -> String {
        let added = Date(timeIntervalSince1970: TimeInterval(member.addedMs) / 1000).formatted(date: .abbreviated, time: .omitted)
        guard let sponsor = member.sponsor else { return "Started the account \(added)" }
        let by = all.first { $0.id == sponsor }.map(memberName) ?? "another device"
        return "Added \(added) by \(by)"
    }

    /// "ABCD EFGH IJKL" reads better than twelve letters in a row.
    private func grouped(_ text: String) -> String {
        stride(from: 0, to: text.count, by: 4).map { start in
            let begin = text.index(text.startIndex, offsetBy: start)
            return String(text[begin..<(text.index(begin, offsetBy: 4, limitedBy: text.endIndex) ?? text.endIndex)])
        }
        .joined(separator: " ")
    }
}

private struct KitText: Identifiable {
    let text: String
    var id: String { text }
}

/// The kit is shown once and kept nowhere on this device.
private struct RecoveryKitSheet: View {
    let kit: String
    let done: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            Text("Your recovery kit").font(.display(Theme.type.title1))
            Text("Write it down or save it in a password manager. It lets you get your library back if you lose every device. Marquee does not keep a copy.")
                .font(.text(Theme.type.body))
                .foregroundStyle(Theme.Palette.muted)
            Text(kit)
                .font(.mono(Theme.type.title3))
                .foregroundStyle(Theme.Palette.text)
                .selectableText()
                .padding(Theme.Space.s)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: Theme.Radius.s).fill(Theme.Palette.surface))
            Button("I saved it", action: done).buttonStyle(PillButtonStyle(prominent: true))
        }
        .padding(Theme.Space.l)
        .background(Theme.Palette.bg)
        .presentationDetents([.medium, .large])
    }
}
