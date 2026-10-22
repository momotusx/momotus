#if os(macOS)
import SwiftUI

extension FocusedValues {
    /// The window's place switch; nil while the player covers the places.
    @Entry var places: OpenTabAction?
    /// Cmd-F: Search, with the cursor in its field every time.
    @Entry var find: OpenTabAction?
    /// The player on screen, for the Playback menu.
    @Entry var playerKeys: PlayerKeys?
    /// The title page in front, when Start Over can play it from the beginning (`startOverAvailable`).
    @Entry var startOver: StartOverAction?
}

/// Plays the title in front from its beginning.
struct StartOverAction {
    let run: @MainActor () -> Void
}

/// What the Playback menu acts on: the player and its way out.
struct PlayerKeys {
    let model: PlayerModel
    let close: @MainActor () -> Void
}

/// The Mac's menus: Settings, Search and the places, the player's keys, and no
/// second window (one window runs the one core, its onboarding and its Now
/// Playing; review apple-journey P2-7).
struct AppCommands: Commands {
    @FocusedValue(\.places) private var places
    @FocusedValue(\.find) private var find
    @FocusedValue(\.playerKeys) private var player
    @FocusedValue(\.startOver) private var startOver

    var body: some Commands {
        CommandGroup(replacing: .newItem) {}
        CommandGroup(replacing: .appSettings) {
            Button("Settings…") { places?.open(.settings) }
                .keyboardShortcut(",")
                .disabled(places == nil)
        }
        CommandGroup(after: .textEditing) {
            Button("Search Titles") { find?.open(.search) }
                .keyboardShortcut("f")
                .disabled(find == nil)
        }
        CommandMenu("Go") {
            ForEach(Array(AppTab.allCases.enumerated()), id: \.element) { index, tab in
                Button(tab.title) { places?.open(tab) }
                    .keyboardShortcut(KeyEquivalent(Character(String(index + 1))))
            }
            .disabled(places == nil)
        }
        CommandMenu("Playback") {
            // A title page's resume point, outside the player's group so it is live on the page.
            Button("Start Over") { startOver?.run() }
                .keyboardShortcut("r", modifiers: [.command, .shift])
                .disabled(startOver == nil)
            Divider()
            Group {
                Button(player?.model.playing == true ? "Pause" : "Play") { player?.model.togglePlay() }
                    .keyboardShortcut(bare(.space))
                Button("Back 10 Seconds") { player?.model.skip(-10) }
                    .keyboardShortcut(bare(.leftArrow))
                Button("Forward 10 Seconds") { player?.model.skip(10) }
                    .keyboardShortcut(bare(.rightArrow))
                if let model = player?.model { subtitles(model) }
                Divider()
                Button("Close Player") { player?.close() }
                    .keyboardShortcut(bare(.escape))
            }
            .disabled(player == nil)
        }
    }

    /// A key alone, only while the player is up: with no player a text field
    /// keeps its spaces and arrows.
    private func bare(_ key: KeyEquivalent) -> KeyboardShortcut? {
        player == nil ? nil : KeyboardShortcut(key, modifiers: [])
    }

    /// Off, the profile's languages, then the file's own tracks; the rest
    /// stays in the player's Subtitles menu.
    private func subtitles(_ model: PlayerModel) -> some View {
        Menu("Subtitles") {
            Toggle("Off", isOn: Binding(get: { model.subtitle == nil && model.textTrack == nil }, set: { _ in model.chooseSubtitle(nil) }))
            let groups = SubtitleMenu.groups(model.playback?.subtitles ?? [], preferred: model.subtitleLanguages)
            ForEach(groups.top, id: \.language) { group in
                Menu(group.heading) {
                    ForEach(group.tracks) { track in
                        Toggle(SubtitleMenu.title(track), isOn: Binding(get: { model.subtitle == track }, set: { _ in model.chooseSubtitle(track) }))
                    }
                }
            }
            if !model.textTracks.isEmpty {
                Section("In the File") {
                    ForEach(Array(model.textTracks.enumerated()), id: \.offset) { index, name in
                        Toggle(name, isOn: Binding(get: { model.textTrack == index }, set: { _ in model.selectText(index) }))
                    }
                }
            }
        }
    }
}
#endif
