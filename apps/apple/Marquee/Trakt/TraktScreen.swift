import SwiftUI

private typealias W = TraktWords

/// Trakt (plans/m7-trakt.md section 3): connect with a code shown here and
/// approved on trakt.tv, then the card (who, when it last synced, what waits,
/// what Trakt could not name) with Sync now and Sign out. Each state the core
/// reports has its own words and its one way on. The record stays in
/// `TraktModel` and is never shown.
struct TraktScreen: View {
    @Environment(TraktModel.self) private var trakt
    @Environment(\.metrics) private var metrics
    /// Each Connect is a new code: a fresh `traktLogin` view.
    @State private var attempt = 0

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                VStack(alignment: .leading, spacing: Theme.Space.xs) {
                    Text(caps: W.eyebrow)
                        .font(.text(Theme.type.caption, weight: .bold, relativeTo: .caption))
                        .tracking(Theme.type.caption * 0.14)
                        .foregroundStyle(Theme.Palette.accent)
                    ScreenTitle(text: W.title)
                    Text(W.lede)
                        .font(.text(Theme.type.body))
                        .foregroundStyle(Theme.Palette.muted)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if trakt.memoryOnly { Said(tone: .warn, text: W.memoryOnly) }
                if trakt.deleteFailed {
                    Said(tone: .bad, text: W.deleteFailed)
                    Button { trakt.deleteAgain() } label: { Label(W.tryAgain, glyph: "refresh-cw") }
                        .buttonStyle(PillButtonStyle())
                }
                if attempt > 0 {
                    TraktConnect(again: { attempt += 1 }, close: { attempt = 0 }).id(attempt)
                } else if let status = trakt.status {
                    TraktCard(status: status, connect: { attempt += 1 })
                    if !status.unmapped.isEmpty { unmapped(status.unmapped) }
                } else if let failure = trakt.view?.failure {
                    Said(tone: .bad, text: failure)
                } else {
                    Said(tone: .warn, text: W.loading)
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
    }

    private func unmapped(_ items: [TraktUnmapped]) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Text(W.unmappedTitle).display(Theme.type.title2).foregroundStyle(Theme.Palette.text).accessibilityAddTraits(.isHeader)
            Text(W.unmappedLede)
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
            ForEach(items, id: \.itemId) { item in
                HStack(alignment: .top, spacing: Theme.Space.s) {
                    Glyph("triangle-alert").foregroundStyle(Theme.Palette.warn)
                    VStack(alignment: .leading, spacing: Theme.Space.xs) {
                        Text(item.name)
                            .font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                            .foregroundStyle(Theme.Palette.text)
                        Text(item.reason)
                            .font(.text(Theme.type.callout, relativeTo: .callout))
                            .foregroundStyle(Theme.Palette.muted)
                    }
                }
                .padding(Theme.Space.s)
                .frame(maxWidth: .infinity, alignment: .leading)
                .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
                .accessibilityElement(children: .combine)
                #if os(tvOS)
                .focusable()
                #endif
            }
        }
    }
}

/// The card: the state's line and its facts, and its actions.
private struct TraktCard: View {
    let status: TraktStatus
    let connect: () -> Void
    @Environment(TraktModel.self) private var trakt
    @State private var busy = false
    @State private var said: (tone: StatusDot.Tone, text: String)?
    @State private var asking = false

    private var card: (line: String, detail: String?, actions: [TraktRules.Action]) {
        TraktRules.card(status, clock: TraktClock.time)
    }

    /// Connected, failing or reconnect: an account is there, with its facts.
    private var connectedish: Bool {
        switch status.state {
        case .connected, .failing, .reconnect: true
        default: false
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Said(tone: tone, text: card.line)
            if let detail = card.detail {
                Text(detail)
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
            }
            if connectedish { facts }
            if let said { Said(tone: said.tone, text: said.text) }
            FlowRow(spacing: Theme.Space.xs) {
                ForEach(Array(card.actions.enumerated()), id: \.offset) { index, action in
                    button(action, first: index == 0)
                }
            }
        }
        .padding(Theme.Space.s)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
        #if os(tvOS)
        .focusSection()
        #endif
        .confirmationDialog(W.signOut, isPresented: $asking, titleVisibility: .visible) {
            Button(W.signOut, role: .destructive) { Task { await signOut() } }
            Button(W.cancel, role: .cancel) {}
        } message: {
            Text(W.signOutAsk)
        }
    }

