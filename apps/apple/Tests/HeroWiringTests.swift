import Foundation
import Testing

// The hero's wiring, read from the sources (review r2, R2-2): the rules are tested as values in HeroPagerTests,
// these pin the lines that feed them, so Home behind another tab, under a page or the player, or under VoiceOver
// can never page or play again by a lost line.

private func source(_ path: String) throws -> String {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    return try String(contentsOf: app.appending(path: path), encoding: .utf8)
}

@Test func theCarouselPagesAndPlaysOnlyWhileLive() throws {
    let carousel = try source("Home/HeroCarousel.swift")
    // The tab shown and what covers Home reach the rule.
    #expect(carousel.contains("@Environment(\\.tabShown) private var tabShown"))
    #expect(carousel.contains("@Environment(\\.heroCovered) private var covered"))
    #expect(carousel.contains("HeroPagerModel.live(visible: visible, tabShown: tabShown, covered: covered)"))
    // The clock, the trailer's task and its start all go by `live`, not by the scroll view's visibility alone.
    let running = try body(of: "private var running: Bool", in: carousel)
    #expect(running.contains("visible: live,"))
    #expect(running.contains("assistive: voiceOver || switchControl"))
    #expect(carousel.contains("TrailerKey(page: page ?? heroes.first?.tile.id, visible: live, active: shown)"))
    let start = try body(of: "private func startTrailer() async", in: carousel)
    #expect(start.contains("live, shown else { return }"))
    // Leaving the screen, or previews turned off, lets the player go.
    #expect(carousel.contains(".onChange(of: live) { _, now in if !now { TrailerPlayer.shared.release() } }"))
    #expect(carousel.contains(".onChange(of: previewsOn) { _, on in if !on { TrailerPlayer.shared.release() } }"))
    #expect(carousel.contains(".onDisappear { TrailerPlayer.shared.release() }"))
}

@Test func theShellAndHomeSayWhatCoversTheHero() throws {
    let root = try source("App/RootView.swift")
    // The profile picker the bar's menu opens over the places covers it too (plans/nav-profile.md), and so does the
    // Mac's profile panel under its dim layer (plans/profile-panel.md).
    #expect(root.contains(".environment(\\.heroCovered, immersion.active || !homePath.isEmpty || showProfiles || showPanel)"))
    #expect(root.contains(".environment(\\.tabShown, item == tab)"))
    let home = try source("Home/HomeScreen.swift")
    #expect(home.contains(".environment(\\.heroCovered, covered || showAccount || showHeroVersions || playing != nil || reviewing || onboarding)"))
}

@Test func theSettingsSwitchWritesTheKeyTheTrailerReads() throws {
    let settings = try source("Settings/SettingsScreen.swift")
    let trailer = try source("Home/HeroTrailer.swift")
    #expect(settings.contains("@AppStorage(HeroTrailers.previewsKey) private var previewsOn = true"))
    #expect(trailer.contains("defaults.object(forKey: HeroTrailers.previewsKey) as? Bool ?? true"))
}

@Test func theMacHeroTakesAClickAndDrawsOnlyTheDesignsRingForTheKeyboard() throws {
    // Review r2, R2-1: owed a 30 s foreground check (a click, Right, Tab); these pin what can be read without one.
    let carousel = try source("Home/HeroCarousel.swift")
    #expect(carousel.contains(".focusable(interactions: .edit)"))
    #expect(carousel.contains(".focusEffectDisabled()"))
    #expect(carousel.contains(".modifier(FocusRing(shape: RoundedRectangle(cornerRadius: 14, style: .continuous), active: heroFocus))"))
    #expect(carousel.contains(".onKeyPress(.rightArrow) { key(1) }"))
    // The ring shows for keyboard navigation only, never for a click.
    #expect(!FocusRule.shown(focused: true, keyboardNavigation: false))
    #expect(FocusRule.shown(focused: true, keyboardNavigation: true))
}

@Test func theHerosPlayIsTheCoresInItsWords() throws {
    // plans/appletv-polish.md 18: a page draws Play only with the core's target, labelled as the title page's.
    let view = try source("Home/HeroView.swift")
    let button = try body(of: "@ViewBuilder private func playButton(_ hero: Hero) -> some View", in: view)
    #expect(button.contains("if let target = hero.play {"))
    #expect(button.contains("HeroPlayLabel(text: target.label, wide: metrics.wide, pages: playLabels)"))
    #expect(try source("Home/HeroCarousel.swift").contains("playLabels: heroes.compactMap { $0.play?.label }"))
    #expect(view.contains(".modifier(StartFocus(scope: hero.play == nil ? startScope : nil))"))
    // Home plays the target's video from where it stopped.
    let play = try body(of: "private func playHero(_ hero: Hero)", in: try source("Home/HomeScreen.swift"))
    #expect(play.contains("guard let play = hero.play else { return }"))
    #expect(play.contains("videoId: play.videoId"))
    #expect(play.contains("resumeMs: play.resumeMs"))
}

