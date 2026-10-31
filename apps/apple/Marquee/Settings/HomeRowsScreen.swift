import SwiftUI

/// Settings > Home rows: show or hide each catalog and set the order. Each
/// edit answers with the new list, which replaces the one on screen. Edits
/// run one at a time, in order, and each reads the list the last one left.
struct HomeRowsScreen: View {
    let core: CoreClient
    @State private var rows: [HomeCatalog]?
    @State private var failure: String?
    /// The last edit asked for; the next one waits for it.
    @State private var last: Task<Void, Never>?
    /// tvOS: the row picked up; up and down move it, select or Back drops it.
    @State private var picked: String?
    @Environment(\.metrics) private var metrics

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.s) {
                ScreenTitle(text: "Home rows")
                Text(explainer)
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.muted)
                if let failure {
                    StateLine(symbol: "triangle-alert", text: failure)
                }
                if let rows {
                    if rows.isEmpty {
                        StateLine(symbol: "list-video", text: "No addon gives Home a catalog yet.")
                    }
                    ForEach(Array(rows.enumerated()), id: \.element.rowId) { index, row in
                        HomeRowLine(
                            row: row, first: index == 0, last: index == rows.count - 1,
                            picked: picked == row.rowId, locked: picked != nil && picked != row.rowId,
                            // The row as the last reply has it, so two quick presses flip it twice.
                            toggle: {
                                edit { rows in
                                    HomeCatalog.toggled(row.rowId, in: rows).map { ("setHomeRow", ["rowId": row.rowId, "hidden": $0]) }
                                }
                            },
                            move: { by in edit { _ in ("moveHomeRow", ["rowId": row.rowId, "by": by]) } },
                            pick: { picked = picked == row.rowId ? nil : row.rowId })
                    }
                } else if failure == nil {
                    Text("Loading")
                        .textFont(Theme.type.callout, relativeTo: .callout)
                        .foregroundStyle(Theme.Palette.muted)
                }
            }
            .frame(maxWidth: 880, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .topScrim()
        #if os(tvOS)
        // Back drops a picked row instead of leaving the screen.
        .onExitCommand(perform: picked == nil ? nil : { picked = nil })
        #endif
        .task {
            do { rows = try await core.call("homeCatalogs", as: [HomeCatalog].self) } catch { failure = ErrorWords.of(error) }
        }
    }

    private var explainer: String {
        #if os(tvOS)
        "Choose what Home shows and in what order. Select a row to pick it up, move it with up and down, and select again to put it down. Continue Watching always comes first."
        #else
        "Choose what Home shows and in what order. Continue Watching always comes first."
        #endif
    }

    /// Queues an edit after the last one; `make` reads the rows as its turn
    /// comes and names the op, or nil when there is nothing left to do.
    private func edit(_ make: @escaping @MainActor ([HomeCatalog]?) -> (String, [String: any Sendable])?) {
        let previous = last
        last = Task {
            await previous?.value
            guard let (op, fields) = make(rows) else { return }
            do {
                rows = try await core.call(op, fields, as: [HomeCatalog].self)
                failure = nil
            } catch {
                failure = ErrorWords.of(error)
            }
        }
    }
}

/// A catalog: its name and addon, shown or hidden, and its place.
private struct HomeRowLine: View {
    let row: HomeCatalog
    let first: Bool
    let last: Bool
    /// tvOS: this row is picked up, or another one is (then this one waits).
    let picked: Bool
    let locked: Bool
    let toggle: () -> Void
    let move: (Int) -> Void
    let pick: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            #if os(tvOS)
            HStack(spacing: Theme.Space.s) {
                Button(action: pick) { label }
                    .buttonStyle(PillButtonStyle(prominent: picked))
                    .onMoveCommand { direction in
                        guard picked else { return }
                        if direction == .up, !first { move(-1) }
                        if direction == .down, !last { move(1) }
                    }
                    .accessibilityHint(picked ? "Up and down move it; select puts it down" : "Select to pick it up and move it")
                shown
            }
            #else
            HStack(spacing: 8) {
                label
                Button(action: toggle) { Glyph(row.hidden ? "eye-off" : "eye") }
                    .accessibilityLabel(row.hidden ? "Show \(row.title) on Home" : "Hide \(row.title) from Home")
                Button { move(-1) } label: { Glyph("arrow-up") }
                    .accessibilityLabel("Move \(row.title) up")
                    .disabled(first)
                Button { move(1) } label: { Glyph("arrow-down") }
                    .accessibilityLabel("Move \(row.title) down")
                    .disabled(last)
            }
            #endif
        }
        .buttonStyle(PillButtonStyle())
        .controlSize(.small)
        .disabled(locked)
        .padding(Theme.Space.s)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
    }

    #if os(tvOS)
    private var shown: some View {
        Button(action: toggle) {
            Label(row.hidden ? "Hidden" : "Shown", glyph: row.hidden ? "eye-off" : "eye")
        }
        .accessibilityLabel(row.hidden ? "Show \(row.title) on Home" : "Hide \(row.title) from Home")
        .disabled(picked)
    }
    #endif

    private var label: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(row.title)
                .textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                .foregroundStyle(Theme.Palette.text)
                .lineLimit(2)
                .opacity(row.hidden ? 0.55 : 1)
            Text(row.hidden ? "\(row.source), hidden from Home" : row.source)
                .textFont(Theme.type.callout, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.muted)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}