    private var tone: StatusDot.Tone {
        switch status.state {
        case .connected: .ok
        case .failing, .thisDeviceOff: .warn
        case .reconnect: .bad
        case .off, .notConfigured: .idle
        }
    }

    @ViewBuilder private var facts: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            fact(status.lastOkMs.map { TraktRules.fill(W.lastSynced, ["time": TraktClock.time($0)]) } ?? W.neverSynced)
            if let pending = TraktRules.pendingLine(status.pendingWrites) { fact(pending) }
            if let importing = status.importing {
                fact(TraktRules.fill(W.importing, ["done": "\(importing.done)", "total": "\(importing.total)"]))
                ProgressView(value: Double(importing.done), total: Double(max(1, importing.total)))
                    .tint(Theme.Palette.accent)
                    .accessibilityLabel(W.importLabel)
            }
            if !trakt.memoryOnly { fact(W.keptHere) }
        }
    }

    private func fact(_ words: String) -> some View {
        Text(words)
            .font(.text(Theme.type.callout, relativeTo: .callout))
            .foregroundStyle(Theme.Palette.muted)
            .fixedSize(horizontal: false, vertical: true)
    }

    @ViewBuilder private func button(_ action: TraktRules.Action, first: Bool) -> some View {
        switch action {
        case .connect, .signInHere, .signInAgain:
            Button(action: connect) { Label(label(action), glyph: "link-2") }
                .buttonStyle(PillButtonStyle(prominent: true))
        case .syncNow:
            Button { Task { await sync() } } label: { Label(busy ? W.syncing : W.syncNow, glyph: "refresh-cw") }
                .buttonStyle(PillButtonStyle(prominent: first))
                .disabled(busy)
        case .signOut:
            Button { asking = true } label: { Label(W.signOut, glyph: "log-out") }
                .buttonStyle(DestructiveTextStyle())
                .disabled(busy)
        }
    }

    private func label(_ action: TraktRules.Action) -> String {
        switch action {
        case .signInHere: W.signInHere
        case .signInAgain: W.signInAgain
        default: W.connect
        }
    }

    private func sync() async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        said = nil
        do {
            said = (.ok, TraktRules.syncedLine(try await trakt.syncNow()))
        } catch {
            said = (.bad, TraktRules.fill(W.syncFailed, ["reason": error.localizedDescription]))
        }
    }

    private func signOut() async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        said = nil
        do {
            try await trakt.signOut()
            // A record the Keychain kept is said at the top instead.
            if !trakt.deleteFailed { said = (.ok, W.signedOut) }
        } catch {
            said = (.bad, TraktRules.fill(W.signOutFailed, ["reason": error.localizedDescription]))
        }
    }
}

