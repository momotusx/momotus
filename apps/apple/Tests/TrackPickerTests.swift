import AetherEngine
import Foundation
import Testing

// The subtitle and audio pickers (plans/player-polish.md 2.4): the levels, the search, the default mark and one
// tap that turns a language's best option on; the web's lib/subtitles.ts rules.

private func track(_ n: Int, _ lang: String, _ language: String, _ name: String, own: String? = nil, source: String = "OpenSubtitles v3") -> SubtitleTrack {
    var t = SubtitleTrack(url: "http://127.0.0.1:1/sub/\(n).vtt", lang: lang, label: nil, source: source, forced: false, hearingImpaired: false,
                          language: language, languageName: name, name: "\(name) \(n)")
    t.nativeName = own
    return t
}

private let english1 = track(1, "eng", "en", "English")
private let english2 = track(2, "eng", "en", "English")
private let turkish = track(3, "tur", "tr", "Turkish", own: "Türkçe")
private let brazil = track(4, "pob", "pt-BR", "Portuguese (Brazil)", own: "Português (Brasil)")
private let klingon = track(5, "tlh", "tlh", "Klingon")

@Test func theFirstLevelIsTheCoresLanguagesWithTheFilesTracksUnderThem() {
    let file = [
        TrackPicker.FileTrack(index: 0, name: "English", language: "eng"),
        TrackPicker.FileTrack(index: 1, name: "Portuguese", language: "pt"),
        TrackPicker.FileTrack(index: 2, name: "German", language: "de"),
        TrackPicker.FileTrack(index: 3, name: "Subtitles 4", language: "und"),
    ]
    let languages = TrackPicker.languages([turkish, english1, english2, brazil], file: file)
    // The core's order (it puts the profile's languages first), then a language only the file has.
    // A track that names no language last, under "Unknown language", as on Android.
    #expect(languages.map(\.language) == ["tr", "en", "pt-BR", "de", ""])
    #expect(languages.map(\.name) == ["Turkish", "English", "Portuguese (Brazil)", "German", "Unknown language"])
    // The own name only where it differs.
    #expect(languages.map(\.own) == ["Türkçe", nil, "Português (Brasil)", "Deutsch", nil])
    // The file's tracks join by tag ("eng" is "en"), else by base language ("pt" joins "pt-BR"); "und" joins none.
    #expect(languages[1].file.map(\.index) == [0] && languages[2].file.map(\.index) == [1])
    #expect(languages[4].file.map(\.index) == [3])
    #expect(languages.map(\.count) == [1, 3, 2, 1, 1])
    #expect(TrackPicker.letters("") == "?" && TrackPicker.letters("tlh") == "TL")
}

@Test func theSearchFindsANameItsOwnNameOrACode() {
    let languages = TrackPicker.languages([turkish, english1, brazil], file: [])
    let find = { (q: String) in languages.filter { TrackPicker.matches($0, q) }.map(\.language) }
    #expect(find("") == ["tr", "en", "pt-BR"])
    #expect(find("eng") == ["en"])
    #expect(find("turkce") == ["tr"])
    #expect(find("Türk") == ["tr"])
    // A word of the name, not its middle.
    #expect(find("bra") == ["pt-BR"])
    #expect(find("razil") == [])
    // Codes, as typed: the tag or a spelling the tracks carry.
    #expect(find("pob") == ["pt-BR"])
    #expect(find("pt-br") == ["pt-BR"])
    #expect(find("de") == [])
}

@Test func oneTapTurnsOnTheLanguagesBestOption() throws {
    let file = [TrackPicker.FileTrack(index: 2, name: "English", language: "en")]
    let languages = TrackPicker.languages([english1, english2, turkish], file: file)
    // The file's own track first (it fits the file), else the core's best match, its first.
    #expect(TrackPicker.best(languages[0]) == .file(2))
    #expect(TrackPicker.best(languages[1]) == .track(turkish))
    #expect(TrackPicker.best(TrackPicker.languages([english2, english1], file: [])[0]) == .track(english2))
    // The language on: by the core's track playing, or the file's.
    #expect(TrackPicker.chosen(languages, subtitle: turkish, fileIndex: nil) == "tr")
    #expect(TrackPicker.chosen(languages, subtitle: nil, fileIndex: 2) == "en")
    #expect(TrackPicker.chosen(languages, subtitle: nil, fileIndex: nil) == nil)
    // One press on a language (a click, a tap, Return) turns its best option on; its chevron opens the options.
    #expect(TrackPicker.press(.language("en"), languages: languages, open: nil) == .turnOn(.file(2)))
    #expect(TrackPicker.press(.language("tr"), languages: languages, open: nil) == .turnOn(.track(turkish)))
    #expect(TrackPicker.press(.opener("tr"), languages: languages, open: nil) == .open("tr"))
    #expect(TrackPicker.press(.off, languages: languages, open: nil) == .off)
    // Every row's press goes through that one rule.
    let source = try String(contentsOf: URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .appending(path: "../Marquee/Player/SubtitlePicker.swift"), encoding: .utf8)
    #expect(source.contains("} action: { run(TrackPicker.press(.language(l.language), languages: languages, open: openLanguage)) }"))
}

