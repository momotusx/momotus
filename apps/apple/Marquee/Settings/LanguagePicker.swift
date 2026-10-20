import SwiftUI

/// Every language the core knows, with search: a tap adds one at the end of
/// the profile's list or takes it off (review apple-journey P3-15).
struct LanguagePicker: View {
    let title: String
    /// Audio takes a language's base code ("pt"), subtitles the core's tag as is.
    let audio: Bool
    let core: CoreClient
    let isOn: (String) -> Bool
    let toggle: (String) -> Void
    @State private var all: [LanguageChoice] = []
    @State private var failure: String?
    @State private var query = ""
    @FocusState private var typing: Bool
    @Environment(\.metrics) private var metrics

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.s) {
                ScreenTitle(text: title)
                TextField("Search languages", text: $query)
                    .focused($typing)
                    .textFieldStyle(.plain)
                    .font(.text(Theme.type.body))
                    .autocorrectionDisabled()
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
                    .fieldChrome(focused: typing)
                    .frame(maxWidth: 560)
                if let failure {
                    StateLine(symbol: "triangle-alert", text: failure, action: ("Try again", { Task { await load() } }))
                }
                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach(LanguageChoice.rows(all, audio: audio, query: query), id: \.code) { row in
                        let on = isOn(row.code)
                        Button { toggle(row.code) } label: {
                            HStack {
                                Text(row.name).font(.text(Theme.type.body))
                                Spacer()
                                if on { Glyph("check") }
                            }
                            .foregroundStyle(Theme.Palette.text)
                            .padding(.horizontal, 14)
                            .padding(.vertical, 12)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.row)
                        .accessibilityAddTraits(on ? .isSelected : [])
                    }
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
        .task { await load() }
    }

    private func load() async {
        do {
            all = try await core.call("languages", as: [LanguageChoice].self)
            failure = nil
        } catch {
            failure = "The languages did not load: \(error.localizedDescription)"
        }
    }
}