/// A sign-in: the code to enter on Trakt's page, until it is approved,
/// refused or runs out. The time left is not announced; the code is, once.
private struct TraktConnect: View {
    let again: () -> Void
    let close: () -> Void
    @Environment(CoreClient.self) private var core
    @Environment(\.metrics) private var metrics
    @Environment(\.openURL) private var openURL
    @State private var login: LiveRecord<TraktLoginSnapshot>?
    @State private var saidCode: String?

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            content
        }
        .padding(Theme.Space.s)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
        #if os(tvOS)
        .focusSection()
        #endif
        .task {
            let view = LiveRecord<TraktLoginSnapshot>("traktLogin", core: core)
            login = view
            view.start()
        }
        .onChange(of: code) { _, now in
            // Said once when it shows (a new code says itself again).
            if let now, now != saidCode {
                saidCode = now
                announce(TraktRules.spokenCode(now))
            }
        }
    }

    private var code: String? {
        if case .code(let userCode, _, _, _)? = login?.snapshot { userCode } else { nil }
    }

    @ViewBuilder private var content: some View {
        if let failure = login?.failure {
            Said(tone: .bad, text: TraktRules.fill(W.failed, ["reason": failure]))
            buttons(retry: true)
        } else {
            switch login?.snapshot {
            case nil, .starting?:
                Said(tone: .warn, text: W.starting)
                buttons(retry: false)
            case let .code(userCode, url, qr, expiresMs)?:
                codeStep(userCode: userCode, url: url, qr: qr, expiresMs: expiresMs)
            case .done?:
                Said(tone: .ok, text: TraktRules.loginLine(login!.snapshot!))
                Button(action: close) { Label(W.close, glyph: "circle-check") }
                    .buttonStyle(PillButtonStyle(prominent: true))
            case let other?:
                Said(tone: other == .notConfigured ? .idle : .bad, text: TraktRules.loginLine(other))
                buttons(retry: TraktRules.loginRetry(other))
            }
        }
    }

    private var showsQR: Bool {
        #if os(iOS)
        false
        #else
        true
        #endif
    }

    @ViewBuilder private func codeStep(userCode: String, url: String, qr: String, expiresMs: Int64) -> some View {
        HStack(alignment: .top, spacing: Theme.Space.m) {
            if showsQR {
                QRCode(text: qr).frame(width: metrics.wide ? 200 : 160, height: metrics.wide ? 200 : 160).accessibilityHidden(true)
            }
            VStack(alignment: .leading, spacing: Theme.Space.xs) {
                Text(showsQR ? W.scanOrOpen : W.open)
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
                Text(url.replacingOccurrences(of: "https://", with: ""))
                    .font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                    .foregroundStyle(Theme.Palette.text)
                Text(W.enterCode)
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
                Text(userCode)
                    .font(.mono(Theme.type.title1))
                    .tracking(2)
                    .foregroundStyle(Theme.Palette.text)
                    .accessibilityLabel(TraktRules.spokenCode(userCode))
                Text(W.waiting)
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
                // The time left, from the code's own deadline. Never announced: it changes every second.
                TimelineView(.periodic(from: .now, by: 1)) { context in
                    Text(TraktRules.fill(W.goodFor, ["time": TraktRules.timeLeft(expiresMs: expiresMs, nowMs: Int64(context.date.timeIntervalSince1970 * 1000))]))
                        .font(.text(Theme.type.callout, relativeTo: .callout).monospacedDigit())
                        .foregroundStyle(Theme.Palette.muted)
                }
            }
        }
        FlowRow(spacing: Theme.Space.xs) {
            #if os(iOS)
            if let page = URL(string: url) {
                Button { openURL(page) } label: { Label(W.openPage, glyph: "external-link") }
                    .buttonStyle(PillButtonStyle(prominent: true))
            }
            #endif
            Button(W.cancel, action: close).buttonStyle(PillButtonStyle())
        }
    }

    private func buttons(retry: Bool) -> some View {
        FlowRow(spacing: Theme.Space.xs) {
            if retry {
                Button(action: again) { Label(W.newCode, glyph: "refresh-cw") }
                    .buttonStyle(PillButtonStyle(prominent: true))
            }
            Button(W.cancel, action: close).buttonStyle(PillButtonStyle())
        }
    }
}

/// The player's mark under the title: Trakt and a check while the play is
/// recorded; a warning says itself on screen, not only to VoiceOver.
struct TraktMark: View {
    let trakt: TraktPlayback?

    var body: some View {
        if let mark = TraktRules.playbackMark(trakt) {
            HStack(spacing: Theme.Space.xs) {
                Glyph(mark.warn ? "triangle-alert" : "circle-check", size: Theme.type.callout)
                    .foregroundStyle(mark.warn ? Theme.Palette.warn : Theme.Palette.ok)
                Text(mark.warn ? mark.label : W.title)
                    .font(.text(Theme.type.callout, weight: .semibold, relativeTo: .callout))
                    .foregroundStyle(.white.opacity(0.85))
                    .fixedSize(horizontal: false, vertical: true)
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(mark.label)
        }
    }
}

/// Times of day on the Trakt card.
enum TraktClock {
    static func time(_ ms: Int64) -> String {
        Date(timeIntervalSince1970: TimeInterval(ms) / 1000).formatted(date: .omitted, time: .shortened)
    }
}
