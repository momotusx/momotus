// iOS and macOS only: TV forms show no downloads (DownloadGate).
#if !os(tvOS)
import SwiftUI

/// The control's glyph, in a determinate ring while a download is under way: the spinner's ring as a
/// value, white on the 10 percent track (design/DECISION.md, Progress), 2.5 pt at 28 pt.
struct DownloadRing: View {
    let control: DownloadControl
    var size: CGFloat = 28

    var body: some View {
        ZStack {
            if let ring = control.ring {
                Circle().stroke(.white.opacity(0.1), lineWidth: stroke)
                Circle()
                    .trim(from: 0, to: min(max(ring, 0), 1))
                    .stroke(.white, style: StrokeStyle(lineWidth: stroke, lineCap: .round))
                    .rotationEffect(.degrees(-90))
                    .animation(Theme.Motion.focus, value: ring)
                Glyph(control.glyph.asset, size: size * (filled ? 0.36 : 0.46))
            } else {
                Glyph(control.glyph.asset, size: size * 0.72)
            }
        }
        .padding(stroke / 2)
        .frame(width: size, height: size)
        .foregroundStyle(.white)
        .accessibilityHidden(true)
    }

    private var stroke: CGFloat { size * 2.5 / 28 }
    /// The filled square and pause sit smaller in the ring than an outline glyph.
    private var filled: Bool { control.glyph == .stop || control.glyph == .pause }
}

/// One entry of a download menu: an op, a way somewhere, or a line that only says where things stand.
struct DownloadMenuEntry: Identifiable {
    let id: String
    let label: String
    var glyph: String?
    /// Nil: a line; pressing it does nothing.
    var run: (() -> Void)?
}

/// How the control is drawn where it sits.
enum DownloadControlLook {
    /// The title's action row: a labelled secondary pill (40 pt on the Mac, 44 pt on the iPhone).
    case pill
    /// The Mac panel row's 28 pt pill, with its word.
    case smallPill
    /// The iPhone panel row's 36 pt round button in a 44 pt target.
    case round
    /// The ring alone in a 44 pt target (the Downloads screen's rows).
    case bare
}

/// The download control (plans/offline-downloads.md 3.1): a tap starts a download when nothing is kept
/// (a press and hold, or a right click, offers the version), else opens its menu with the reason in
/// full on top. VoiceOver reads the state's word, the ring's value and, where a tap opens a menu, "Shows
/// download options".
struct DownloadControlButton: View {
    let control: DownloadControl
    var look: DownloadControlLook = .pill
    /// The menu a tap opens; empty, a tap runs `start`.
    let entries: [DownloadMenuEntry]
    var start: () -> Void = {}
    /// "Choose a version to download" where a tap starts the download.
    var choose: (() -> Void)?
    @Environment(\.metrics) private var metrics

    var body: some View {
        if entries.isEmpty {
            Button(action: start) { label }
                .modifier(Chrome(look: look))
                .contextMenu {
                    if let choose { Button(DownloadItem.choose.label, glyph: DownloadItem.choose.glyph, action: choose) }
                }
                .accessibilityLabel(control.spoken)
                .accessibilityActions {
                    if let choose { Button(DownloadItem.choose.label, action: choose) }
                }
        } else {
            Menu {
                if let detail = control.detail {
                    Section(detail) { items }
                } else {
                    items
                }
            } label: { label }
                .menuStyle(.button)
                .menuIndicator(.hidden)
                .modifier(Chrome(look: look))
                .accessibilityLabel(control.spoken)
                .accessibilityValue(control.value ?? "")
                .accessibilityHint(control.hint ?? "")
        }
    }

    @ViewBuilder private var items: some View {
        ForEach(entries) { entry in
            if let run = entry.run {
                if let glyph = entry.glyph { Button(entry.label, glyph: glyph, action: run) } else { Button(entry.label, action: run) }
            } else {
                Text(entry.label)
            }
        }
    }

    @ViewBuilder private var label: some View {
        switch look {
        case .pill:
            Label { Text(control.label).monospacedDigit() } icon: { DownloadRing(control: control, size: metrics.wide ? 20 : 22) }
        case .smallPill:
            HStack(spacing: 6) {
                DownloadRing(control: control, size: 16)
                Text(control.label).monospacedDigit()
            }
            .textFont(13, weight: .semibold, relativeTo: .footnote)
            .foregroundStyle(.white)
            .padding(.horizontal, 12)
            .frame(height: 28)
            .background(SecondaryFill(shape: Capsule()))
        case .round:
            DownloadRing(control: control, size: control.ring == nil ? 20 : 24)
                .frame(width: 36, height: 36)
                .background(SecondaryFill(shape: Circle()))
                .frame(width: 44, height: 44)
                .contentShape(Rectangle())
        case .bare:
            DownloadRing(control: control)
                .frame(width: 44, height: 44)
                .contentShape(Rectangle())
        }
    }

    /// The button style for each look: the pill is the secondary pill; the rest draw their own chrome.
    private struct Chrome: ViewModifier {
        let look: DownloadControlLook

        func body(content: Content) -> some View {
            switch look {
            case .pill: content.buttonStyle(PillButtonStyle()).fixedSize()
            case .smallPill: content.buttonStyle(FocusPlainStyle(shape: Capsule(), scale: 1)).fixedSize()
            case .round, .bare: content.buttonStyle(FocusPlainStyle(shape: Circle(), scale: 1)).fixedSize()
            }
        }
    }
}

/// A video's state on an episode still: a white 20 pt glyph or ring on the black disc of the Mac's
/// "..." (`BackdropStops.cardMoreBacking`), at the still's top leading corner.
struct DownloadStateMark: View {
    let badge: DownloadBadge

    var body: some View {
        DownloadRing(control: DownloadCopy.control(badge), size: 20)
            .frame(width: 30, height: 30)
            .background(Circle().fill(.black.opacity(BackdropStops.cardMoreBacking)))
            .accessibilityHidden(true)
    }
}

/// "Downloaded" as a plain badge: a kids profile's title, which has no control.
struct DownloadedBadge: View {
    var body: some View {
        Label("Downloaded", glyph: "circle-check", size: 15)
            .labelStyle(PillLabelStyle())
            .textFont(13, weight: .semibold, relativeTo: .footnote)
            .foregroundStyle(.white)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(.white.opacity(0.16), in: Capsule())
    }
}

/// The confirmation every download delete takes (design/DECISION.md, Destructive and Confirmation): an
/// Apple dialog in the key tint, Cancel and the plain word, no destructive role and no red.
struct DownloadConfirm: ViewModifier {
    let title: String
    @Binding var isPresented: Bool
    let delete: () -> Void

    func body(content: Content) -> some View {
        content.background {
            Color.clear
                .confirmationDialog(title, isPresented: $isPresented, titleVisibility: .visible) {
                    Button("Delete", action: delete)
                    Button("Cancel", role: .cancel) {}
                }
                .tint(Theme.Palette.key)
        }
    }
}

extension View {
    func downloadConfirm(_ title: String, isPresented: Binding<Bool>, delete: @escaping () -> Void) -> some View {
        modifier(DownloadConfirm(title: title, isPresented: isPresented, delete: delete))
    }
}
#endif