@Test func theDefaultIsMarkedAndEveryLanguageHasAFlagOrItsLetters() throws {
    #expect(TrackPicker.flag("en") == "flag-en")
    #expect(TrackPicker.flag("pt-BR") == "flag-pt-br")
    // A region without its own flag takes its language's.
    #expect(TrackPicker.flag("es-AR") == "flag-es")
    #expect(TrackPicker.flag("es-419") == "flag-es-419")
    // The Chinese variants and the contested or constructed languages show the language glyph (design/flag-overrides.json).
    #expect(TrackPicker.flag("zh-Hant") == nil)
    #expect(TrackPicker.glyph("zh-Hant"))
    #expect(TrackPicker.glyph("zh"))
    #expect(TrackPicker.flag("ar") == nil)
    #expect(TrackPicker.glyph("ar-EG"))
    #expect(TrackPicker.flag("pt-MZ") == "flag-pt-mz")
    #expect(TrackPicker.flag("tlh") == nil)
    #expect(!TrackPicker.glyph("tlh"))
    #expect(TrackPicker.letters("tlh") == "TL")
    let l = TrackPicker.languages([english1, english2], file: [])[0]
    #expect(TrackPicker.spoken(l, auto: true, selected: true) == "English, Auto, 2 options, selected")
    #expect(TrackPicker.spoken(TrackPicker.languages([turkish], file: [])[0], auto: false, selected: false) == "Turkish, Türkçe, 1 option")
    // Audio: Default marks the file's default once there are two or more tracks.
    #expect(TrackPicker.marksDefault(index: 1, defaultIndex: 1, count: 2))
    #expect(!TrackPicker.marksDefault(index: 0, defaultIndex: 0, count: 1))
    #expect(!TrackPicker.marksDefault(index: 0, defaultIndex: 1, count: 2))
    // Audio: a search only past five tracks.
    #expect(!TrackPicker.audioSearch(5) && TrackPicker.audioSearch(6))
    // The default: the core's start pick reads Auto while the viewer has not chosen; the audio default reads Default.
    let player = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Player")
    let model = try String(contentsOf: player.appending(path: "PlayerModel.swift"), encoding: .utf8)
    let controls = try String(contentsOf: player.appending(path: "PlayerControls.swift"), encoding: .utf8)
    let picker = try String(contentsOf: player.appending(path: "SubtitlePicker.swift"), encoding: .utf8)
    // Auto: the core's start pick while the viewer has not chosen, its file track included.
    #expect(TrackPicker.isAuto(subtitleURL: "a", autoURL: "a", textTrack: nil, startFile: nil, viewerChose: false))
    #expect(!TrackPicker.isAuto(subtitleURL: "a", autoURL: "a", textTrack: nil, startFile: nil, viewerChose: true))
    #expect(!TrackPicker.isAuto(subtitleURL: "b", autoURL: "a", textTrack: nil, startFile: nil, viewerChose: false))
    #expect(!TrackPicker.isAuto(subtitleURL: nil, autoURL: nil, textTrack: nil, startFile: nil, viewerChose: false))
    #expect(TrackPicker.isAuto(subtitleURL: nil, autoURL: "a", textTrack: 1, startFile: 1, viewerChose: false))
    #expect(!TrackPicker.isAuto(subtitleURL: nil, autoURL: "a", textTrack: 0, startFile: 1, viewerChose: false))
    #expect(try body(of: "var subtitleIsAuto: Bool", in: model).contains("TrackPicker.isAuto("))
    #expect(model.contains("audioDefault = group.defaultOption.flatMap { group.options.firstIndex(of: $0) }"))
    // The button: "English, Auto" for the start pick, its file track too; the viewer's own pick by its name.
    #expect(TrackPicker.buttonValue(core: "English", file: nil, auto: true, autoLanguage: "English") == "English, Auto")
    #expect(TrackPicker.buttonValue(core: nil, file: "English (US) Transcribed", auto: true, autoLanguage: "English") == "English, Auto")
    #expect(TrackPicker.buttonValue(core: nil, file: "English (US) Transcribed", auto: false, autoLanguage: "English") == "English (US) Transcribed")
    #expect(TrackPicker.buttonValue(core: "Turkish", file: nil, auto: false, autoLanguage: "English") == "Turkish")
    #expect(controls.contains("picked: TrackPicker.buttonValue("))
    #expect(picker.contains("let auto = on && !second && model.subtitleIsAuto"))
    #expect(picker.contains("if isDefault { AutoMark(text: \"Default\") }"))
}

