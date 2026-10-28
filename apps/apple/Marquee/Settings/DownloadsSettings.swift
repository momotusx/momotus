// iOS and macOS only: TV forms have no downloads (DownloadGate).
#if !os(tvOS)
import SwiftUI

/// Settings' Downloads (plans/offline-downloads.md 3.7): Wi-Fi only (iPhone and iPad: a Mac always
/// tells the core its network is not metered), Smart downloads with its line, the storage line, and
/// Delete all downloads, which asks first.
struct DownloadSettingsSection: View {
    let keeper: DownloadKeeper
    let snapshot: DownloadsSnapshot
    @State private var asking = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            #if os(iOS)
            SwitchRow(title: DownloadCopy.wifiOnly, line: nil, isOn: Binding(get: { snapshot.settings.wifiOnly }, set: { keeper.setSettings(wifiOnly: $0) }))
            #endif
            SwitchRow(title: DownloadCopy.smart, line: DownloadCopy.smartLine, isOn: Binding(get: { snapshot.settings.smart }, set: { keeper.setSettings(smart: $0) }))
            Text(DownloadCopy.storage(snapshot.storage))
                .textFont(Theme.type.callout, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.muted)
                .monospacedDigit()
            if snapshot.storage.downloadsBytes > 0 || !snapshot.titles.isEmpty || !snapshot.downloading.isEmpty {
                // The plain secondary with its white word: no red (design/DECISION.md, Destructive).
                Button { asking = true } label: { Label(DownloadCopy.deleteAllButton, glyph: DownloadItem.delete.glyph) }
                    .buttonStyle(PillButtonStyle())
                    .downloadConfirm(DownloadCopy.deleteAll(snapshot.storage.downloadsBytes), isPresented: $asking) {
                        keeper.act(.remove, .all)
                    }
            }
        }
        .downloadProblem(keeper)
    }
}

/// A setting that is on or off: its words at the left, Apple's switch in its green at the right.
private struct SwitchRow: View {
    let title: String
    let line: String?
    @Binding var isOn: Bool

    var body: some View {
        Toggle(isOn: $isOn) {
            VStack(alignment: .leading, spacing: 3) {
                Text(title).textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                if let line {
                    Text(line)
                        .textFont(Theme.type.callout, relativeTo: .callout)
                        .foregroundStyle(Theme.Palette.muted)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
        .toggleStyle(.switch)
        .tint(Theme.Palette.switchOn)
        .foregroundStyle(Theme.Palette.text)
        .padding(Theme.Space.s)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
    }
}
#endif