@MainActor @Test func theSoundSwitchSitsAtTheHerosFootAndKeepsTheSessionsChoice() throws {
    let home = try source("Home/HomeScreen.swift")
    // One switch, at the bottom trailing corner on every screen that plays trailers, none beside the profile.
    #expect(home.components(separatedBy: "TrailerMute()").count == 2)
    let corner = try #require(home.range(of: ".overlay(alignment: .bottomTrailing) {"))
    let after = home[corner.upperBound...]
    // Drawn from the moment the trailer plays, when a kept sound is heard (review r4, W2).
    #expect(after.hasPrefix("\n                            if TrailerState.shared.playing && !walkMuted {\n                                TrailerMute()"))
    #expect(home.contains(".padding(.bottom, HeroFoot.soundSwitch(wide: metrics.wide))"))
    // Its middle level with the dots'.
    #expect(try source("Home/HeroCarousel.swift").contains(".padding(.bottom, HeroFoot.dots(wide: metrics.wide))"))
    for wide in [false, true] {
        let middle = HeroFoot.soundSwitch(wide: wide) + HeroFoot.switchSize / 2
        #expect(middle == HeroFoot.dots(wide: wide) + HeroFoot.dotsHeight / 2)
    }
    // A toggle under a fixed name, on or off; the choice is the session's, so no switch keeps its own.
    let mute = try body(of: "struct TrailerMute: View", in: try source("Home/HeroCarousel.swift"))
    #expect(mute.contains(".accessibilityLabel(\"Trailer sound\")"))
    #expect(mute.contains(".accessibilityAddTraits(.isToggle)"))
    #expect(mute.contains(".accessibilityValue(state.sound ? \"On\" : \"Off\")"))
    #expect(!mute.contains(".isSelected"))
    #expect(!mute.contains("@State"))
    let trailer = try source("Home/HeroTrailer.swift")
    // A preview keeps the session's sound; the full-screen trailer brings its own (plans/trailer-inline.md 7).
    #expect(trailer.contains("if message[\"full\"] as? Bool != true, TrailerState.shared.sound { sound(true) }"))
    // Muted at launch; only a press turns it on.
    #expect(!TrailerState().sound)
    #expect(trailer.components(separatedBy: "sound = true").count == 1)
    #expect(try source("Home/HeroPager.swift").components(separatedBy: "sound = true").count == 1)
}

@Test func aLatePlayKeepsItsPlaceAndNeverTakesTheTVFocus() throws {
    // Review r4: while the core reads a page's episodes, Play's place is held, hidden and out of reach, so Details
    // never moves when Play lands.
    let button = try body(of: "@ViewBuilder private func playButton(_ hero: Hero) -> some View", in: try source("Home/HeroView.swift"))
    let held = try #require(button.range(of: "} else if hero.holdsPlaySlot {"))
    let slot = button[held.upperBound...]
    #expect(slot.contains(".buttonStyle(PillButtonStyle(prominent: true))"))
    #expect(slot.contains(".disabled(true)"))
    #expect(slot.contains(".hidden()"))
    #expect(slot.contains(".accessibilityHidden(true)"))
    // TV: the start focus lands once and stays. Home resets it only for Back to the top, never when Play lands.
    let home = try source("Home/HomeScreen.swift")
    #expect(home.components(separatedBy: "resetFocus(in: startScope)").count == 2)
    #expect(try body(of: ".onChange(of: topRequest)", in: home).contains("resetFocus(in: startScope)"))
    #expect(!home.contains("hero?.play"))
}

@MainActor @Test func aFilmStartingInThePlayerTurnsTheTrailerSoundOff() throws {
    // The lead, after review r4: the sound stays through tabs and pages, but coming back from a film is never loud.
    let state = TrailerState()
    state.sound = true
    state.loading = true
    state.filmStarts()
    #expect(!state.sound, "a film's start mutes the next trailer")
    #expect(state.loading, "and leaves the rest of the trailer's state alone")
    // Every source of every title passes through the player's one load.
    let load = try body(of: "private func load(_ url: String, key: String?, attempt: Int64?, at start: Double)", in: try source("Player/PlayerModel.swift"))
    #expect(load.contains("TrailerState.shared.filmStarts()"))
}

@Test func theDotsAreOnePageControlOfAtLeast44PointsAndATapGoesToTheNearestDot() throws {
    // HIG: a 44 pt target; the dots' 16 pt pitch is one control's, as iOS's page control.
    #expect(PageDots.side >= 44)
    // Three dots (56 pt) on page one: middles at 12, 36.5 and 52.5.
    #expect(PageDots.nearest(0, count: 3, active: 0) == 0)
    #expect(PageDots.nearest(30, count: 3, active: 0) == 1)
    #expect(PageDots.nearest(60, count: 3, active: 0) == 2)
    // Two dots (40 pt) sit 2 pt into the 44 pt control.
    #expect(PageDots.nearest(38, count: 2, active: 0) == 1)
    #expect(PageDots.nearest(0, count: 2, active: 1) == 0)
    let dots = try body(of: "struct HeroDots: View", in: try source("Home/HeroCarousel.swift"))
    #expect(!dots.contains("Button"))
    #expect(dots.contains(".frame(minWidth: PageDots.side, minHeight: PageDots.side)"))
    #expect(dots.contains(".accessibilityElement(children: .ignore)"))
    #expect(dots.contains(".accessibilityAdjustableAction"))
}

/// QA r2, Q2 row 33: an urgent account alert that lands after Home opened moved the hero down. It lies over the hero's
/// top instead, and is the list's first item only where there is no hero to lie over.
@Test func theUrgentAlertLiesOverTheHeroAndTakesNoRoom() throws {
    let home = try source("Home/HomeScreen.swift")
    let stack = try body(of: "var body: some View", in: home)
    #expect(stack.contains("hero\n                        // Over the top of the hero's art"))
    #expect(stack.contains(".overlay(alignment: .topLeading) { alert }"))
    #expect(stack.contains("if model.needsSetup || model.allFailed { alert }"))
    #expect(!stack.contains("AccountCard(top: true)"))
}