@Test func everyFlagInTheSetIsInTheCatalog() throws {
    let catalog = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Resources/Flags.xcassets")
    let sets = try FileManager.default.contentsOfDirectory(atPath: catalog.path()).filter { $0.hasSuffix(".imageset") }
    #expect(Set(sets) == Set(FlagSet.tags.map { "flag-\($0).imageset" }))
    #expect(FlagSet.tags.count == 200)
    // The mask goes (the app clips to the circle): the renderer gets flat shapes.
    let en = try String(contentsOf: catalog.appending(path: "flag-en.imageset/en.svg"), encoding: .utf8)
    #expect(!en.contains("<mask") && en.hasPrefix("<svg"))
}

@Test func aTrackInsideTheFileWinsTheStartPickInItsLanguage() throws {
    // The Sintel walk: the MP4 carries an English track, and the core's start pick is an addon's English one.
    let file = [TrackPicker.FileTrack(index: 0, name: "Spanish", language: "es"),
                TrackPicker.FileTrack(index: 1, name: "English (US) Transcribed", language: "en-US")]
    #expect(TrackPicker.startFile(english1, in: file) == 1)
    #expect(TrackPicker.startFile(turkish, in: file) == nil)
    // By tag first, else by base language; "und" never counts.
    #expect(TrackPicker.startFile(brazil, in: [TrackPicker.FileTrack(index: 3, name: "Portuguese", language: "por")]) == 3)
    #expect(TrackPicker.startFile(english1, in: [TrackPicker.FileTrack(index: 0, name: "Subtitles", language: "und")]) == nil)
    // The model applies it to the core's start pick, at once and again when the file's tracks arrive later, as the
    // start (not the viewer's pick), and marks it Auto.
    let model = try String(contentsOf: URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .appending(path: "../Marquee/Player/PlayerModel.swift"), encoding: .utf8)
    #expect(try body(of: "private func startPick(", in: model).contains("selectText(index, start: true)"))
    #expect(try body(of: "func selectText(", in: model).contains("if start { startFile = index } else { choices.choseInFile(language: textTrackLanguages[safe: index] ?? nil) }"))
    #expect(model.contains("if let track = step.turnOn {\n            startPick(track)"))
    #expect(try body(of: "private func loadTracks(", in: model).contains("if let auto = subtitle, !choices.viewerChose, auto.url == playback?.autoSubtitle { startPick(auto) }"))
    #expect(try body(of: "var subtitleIsAuto: Bool", in: model).contains("startFile: startFile"))
}

