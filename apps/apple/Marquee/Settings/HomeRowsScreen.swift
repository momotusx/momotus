import SwiftUI

/// One Home catalog as the Customize Home screen lists it; hidden ones sit
/// where they would show.
struct HomeCatalog: Decodable, Hashable, Sendable {
    let rowId: String
    let title: String
    let source: String
    let hidden: Bool
}

/// Settings > Home rows: show or hide each catalog and set the order. Each
/// edit answers with the new list, which replaces the one on screen.
struct HomeRowsScreen: View {
    let core: CoreClient
    @State private var rows: [HomeCatalog]?
    @State private var failure: String?
    /// tvOS: the row picked up; up and down move it, select or Back drops it.
    @State private var picked: String?
    @Environment(\.metrics) private var metrics

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.s) {
                ScreenTitle(text: "Home rows")
                Text(explainer)
                    .font(.text(Theme.type.callout, relativeTo: .callout))
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
                            toggle: { edit("setHomeRow", ["rowId": row.rowId, "hidden": !row.hidden]) },
                            move: { by in edit("moveHomeRow", ["rowId": row.rowId, "by": by]) },
                            pick: { picked = picked == row.rowId ? nil : row.rowId })
                    }
                } else if failure == nil {
                    Text("Loading")
                        .font(.text(Theme.type.callout, relativeTo: .callout))
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
            do { rows = try await core.call("homeCatalogs", as: [HomeCatalog].self) } catch { failure = error.localizedDescription }
        }
    }

    private var explainer: String {
        #if os(tvOS)
        "Choose what Home shows and in what order. Select a row to pick it up, move it with up and down, and select again to put it down. Continue Watching always comes first."
        #else
        "Choose what Home shows and in what order. Continue Watching always comes first."
        #endif
    }

    private func edit(_ op: String, _ fields: [String: any Sendable]) {
        Task {
            do {
                rows = try await core.call(op, fields, as: [HomeCatalog].self)
                failure = nil
            } catch {
                failure = error.localizedDescription
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
        .background(RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous).fill(Theme.Palette.surface))
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
                .font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                .foregroundStyle(Theme.Palette.text)
                .opacity(row.hidden ? 0.55 : 1)
            Text(row.hidden ? "\(row.source), hidden from Home" : row.source)
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.muted)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}
