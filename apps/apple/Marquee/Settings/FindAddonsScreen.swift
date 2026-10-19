import SwiftUI

private typealias W = FindAddonsWords

/// Where focus can be on Find addons: a list chip, a filter chip, the TV's
/// Paste a link row, Try again, Refresh, or a card by its entry key.
enum FindFocus: Hashable {
    case list(String)
    case filter(FindFilter)
    case paste
    case retry
    case refresh
    case card(String)
}

/// Find addons (plans/m12-addon-discovery.md 3.4): Cinemeta's Official and
/// Community lists, filtered on the device, each card installing in one
/// press. Entry links may hold keys and are never shown.
struct FindAddonsScreen: View {
    let core: CoreClient
    @State private var page: FindAddonsPage
    /// The paste field's installs and their failure; the lists follow them.
    @State private var addons: LiveRecord<AddonsSnapshot>
    @State private var list = "official"
    @State private var filter = FindFilter.all
    @FocusState private var focus: FindFocus?
    /// The card focus returns to when it comes back to the grid (TV).
    @State private var lastCard: String?
    @Environment(\.metrics) private var metrics

    init(core: CoreClient) {
        self.core = core
        _page = State(initialValue: FindAddonsPage(core: core))
        _addons = State(initialValue: LiveRecord("addons", core: core))
    }

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                header
                paste
                chips
                if list == "community" { communityNote }
                content
            }
            .frame(maxWidth: maxWidth, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .topScrim()
        .task(id: page.catalog.taskId) { page.catalog.start() }
        .onAppear { page.appear() }
        .onChange(of: status) { old, new in
            if let words = new.announcement(after: old) { announce(words) }
        }
        #if os(tvOS)
        .defaultFocus($focus, entryFocus)
        .onChange(of: visibleKeys) { _, keys in recover(keys) }
        .onChange(of: focus) { _, now in
            if case .card(let key) = now { lastCard = key }
        }
        // Back from a card goes to the list chips; from the chips it leaves.
        .onExitCommand(perform: onCard ? { focus = .list(list) } : nil)
        #endif
    }

    private var header: some View {
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
    }

    /// Phone and Mac: the link field. TV has no field here: a row opens Addons.
    @ViewBuilder private var paste: some View {
        #if os(tvOS)
        NavigationLink { AddonsPage(core: core) } label: { Label(W.pasteLink, glyph: "link-2") }
            .buttonStyle(PillButtonStyle())
            .focused($focus, equals: .paste)
        #else
        InstallField(addons: addons)
        if let failure = addons.failure {
            StateLine(symbol: "triangle-alert", text: failure)
        }
        #endif
    }

    /// The List chips with the list's status or Refresh beside them, then the
    /// Show chips: one block, so the first card is on the first screen.
    private var chips: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            HStack(alignment: .bottom, spacing: Theme.Space.s) {
                ChipRow(
                    label: W.listLabel,
                    chips: [Chip(id: FindFocus.list("official"), title: W.official, detail: count("official")),
                            Chip(id: FindFocus.list("community"), title: W.communityList, detail: count("community"))],
                    isSelected: { $0 == .list(list) },
                    select: { if case .list(let id) = $0 { list = id } },
                    focus: $focus)
                tools
            }
            ChipRow(
                label: W.showLabel, chips: FindFilter.allCases.map { Chip(id: FindFocus.filter($0), title: $0.label) },
                isSelected: { $0 == .filter(filter) },
                select: { if case .filter(let value) = $0 { filter = value } },
                focus: $focus)
        }
        #if os(tvOS)
        .focusSection()
        #endif
    }

    private var communityNote: some View {
        HStack(alignment: .top, spacing: Theme.Space.s) {
            Glyph("info", size: Theme.type.headline, relativeTo: .callout).foregroundStyle(Theme.Palette.text)
            Text(W.community)
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(metrics.wide ? Theme.Space.m : Theme.Space.s)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
        .accessibilityElement(children: .combine)
    }

    private var current: AddonList? { page.list(list) }

    private var status: FindStatus { page.status(of: list) }

    /// The view could not open, or this list failed: its words, with Try again.
    private var failure: String? {
        if case .failed(let words) = status { words } else { nil }
    }

    /// The count beside a list's chip, once that list is in.
    private func count(_ id: String) -> String? {
        guard let list = page.list(id), list.state == .ready else { return nil }
        return "\(list.entries.count)"
    }

    private var visible: [ListedAddon] {
        guard let current, current.state == .ready else { return [] }
        return current.entries.filter(filter.shows)
    }

    private var visibleKeys: [String] { visible.map(\.key) }

    /// Asking Cinemeta while the list loads, Refresh once it is in.
    @ViewBuilder private var tools: some View {
        if failure == nil, current == nil || current?.state == .loading {
            StatusDot(tone: .warn, text: W.loading)
                .padding(.bottom, Theme.Space.xs)
        } else if current?.state == .ready {
            // On a phone the word would push the Community chip under the edge fade.
            Button(action: retry) {
                if metrics.wide { Label(W.refresh, glyph: "rotate-cw") } else { Glyph("rotate-cw") }
            }
            .buttonStyle(PillButtonStyle())
            .accessibilityLabel(W.refresh)
            .focused($focus, equals: .refresh)
        }
    }

    @ViewBuilder private var content: some View {
        if let failure {
            ViewThatFits(in: .horizontal) {
                HStack(spacing: Theme.Space.s) { failureWords(failure); tryAgain }
                VStack(alignment: .leading, spacing: Theme.Space.s) { failureWords(failure); tryAgain }
            }
        } else if let current, current.state == .ready {
            if current.entries.isEmpty {
                note(W.empty)
            } else if visible.isEmpty, let words = filter.emptyWords {
                note(words)
            } else {
                let repeated = ListedAddon.repeatedNames(current.entries)
                let columns = [GridItem(.adaptive(minimum: columnWidth), spacing: gap, alignment: .top)]
                LazyVGrid(columns: columns, alignment: .leading, spacing: gap) {
                    // By key: two entries of a list can share a manifest id.
                    ForEach(visible, id: \.key) { entry in
                        FindAddonCard(entry: entry, sameName: repeated.contains(entry.name), core: core, focus: $focus)
                    }
                }
                #if os(tvOS)
                .focusSection()
                // Coming back to the grid lands on the card it left.
                .defaultFocus($focus, lastCard.map(FindFocus.card) ?? visibleKeys.first.map(FindFocus.card), priority: .userInitiated)
                #endif
            }
        }
    }

    private func failureWords(_ words: String) -> some View {
        Label(words, glyph: "triangle-alert")
            .font(.text(Theme.type.callout, relativeTo: .callout))
            .foregroundStyle(Theme.Palette.muted)
            .fixedSize(horizontal: false, vertical: true)
    }

    private var tryAgain: some View {
        Button(action: retry) { Label(W.tryAgain, glyph: "rotate-cw") }
            .buttonStyle(PillButtonStyle())
            .focused($focus, equals: .retry)
    }

    private func note(_ words: String) -> some View {
        Text(words)
            .font(.text(Theme.type.callout, relativeTo: .callout))
            .foregroundStyle(Theme.Palette.muted)
            .fixedSize(horizontal: false, vertical: true)
    }

    /// Cards fill the width in as many columns as fit; a phone has one, a TV two.
    private var columnWidth: CGFloat {
        #if os(tvOS)
        660
        #else
        metrics.wide ? 400 : 280
        #endif
    }

    private var maxWidth: CGFloat {
        #if os(tvOS)
        .infinity
        #else
        1240
        #endif
    }

    private var gap: CGFloat { metrics.wide ? Theme.Space.m : Theme.Space.s }

    /// Try again and Refresh (`FindAddonsPage.reload`).
    private func retry() {
        // The pressed button goes away while the list loads: focus waits on the list chip.
        focus = .list(list)
        page.reload()
    }

    #if os(tvOS)
    /// Where focus enters: the list chip once a list is in, else Try again or
    /// the Paste a link row.
    private var entryFocus: FindFocus {
        if failure != nil { return .retry }
        return current?.state == .ready ? .list(list) : .paste
    }

    private var onCard: Bool {
        if case .card = focus { true } else { false }
    }

    /// A filter or a new list took the focused or remembered card away: the
    /// first card, else the list chip.
    private func recover(_ keys: [String]) {
        if let last = lastCard, !keys.contains(last) { lastCard = keys.first }
        if case .card(let key) = focus, !keys.contains(key) {
            focus = keys.first.map(FindFocus.card) ?? .list(list)
        }
    }
    #endif
}