@Test func theMacKeysWalkTheLevelsAndTheTimingKeepsItsOwnOnlyWhenFocused() throws {
    let file = [TrackPicker.FileTrack(index: 0, name: "English (US) Transcribed", language: "en-US")]
    let languages = TrackPicker.languages([english1, english2, turkish], file: file)
    let first = TrackPicker.order(audio: nil, open: nil, shown: languages)
    #expect(first == [.search, .off, .language("en"), .language("tr")])
    let key = { (k: TrackPicker.Key, focus: TrackPicker.Row?, open: TrackPicker.Language?) in
        TrackPicker.key(k, focus: focus, order: TrackPicker.order(audio: nil, open: open, shown: languages), languages: languages, open: open)
    }
    // The languages: Down from the search field to Off, then the languages; Up back; Right opens the focused one,
    // its chevron too; Return turns its best on; Esc closes.
    #expect(key(.down, .search, nil) == .focus(.off))
    #expect(key(.down, .off, nil) == .focus(.language("en")))
    #expect(key(.up, .language("en"), nil) == .focus(.off))
    #expect(key(.down, .opener("en"), nil) == .focus(.language("tr")))
    #expect(key(.down, .language("tr"), nil) == .focus(.language("tr")))
    #expect(key(.right, .language("tr"), nil) == .open("tr"))
    #expect(key(.right, .opener("en"), nil) == .open("en"))
    #expect(key(.enter, .language("en"), nil) == .turnOn(.file(0)))
    #expect(key(.escape, .language("en"), nil) == .close)
    // Left and Right in the search field are the field's.
    #expect(key(.left, .search, nil) == TrackPicker.Act.none && key(.right, .search, nil) == TrackPicker.Act.none)
    // A language's options: back, the file's track, the core's; Left and Esc go back; Return turns one on.
    let en = languages[0]
    #expect(TrackPicker.order(audio: nil, open: en, shown: languages) == [.back, .option(10_000), .option(0), .option(1)])
    #expect(key(.left, .option(1), en) == .back)
    #expect(key(.escape, .option(1), en) == .back)
    #expect(key(.down, .option(0), en) == .focus(.option(1)))
    #expect(key(.enter, .option(1), en) == .turnOn(.track(english2)))
    #expect(key(.enter, .option(10_000), en) == .turnOn(.file(0)))
    // It lands on the option playing in the chosen language, else on the best.
    #expect(TrackPicker.landing(en, subtitle: english2, fileIndex: nil) == .option(1))
    #expect(TrackPicker.landing(en, subtitle: nil, fileIndex: nil) == .option(10_000))
    #expect(TrackPicker.landing(languages[1], subtitle: english2, fileIndex: nil) == .option(0))
    // Audio: no search with five tracks or fewer.
    #expect(TrackPicker.order(audio: [TrackPicker.FileTrack(index: 0, name: "English", language: "en")], open: nil, shown: []) == [.audio(0)])
    // The timing stepper in the options does not take the focus: the arrows stay the rows' until it is focused on purpose.
    let player = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Player")
    let controls = try String(contentsOf: player.appending(path: "PlayerControls.swift"), encoding: .utf8)
    let picker = try String(contentsOf: player.appending(path: "SubtitlePicker.swift"), encoding: .utf8)
    #expect(controls.contains("if takesFocus { focus = .value }"))
    #expect(picker.contains("SubtitleTimingStepper(value: value(), track: track, takesFocus: false)"))
    #expect(try body(of: "private func key(", in: picker).contains("key, focus: focus ?? current, order: order, languages: languages, open: openLanguage,"))
    // One reader stop per language: the chevron is hidden, the row carries Options.
    #expect(picker.contains(".focused($focus, equals: .opener(l.language))\n                // One stop per language for a reader: the row carries Options as an action.\n                .accessibilityHidden(true)"))
}

@Test func thePanelGrowsToTheAreaUnderTheTitleBar() {
    // The Mac walk's 760 pt window with 172 pt of controls under the panel: 478 pt, not the six-row 440.
    #expect(TrackPicker.panelHeight(window: 760, below: 172) == 478)
    // A tall window: 70 percent.
    #expect(TrackPicker.panelHeight(window: 1200, below: 172) == 840)
    // Not measured yet, or a short window: never under 320.
    #expect(TrackPicker.panelHeight(window: 0, below: 172) == 320)
}

