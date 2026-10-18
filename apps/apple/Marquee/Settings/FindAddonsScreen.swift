import SwiftUI

private typealias W = FindAddonsWords

/// Find addons (plans/m12-addon-discovery.md 3.4): Cinemeta's Official and
/// Community lists, filtered on the device, each card installing in one
/// press. Entry links may hold keys and are never shown.
struct FindAddonsScreen: View {
    let core: CoreClient
    @State private var catalog: LiveRecord<AddonCatalogSnapshot>
    /// The paste field's installs and their failure; the lists follow them.
    @State private var addons: LiveRecord<AddonsSnapshot>
    @State private var list = "official"
    @State private var filter = FindFilter.all
    @Environment(\.metrics) private var metrics

    init(core: CoreClient) {
        self.core = core
        _catalog = State(initialValue: LiveRecord("addonCatalog", core: core))
        _addons = State(initialValue: LiveRecord("addons", core: core))
    }

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                header
                paste
                VStack(alignment: .leading, spacing: 12) {
                    ChipRow(
                        label: W.listLabel,
                        chips: [Chip(id: "official", title: W.official, detail: count("official")),
                                Chip(id: "community", title: W.communityList, detail: count("community"))],
                        isSelected: { $0 == list }, select: { list = $0 })
                    ChipRow(
                        label: W.showLabel, chips: FindFilter.allCases.map { Chip(id: $0, title: $0.label) },
                        isSelected: { $0 == filter }, select: { filter = $0 })
                }
                if list == "community" { communityNote }
                tools
                content
            }
            .frame(maxWidth: 1240, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .topScrim()
        .task(id: catalog.taskId) { catalog.start() }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 8) {
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
    }

    /// Phone and Mac: the link field. TV has no field here: a row opens Addons.
    @ViewBuilder private var paste: some View {
        #if os(tvOS)
        NavigationLink { AddonsPage(core: core) } label: { Label(W.pasteLink, glyph: "link-2") }
            .buttonStyle(PillButtonStyle())
        #else
        InstallField(addons: addons)
        if let failure = addons.failure {
            StateLine(symbol: "triangle-alert", text: failure)
        }
        #endif
    }

    private var communityNote: some View {
        HStack(alignment: .top, spacing: 12) {
            Glyph("info", size: Theme.type.headline, relativeTo: .callout).foregroundStyle(Theme.Palette.text)
            Text(W.community)
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(.horizontal, metrics.wide ? 28 : 16)
        .padding(.vertical, metrics.wide ? 22 : 14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
        .accessibilityElement(children: .combine)
        .modifier(ReadableOnTV())
    }

    private var current: AddonList? { catalog.snapshot?.lists.first { $0.id == list } }

    /// The view could not open, or this list failed: its words, with Try again.
    private var failure: String? {
        if let failure = catalog.failure { return failure }
        if case .failed(let words) = current?.state { return words.isEmpty ? W.failed : words }
        return nil
    }

    /// The count beside a list's chip, once that list is in.
    private func count(_ id: String) -> String? {
        guard let list = catalog.snapshot?.lists.first(where: { $0.id == id }), list.state == .ready else { return nil }
        return "\(list.entries.count)"
    }

    /// The list's status or its Refresh, in one place above the cards.
    @ViewBuilder private var tools: some View {
        if failure == nil, current == nil || current?.state == .loading {
            StatusDot(tone: .warn, text: W.loading)
        } else if current?.state == .ready {
            Button(action: retry) { Label(W.refresh, glyph: "rotate-cw") }
                .buttonStyle(PillButtonStyle())
        }
    }

    @ViewBuilder private var content: some View {
        if let failure {
            StateLine(symbol: "triangle-alert", text: failure, action: (W.tryAgain, retry), primary: false)
        } else if let current, current.state == .ready {
            let visible = current.entries.filter(filter.shows)
            if current.entries.isEmpty {
                note(W.empty)
            } else if visible.isEmpty, let words = filter.emptyWords {
                note(words)
            } else {
                let repeated = ListedAddon.repeatedNames(current.entries)
                LazyVGrid(columns: [GridItem(.adaptive(minimum: columnWidth), spacing: gap, alignment: .top)], alignment: .leading, spacing: gap) {
                    // By key: two entries of a list can share a manifest id.
                    ForEach(visible, id: \.key) { entry in
                        FindAddonCard(entry: entry, sameName: repeated.contains(entry.name), core: core)
                    }
                }
            }
        }
    }

    private func note(_ words: String) -> some View {
        Text(words)
            .font(.text(Theme.type.callout, relativeTo: .callout))
            .foregroundStyle(Theme.Palette.muted)
            .fixedSize(horizontal: false, vertical: true)
            .modifier(ReadableOnTV())
    }

    /// Cards fill the width in as many columns as fit; a phone has one.
    private var columnWidth: CGFloat {
        #if os(tvOS)
        660
        #else
        metrics.wide ? 400 : 280
        #endif
    }

    private var gap: CGFloat { metrics.wide ? 22 : 12 }

    /// Try again and Refresh close the view and open it with `fresh`, which
    /// waits for the network instead of the addon cache. Results from before
    /// are about the old list; requests still running keep their cards.
    private func retry() {
        InstallCards.shared.forgetAll(running: false)
        catalog = LiveRecord("addonCatalog", params: ["fresh": true], core: core)
        catalog.start()
    }
}

/// A link to Find addons as a secondary pill (Addons, Add a source, onboarding).
struct FindAddonsLink: View {
    let core: CoreClient

    var body: some View {
        NavigationLink { FindAddonsScreen(core: core) } label: { Label(W.title, glyph: "package-search") }
            .buttonStyle(PillButtonStyle())
    }
}

/// One entry: its initial on the brand surface, name, host, what it
/// provides, two lines of description, then its state and action. While a
/// request runs the pressed button stays and says so, so focus and
/// VoiceOver stay on it. On TV the card is one focus stop whose select
/// performs its action.
private struct FindAddonCard: View {
    let entry: ListedAddon
    let sameName: Bool
    let core: CoreClient
    @Environment(\.metrics) private var metrics
    @Environment(\.openURL) private var openURL
    @ScaledMetric(relativeTo: .headline) private var markScale: CGFloat = 1

    private var cards: InstallCards { .shared }
    private var card: InstallCards.Card { cards[entry.key] }
    private var act: FindAction { entry.cardAction(lastCode: card.code) }

    /// The last refusal, while the card offers an action again.
    private var failure: String? {
        if card.busy { return nil }
        if case .none = act { return nil }
        return card.words
    }

    private var isTurnOn: Bool {
        if case .turnOn = act { true } else { false }
    }

    var body: some View {
        #if os(tvOS)
        Button(action: perform) { face }
            .buttonStyle(CardButtonStyle(radius: Theme.Radius.l))
            .accessibilityLabel(act.pressable ? entry.spoken(act, sameName: sameName) : entry.spoken(.none(word: act.word ?? ""), sameName: sameName))
            .accessibilityValue(card.busy ? busyWord : "")
            .accessibilityRemoveTraits(act.pressable ? [] : .isButton)
            // A result is about the state it was pressed in: a new state clears it.
            .onChange(of: entry.installed) { cards.forget(entry.key) }
        #else
        face.onChange(of: entry.installed) { cards.forget(entry.key) }
        #endif
    }

    private var face: some View {
        VStack(alignment: .leading, spacing: metrics.wide ? 20 : 12) {
            HStack(alignment: .top, spacing: metrics.wide ? 22 : 14) {
                mark
                text
            }
            if act.word != nil || act.pressable || (!isTV && act.offersConfigure) {
                FlowRow(spacing: 8) { actions }
                    .frame(maxWidth: .infinity, alignment: metrics.wide ? .trailing : .leading)
            }
        }
        .multilineTextAlignment(.leading)
        .foregroundStyle(Theme.Palette.text)
        .padding(metrics.wide ? 28 : 16)
        // As tall as the tallest card of its row; the actions keep to the top.
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .glass(RoundedRectangle(cornerRadius: metrics.wide ? Theme.Radius.l : Theme.Radius.card, style: .continuous))
    }

    private var mark: some View {
        // Decoration: it grows with the text a little, never into the text's room.
        let size = (metrics.wide ? 80 : 48) * min(markScale, 1.3)
        let shape = RoundedRectangle(cornerRadius: metrics.wide ? Theme.Radius.card : 12, style: .continuous)
        return Text(entry.initial)
            .display(metrics.wide ? 38 : 22)
            .minimumScaleFactor(0.5)
            .lineLimit(1)
            .foregroundStyle(Theme.Palette.text)
            .frame(width: size, height: size)
            .background(shape.fill(LinearGradient(colors: [Theme.Palette.raised, Theme.Palette.surface], startPoint: .topLeading, endPoint: .bottomTrailing)))
            .overlay(shape.strokeBorder(Theme.Palette.edge, lineWidth: 1))
            .accessibilityHidden(true)
    }

    private var text: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(entry.name)
                .font(.text(Theme.type.headline, weight: .bold, relativeTo: .headline))
                .natural(entry.name)
            Text(entry.host)
                .font(.mono(Theme.type.caption))
                .foregroundStyle(Theme.Palette.dim)
                .lineLimit(1)
                .truncationMode(.tail)
                .padding(.top, 4)
            Text(entry.providesLine)
                .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
                .fixedSize(horizontal: false, vertical: true)
                .padding(.top, 8)
            if let description = entry.description, !description.isEmpty {
                Text(description)
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
                    .lineLimit(2)
                    .natural(description)
                    .padding(.top, 6)
            }
            if act == .configure {
                Text(isTV ? W.configureTv : W.configureHelp)
                    .font(.text(Theme.type.callout, weight: .medium, relativeTo: .callout))
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.top, 10)
            }
            if let failure {
                StatusDot(tone: .bad, text: failure).padding(.top, 10)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder private var actions: some View {
        if let word = act.word {
            HStack(spacing: 8) {
                if case .none = act { Glyph("check", size: Theme.type.headline).foregroundStyle(Theme.Palette.ok) }
                Text(word)
            }
            .labelFont(Theme.type.callout, relativeTo: .callout)
            .foregroundStyle(act.pressable ? Theme.Palette.muted : Theme.Palette.text)
            .padding(.vertical, 12)
            .padding(.trailing, 4)
        }
        if act.pressable {
            #if os(tvOS)
            TVActionPill(busy: card.busy, glyph: glyph, word: card.busy ? busyWord : verb)
            #else
            Button(action: perform) { actionLabel(busy: card.busy) }
                .buttonStyle(PillButtonStyle())
                .accessibilityLabel(entry.spoken(act, sameName: sameName))
                .accessibilityValue(card.busy ? busyWord : "")
            #endif
        }
        #if !os(tvOS)
        if act.offersConfigure, let url = URL(string: entry.configureUrl) {
            Button { openURL(url) } label: { Label(W.configure, glyph: "external-link") }
                .buttonStyle(PillButtonStyle())
                .accessibilityLabel(entry.spoken(.configure, sameName: sameName))
        }
        #endif
    }

    private func actionLabel(busy: Bool) -> some View {
        HStack(spacing: 8) {
            if busy {
                ProgressView().controlSize(.small).tint(Theme.Palette.text)
            } else {
                Glyph(glyph)
            }
            Text(busy ? busyWord : verb)
        }
    }

    private var verb: String { isTurnOn ? W.turnOn : W.install }
    private var busyWord: String { isTurnOn ? W.turningOn : W.installing }
    private var glyph: String { isTurnOn ? "power" : "plus" }

    private var isTV: Bool {
        #if os(tvOS)
        true
        #else
        false
        #endif
    }

    /// Install or Turn on; a press while the card's request runs does nothing.
    private func perform() {
        guard !card.busy else { return }
        let (key, core) = (entry.key, core)
        switch act {
        case .install:
            Task { await cards.run(key, { try await core.perform("installListed", ["key": key]) }, failure: Self.refusal) }
        case .turnOn(let url, _):
            Task { await cards.run(key, { try await core.perform("setAddonEnabled", ["url": url, "enabled": true]) }, failure: Self.refusal) }
        case .none, .configure:
            break
        }
    }

    /// The core's words for the card, and the code that may change its action.
    nonisolated private static func refusal(_ error: any Error) -> (code: String?, words: String) {
        ((error as? CoreError)?.code, error.localizedDescription)
    }
}

#if os(tvOS)
/// The card's action drawn as a pill inside the card (the card is the
/// button): white while the card has focus, as the primary.
private struct TVActionPill: View {
    let busy: Bool
    let glyph: String
    let word: String
    @Environment(\.card) private var card

    var body: some View {
        let lit = card.focused && !busy
        HStack(spacing: 12) {
            if busy {
                ProgressView().tint(Theme.Palette.text)
            } else {
                Glyph(glyph)
            }
            Text(word)
        }
        .labelFont(Theme.type.callout, relativeTo: .callout)
        .foregroundStyle(lit ? Theme.Palette.onPrimary : Theme.Palette.text)
        .padding(.horizontal, 26)
        .padding(.vertical, 14)
        .background(Capsule().fill(lit ? AnyShapeStyle(Theme.Palette.primary) : AnyShapeStyle(Theme.controlFill)))
        .overlay { if !lit { Capsule().strokeBorder(Theme.Palette.control, lineWidth: edgeWidth) } }
    }
}
#endif

extension View {
    /// Someone else's text in its own direction (the web's `dir="auto"`):
    /// a Hebrew or Arabic name lines up at the right.
    @ViewBuilder func natural(_ text: String) -> some View {
        let aligned = frame(maxWidth: .infinity, alignment: .leading)
        if text.readsRightToLeft {
            aligned.environment(\.layoutDirection, .rightToLeft)
        } else {
            aligned
        }
    }
}