/// Says something to VoiceOver now (an install's result, a list that came in).
func announce(_ words: String) {
    AccessibilityNotification.Announcement(words).post()
}

/// A link to Find addons as a secondary pill (Addons, Add a source, onboarding).
struct FindAddonsLink: View {
    let core: CoreClient

    var body: some View {
        NavigationLink { FindAddonsScreen(core: core) } label: { Label(W.title, glyph: "package-search") }
            .buttonStyle(PillButtonStyle())
    }
}

/// One entry: its initial on the brand surface, name, host, two lines of
/// description, what it provides, then its state and action. VoiceOver reads
/// the card as one element with its actions, so its focus stays when Install
/// turns into Installed. On TV the card is one focus stop whose select
/// performs its action.
private struct FindAddonCard: View {
    let entry: ListedAddon
    let sameName: Bool
    let core: CoreClient
    let focus: FocusState<FindFocus?>.Binding
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

    /// The card does something when activated (iOS: Configure opens the page too).
    private var acts: Bool { act.pressable || (act == .configure && entry.configurePage != nil) }

    private var isTurnOn: Bool {
        if case .turnOn = act { true } else { false }
    }

    var body: some View {
        #if os(tvOS)
        Button(action: perform) { face }
            .buttonStyle(CardButtonStyle(radius: Theme.Radius.l))
            .focused(focus, equals: .card(entry.key))
            .accessibilityLabel(entry.cardLabel(act, sameName: sameName, tv: true))
            .accessibilityValue(spokenValue)
            .accessibilityRemoveTraits(act.pressable ? [] : .isButton)
            // A result is about the state it was pressed in: a new state clears it.
            .onChange(of: entry.installed) { cards.forget(entry.key) }
        #else
        face
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(entry.cardLabel(act, sameName: sameName, tv: false))
            .accessibilityValue(spokenValue)
            // Install, Turn on and Configure act; an installed card is not a button.
            .accessibilityAddTraits(acts ? .isButton : [])
            .accessibilityRemoveTraits(acts ? [] : .isButton)
            .accessibilityAction { perform() }
            .accessibilityActions {
                if act.offersConfigure, entry.configurePage != nil {
                    Button(W.configure, action: configure)
                }
            }
            .onChange(of: entry.installed) { cards.forget(entry.key) }
        #endif
    }