@Test func onTheMacAnOpenPickerTakesTheBareKeysFromThePlayerAndTheMenu() throws {
    // The key walk (2026-10-23): the Playback menu's bare arrows and Esc, and the player's own, fired over an open
    // picker (Right seeked, Esc closed the player). Open, the picker holds the keys.
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    let commands = try String(contentsOf: app.appending(path: "App/AppCommands.swift"), encoding: .utf8)
    let screen = try String(contentsOf: app.appending(path: "Player/PlayerScreen.swift"), encoding: .utf8)
    let controls = try String(contentsOf: app.appending(path: "Player/PlayerControls.swift"), encoding: .utf8)
    let picker = try String(contentsOf: app.appending(path: "Player/SubtitlePicker.swift"), encoding: .utf8)
    #expect(try body(of: "private func playerKey(", in: commands).contains("guard let player, !player.pickerOpen else { return nil }"))
    // Versions over the player holds them too: the menu's Esc would close the player under it.
    #expect(screen.contains("PlayerKeys(model: model, close: close, pickerOpen: model.pickerOpen || showVersions)"))
    #expect(try body(of: "private func playerKey(", in: screen).contains("guard !showVersions, !model.pickerOpen else { return .ignored }"))
    // Esc is the picker's while it is open; else it leaves Versions, then the player.
    #expect(screen.contains(".onKeyPress(.escape) {\n            guard !model.pickerOpen else { return .ignored }\n            escape()"))
    #expect(controls.contains("model.pickerOpen = open != nil"))
    // The panel itself holds the focus (a plain button takes none without Full Keyboard Access), again after a level change.
    #expect(picker.contains(".focusable(interactions: .edit)\n            .focusEffectDisabled()\n            .focused($panelFocused)"))
    #expect(try body(of: "private func setRow(", in: picker).contains("Task { @MainActor in panelFocused = true }"))
}

@Test func onTheMacThePlayerHoldsTheKeysItWasPushedWithout() throws {
    // The owner's Mac (53312e49b): Space, Left and Right did nothing in the player. Pushed over a title page whose
    // Play is a plain button (no focus without Full Keyboard Access), the window's first responder stayed the
    // NSWindow itself, so `.focusable()` alone never took the keys. The player takes them as it shows, and again
    // as a picker or Versions lets them go.
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    let screen = try String(contentsOf: app.appending(path: "Player/PlayerScreen.swift"), encoding: .utf8)
    let controls = try String(contentsOf: app.appending(path: "Player/PlayerControls.swift"), encoding: .utf8)
    #expect(screen.contains(".focusable()\n        .focusEffectDisabled()\n        .focused($keysFocused)"))
    #expect(screen.contains("#endif\n            #if os(macOS)\n            keysFocused = true\n"))
    #expect(screen.contains(".onChange(of: model.pickerOpen || showVersions) { _, covered in if !covered { keysFocused = true } }"))
    // A picker hidden with the controls (a click on the picture) never leaves the keys with a picker that is gone,
    // and the controls never hide from under an open one.
    #expect(controls.contains(".onDisappear { model.pickerOpen = false }"))
    #expect(try body(of: "private func autoHide(", in: screen).contains("!menuOpen, !model.pickerOpen,"))
}

@Test func onTheMacEveryMouseMoveBringsTheControlsBack() throws {
    // The owner's Mac: moving the pointer over the video brought nothing back. The controls follow the window's own
    // mouse moves, as the pointer does, not SwiftUI's hover over the video; the monitor goes with the player.
    let screen = try String(contentsOf: URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .appending(path: "../Marquee/Player/PlayerScreen.swift"), encoding: .utf8)
    #expect(screen.contains("pointerMoves = NSEvent.addLocalMonitorForEvents(matching: .mouseMoved) { event in\n                touched(toggle: false)\n                return event"))
    #expect(screen.contains(".onDisappear {\n            immersion.active = false\n            #if os(macOS)\n            if let pointerMoves { NSEvent.removeMonitor(pointerMoves) }"))
    // The iPad's pointer keeps the hover.
    #expect(screen.contains("#if os(iOS)\n        .onContinuousHover { _ in touched(toggle: false) }"))
}

@Test func aPageRenderedUnderThePlayerNeverMakesASecondPlayer() throws {
    // The owner's Mac (53312e49b, `sample`): VersionsScreen.body under the player ran PlayerScreen.init, which made a
    // PlayerModel (an AVPlayer and its observers) on every render; the log held 22 AVPlayer init and dealloc pairs.
    // The model is made on first use in the body and kept in state.
    let screen = try String(contentsOf: URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .appending(path: "../Marquee/Player/PlayerScreen.swift"), encoding: .utf8)
    let initBody = try body(of: "init(route: PlayRoute, versions: VersionsModel?, core: CoreClient)", in: screen)
    #expect(initBody.contains("make = { PlayerModel(route: route, versions: versions, core: core) }"))
    #expect(!screen.contains("State(initialValue: PlayerModel("))
    #expect(try body(of: "private var model: PlayerModel", in: screen).contains("if let model = made.model { return model }"))
}

