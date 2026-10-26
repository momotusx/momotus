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
    #expect(root.contains(".environment(\\.heroCovered, immersion.active || !homePath.isEmpty)"))
    #expect(root.contains(".environment(\\.tabShown, item == tab)"))
    let home = try source("Home/HomeScreen.swift")
    #expect(home.contains(".environment(\\.heroCovered, covered || showProfiles || showAccount || showHeroVersions || playing != nil || reviewing || onboarding)"))
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