    /// After the label: Installing or the refusal, then the description.
    private var spokenValue: String {
        [card.busy ? busyWord : failure, entry.description].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: ". ")
    }

    private var face: some View {
        VStack(alignment: .leading, spacing: metrics.wide ? Theme.Space.m : Theme.Space.s) {
            HStack(alignment: .top, spacing: Theme.Space.s) {
                mark
                text
            }
            if act.word != nil || act.pressable || (!isTV && act.offersConfigure) {
                FlowRow(spacing: Theme.Space.xs) { actions }
                    .frame(maxWidth: .infinity, alignment: metrics.wide ? .trailing : .leading)
            }
        }
        .multilineTextAlignment(.leading)
        .foregroundStyle(Theme.Palette.text)
        .padding(metrics.wide ? Theme.Space.m : Theme.Space.s)
        // As tall as the tallest card of its row; the actions keep to the top.
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .glass(RoundedRectangle(cornerRadius: metrics.wide ? Theme.Radius.l : Theme.Radius.card, style: .continuous))
    }

    private var mark: some View {
        // Decoration: it grows with the text a little, never into the text's room.
        let size = (metrics.wide ? 80 : 48) * min(markScale, 1.3)
        let shape = RoundedRectangle(cornerRadius: metrics.wide ? Theme.Radius.card : Theme.Radius.m, style: .continuous)
        return Text(entry.initial)
            .display(Theme.type.title3)
            .minimumScaleFactor(0.5)
            .lineLimit(1)
            .foregroundStyle(Theme.Palette.text)
            .frame(width: size, height: size)
            .background(shape.fill(LinearGradient(colors: [Theme.Palette.raised, Theme.Palette.surface], startPoint: .topLeading, endPoint: .bottomTrailing)))
            .overlay(shape.strokeBorder(Theme.Palette.edge, lineWidth: 1))
            .accessibilityHidden(true)
    }

    private var text: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(entry.name)
                .font(.text(Theme.type.headline, weight: .bold, relativeTo: .headline))
                .natural(entry.name)
            // Two lines at most, the end kept: it tells same-name entries apart.
            Text(entry.host)
                .font(.mono(Theme.type.caption))
                .foregroundStyle(Theme.Palette.dim)
                .lineLimit(2)
                .truncationMode(.middle)
            if let description = entry.description, !description.isEmpty {
                Text(description)
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
                    .lineLimit(2)
                    .natural(description)
                    .padding(.top, 4)
            }
            Text(entry.providesLine)
                .font(.text(Theme.type.caption, relativeTo: .caption))
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.top, 4)
            if act == .configure {
                Text(isTV ? W.configureTv : W.configureHelp)
                    .font(.text(Theme.type.callout, weight: .medium, relativeTo: .callout))
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.top, Theme.Space.xs)
            }
            if let failure {
                StatusDot(tone: .bad, text: failure).padding(.top, Theme.Space.xs)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder private var actions: some View {
        if let word = act.word {
            HStack(spacing: Theme.Space.xs) {
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
            // The button stays while its request runs and reads Installing.
            Button(action: perform) { actionLabel(busy: card.busy) }
                .buttonStyle(PillButtonStyle())
            #endif
        }
        #if !os(tvOS)
        if act.offersConfigure, entry.configurePage != nil {
            Button(action: configure) { Label(W.configure, glyph: "external-link") }
                .buttonStyle(PillButtonStyle())
        }
        #endif
    }

    private func actionLabel(busy: Bool) -> some View {
        HStack(spacing: Theme.Space.xs) {
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

    /// Opens the addon's own configure page in the system browser (https only).
    private func configure() {
        if let page = entry.configurePage { openURL(page) }
    }

    /// Install or Turn on; a press while the card's request runs does nothing.
    /// VoiceOver hears how it ended.
    private func perform() {
        guard !card.busy else { return }
        let (key, name, core, cards) = (entry.key, entry.name, core, cards)
        let request: @MainActor () async throws -> Void
        let done: String
        switch act {
        case .install:
            request = { try await core.perform("installListed", ["key": key]) }
            done = W.installed
        case .turnOn(let url, _):
            request = { try await core.perform("setAddonEnabled", ["url": url, "enabled": true]) }
            // A configured copy turned on reads Set up, the listed link Installed.
            done = url == key ? W.installed : W.setUp
        case .configure:
            #if !os(tvOS)
            configure()
            #endif
            return
        case .none:
            return
        }
        Task {
            guard let result = await cards.run(key, request) else { return }
            announce(result.words ?? "\(name), \(done)")
        }
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
        HStack(spacing: Theme.Space.xs) {
            if busy {
                ProgressView().tint(Theme.Palette.text)
            } else {
                Glyph(glyph)
            }
            Text(word)
        }
        .labelFont(Theme.type.callout, relativeTo: .callout)
        .foregroundStyle(lit ? Theme.Palette.onPrimary : Theme.Palette.text)
        .padding(.horizontal, Theme.Space.m)
        .padding(.vertical, Theme.Space.s)
        .background(Capsule().fill(lit ? AnyShapeStyle(Theme.Palette.primary) : AnyShapeStyle(Theme.controlFill)))
        .overlay { if !lit { Capsule().strokeBorder(Theme.Palette.control, lineWidth: edgeWidth) } }
    }
}
#endif

extension View {
    /// Someone else's text in its own direction (the web's `dir="auto"`): a
    /// Hebrew or Arabic name lines up at the right, an English one at the
    /// left, whatever the interface's direction.
    func natural(_ text: String) -> some View {
        frame(maxWidth: .infinity, alignment: .leading)
            .environment(\.layoutDirection, text.readsRightToLeft ? .rightToLeft : .leftToRight)
    }
}