@Test func skipSaysOnlyWhatItSkipsAndStaysOffAnOpenPicker() throws {
    // The languages shot: "Skip credits" drew over the picker's Catalan row, with "Skip data: TheIntroDB" under it.
    #expect(PlayerLayout.skipShows(segment: true, failed: false, endCard: false, pickerOpen: false))
    #expect(!PlayerLayout.skipShows(segment: true, failed: false, endCard: false, pickerOpen: true))
    #expect(!PlayerLayout.skipShows(segment: true, failed: true, endCard: false, pickerOpen: false))
    #expect(!PlayerLayout.skipShows(segment: true, failed: false, endCard: true, pickerOpen: false))
    #expect(!PlayerLayout.skipShows(segment: false, failed: false, endCard: false, pickerOpen: false))
    let screen = try String(contentsOf: URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .appending(path: "../Marquee/Player/PlayerScreen.swift"), encoding: .utf8)
    #expect(try body(of: "private var skipShown: Bool", in: screen).contains("pickerOpen: model.pickerOpen"))
    // The button names no data source.
    #expect(!screen.contains("Skip data") && !screen.contains("segment.source"))
}

@Test func theEnginesFileTracksGiveThePickersTheirLanguagesAndDefault() {
    let audio = [
        TrackInfo(id: 1, name: "", codec: "eac3", language: "eng", isDefault: false),
        TrackInfo(id: 2, name: "", codec: "ac3", language: "tur", isDefault: true),
        TrackInfo(id: 3, name: "", codec: "aac", language: "und", isDefault: false),
    ]
    let text = [
        TrackInfo(id: 4, name: "", codec: "subrip", language: "fre", isDefault: false),
        TrackInfo(id: 5, name: "", codec: "hdmv_pgs_subtitle", language: nil, isDefault: false),
    ]
    let tags = AetherSession.pickerTags(audio: audio, text: text)
    // FFmpeg's ISO 639-2 tags as the file gives them; "und" and none are nil.
    #expect(tags.audio == ["eng", "tur", nil])
    #expect(tags.audioDefault == 1)
    #expect(tags.text == ["fre", nil])
    // A "fre" track joins the core's French and wins its start pick.
    let file = tags.text.enumerated().map { TrackPicker.FileTrack(index: $0.offset, name: "Subtitles \($0.offset + 1)", language: $0.element) }
    #expect(TrackPicker.startFile(track(6, "fre", "fr", "French"), in: file) == 0)
}

@Test func onTheSecondEngineTheStartPickAndTheTagsJoinAsOnAVPlayer() throws {
    let model = try String(contentsOf: URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Player/PlayerModel.swift"), encoding: .utf8)
    // Both engines' branches of selectText mark the core's start pick, not the viewer's.
    let select = try body(of: "func selectText(_ index: Int, start: Bool = false)", in: model)
    #expect(select.components(separatedBy: "if start { startFile = index } else { choices.choseInFile(language: textTrackLanguages[safe: index] ?? nil) }").count == 3)
    // The engine's tracks fill the pickers' tags each time, and the start pick runs once the file's text arrives.
    let tracks = try body(of: "private func engineTracks(", in: model)
    #expect(tracks.contains("(audioTrackLanguages, audioDefault, textTrackLanguages) = AetherSession.pickerTags(audio: tracks.audio, text: text)"))
    #expect(tracks.contains("if let auto = subtitle, !choices.viewerChose, auto.url == playback?.autoSubtitle { startPick(auto) }"))
}

@Test func anAudioTrackAndAFileOnlyLanguageCarryTheirOwnName() {
    // As a subtitle language shows "Français": the system's own-language name, capitalized.
    #expect(TrackPicker.audioOwnName(TrackPicker.FileTrack(index: 1, name: "German", language: "ger")) == "Deutsch")
    #expect(TrackPicker.audioOwnName(TrackPicker.FileTrack(index: 0, name: "French", language: "fre")) == "Français")
    // English reads the same in its own words; a track with no language has none.
    #expect(TrackPicker.audioOwnName(TrackPicker.FileTrack(index: 0, name: "English", language: "eng")) == nil)
    #expect(TrackPicker.audioOwnName(TrackPicker.FileTrack(index: 2, name: "Track 3", language: "und")) == nil)
    // A language only the file has gets its own name too.
    let only = TrackPicker.languages([], file: [TrackPicker.FileTrack(index: 0, name: "German", language: "deu")])
    #expect(only.map(\.own) == ["Deutsch"])
}

@Test func audioSearchFindsATrackByItsOwnName() {
    let german = TrackPicker.FileTrack(index: 1, name: "German", language: "ger")
    let english = TrackPicker.FileTrack(index: 0, name: "English", language: "eng")
    // "deutsch" finds German, as "turkce" finds Türkçe among the subtitles.
    #expect(TrackPicker.matches(TrackPicker.audioLanguage(german), "deutsch"))
    #expect(!TrackPicker.matches(TrackPicker.audioLanguage(english), "deutsch"))
    #expect(TrackPicker.audioLanguage(german).own == "Deutsch")
    // The file's own spelling of the code finds it too, as a subtitle language's: "nld" is in neither name.
    let dutch = TrackPicker.audioLanguage(TrackPicker.FileTrack(index: 2, name: "Dutch", language: "nld"))
    #expect(TrackPicker.matches(dutch, "nld") && TrackPicker.matches(dutch, "nl"))
}

@Test func aRegionalAudioTagTakesItsBaseLanguagesOwnName() {
    // "de-DE" beside a plain "German": "Deutsch", not "Deutsch (Deutschland)".
    #expect(TrackPicker.audioOwnName(TrackPicker.FileTrack(index: 0, name: "German", language: "de-DE")) == "Deutsch")
    // The region stays when the English label says it too.
    #expect(TrackPicker.audioOwnName(TrackPicker.FileTrack(index: 0, name: "German (Germany)", language: "de-DE")) == "Deutsch (Deutschland)")
}

@Test func anAudioTrackThatNamesNoLanguageIsNamedByItsPlaceWithNoQuestionMark() throws {
    // One: "Audio", never "Default" with a "?" nor "Original" (a dub is not); several: "Audio 1", "Audio 2".
    // The words are the shared player ones (apps/shared/stream-choice-words.json).
    #expect(TrackPicker.placeName(index: 0, count: 1, audio: true) == "Audio")
    #expect(TrackPicker.placeName(index: 0, count: 1, audio: true) == StreamChoice.words.player.audio)
    #expect(TrackPicker.placeName(index: 1, count: 3, audio: true) == "Audio 2")
    #expect(TrackPicker.placeName(index: 0, count: 1, audio: false) == "Subtitles")
    #expect(TrackPicker.placeName(index: 2, count: 3, audio: false) == "Subtitles 3")
    // No flag column when no track names its language; a blank, not "?", beside tagged ones.
    let untagged = TrackPicker.FileTrack(index: 0, name: "Audio", language: nil)
    #expect(!TrackPicker.audioFlags([untagged]))
    #expect(!TrackPicker.audioFlags([TrackPicker.FileTrack(index: 0, name: "Audio 1", language: "und")]))
    #expect(TrackPicker.audioFlags([untagged, TrackPicker.FileTrack(index: 1, name: "German", language: "ger")]))
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    let picker = try String(contentsOf: app.appending(path: "Player/SubtitlePicker.swift"), encoding: .utf8)
    let row = try body(of: "private func audioRow(", in: picker)
    #expect(row.contains("if TrackPicker.audioFlags(audio) {"))
    #expect(row.contains("} else { Color.clear.frame(width: 24, height: 24) }"))
    #expect(!row.contains("?? \"\")"))
    // Both engines name by place through the one rule.
    let model = try String(contentsOf: app.appending(path: "Player/PlayerModel.swift"), encoding: .utf8)
    #expect(model.components(separatedBy: "TrackPicker.placeName(index:").count == 3)
    #expect(!model.contains("one: \"Default\""))
}

@Test func theStartAudioIsTheFirstWantedLanguageTheFileHas() {
    // A French release's tracks: French first and default, the original English second, one untagged.
    let tracks: [String?] = ["fre", "eng", nil]
    #expect(TrackPicker.startAudio(tracks, wanted: ["en"]) == 1)
    #expect(TrackPicker.startAudio(tracks, wanted: ["de", "fr"]) == 0)
    // None of them in the file, or none wanted: the engine's own pick stays.
    #expect(TrackPicker.startAudio(tracks, wanted: ["ja"]) == nil)
    #expect(TrackPicker.startAudio(tracks, wanted: []) == nil)
    // A regional track counts with its language; "und" names none.
    #expect(TrackPicker.startAudio(["und", "pt-BR"], wanted: ["pt"]) == 1)
}
