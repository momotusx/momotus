import AppKit
import Foundation
import SwiftUI
import Testing

// First run as Poster Wall (plans/first-screens.md 3, 5, 7, L3; round2.html combo A): the words, the wall's ask, motion
// and geometry, the sheets' widths, contrast over a white wall and under TV focus, the 44 pt floor, the pause toggle,
// the profile form's focus and its untouched Continue. What needs the app's models to run is read from the sources.

private let shared = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../../shared")

private func source(_ path: String) throws -> String {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    return try String(contentsOf: app.appending(path: path), encoding: .utf8)
}

// MARK: words

@Test func theFirstRunWordsReaderHoldsEveryKeyOfTheSharedFile() throws {
    let data = try Data(contentsOf: shared.appending(path: "first-run-words.json"))
    let file = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
    #expect(Set(file.keys).subtracting(["about"]) == FirstRunWords.keys)
    let words = FirstRunWords.shared
    for (key, value) in [("welcomeTitle", words.welcomeTitle), ("getStarted", words.getStarted), ("pauseMotion", words.pauseMotion),
                         ("playMotion", words.playMotion), ("profileTitle", words.profileTitle), ("namePlaceholder", words.namePlaceholder),
                         ("startWatching", words.startWatching), ("skipImport", words.skipImport), ("continue", words.continue)] {
        #expect(value == file[key] as? String, "\(key)")
    }
    #expect(words.tvSteps == file["tvSteps"] as? [String])
}

// MARK: the wall

private actor Asks {
    var count = 0
    func add() -> Int {
        count += 1
        return count
    }
}

@MainActor @Test func theWallIsKeptOnceItCameAndAnEmptyOrFailedAskIsAskedAgain() async {
    let asks = Asks()
    let wall = WallOnce()
    let posters = ["https://art.example/1.jpg", "https://art.example/2.jpg"]
    // The core not ready yet: the ask fails, nothing is kept.
    #expect(await wall.get { _ = await asks.add(); throw CancellationError() }.isEmpty)
    // Offline or past the core's budget: empty, asked again at the next step.
    #expect(await wall.get { _ = await asks.add(); return [] }.isEmpty)
    #expect(wall.posters.isEmpty)
    #expect(await wall.get { _ = await asks.add(); return posters } == posters)
    // Kept for the app run: no fourth ask.
    #expect(await wall.get { _ = await asks.add(); return [] } == posters)
    #expect(wall.posters == posters)
    #expect(await asks.count == 3)
}

@Test func theWallDriftsOnWelcomeOnlyAndNeverUnderASheetOrWithReduceMotion() {
    #expect(WallMotion.runs(step: .welcome, reduceMotion: false, scenePhase: .active, paused: false))
    for step in [FirstRunStep.login, .profile, .who, .setup, .importing, .home, .none] {
        #expect(!WallMotion.runs(step: step, reduceMotion: false, scenePhase: .active, paused: false), "the wall drifts under \(step)")
    }
    #expect(!WallMotion.runs(step: .welcome, reduceMotion: true, scenePhase: .active, paused: false), "Reduce Motion")
    #expect(!WallMotion.runs(step: .welcome, reduceMotion: false, scenePhase: .inactive, paused: false), "the scene is not active")
    #expect(!WallMotion.runs(step: .welcome, reduceMotion: false, scenePhase: .background, paused: false), "backgrounded")
    #expect(!WallMotion.runs(step: .welcome, reduceMotion: false, scenePhase: .active, paused: true), "Pause motion")
}

@Test func everyWallReadsItsMotionFromTheOneRule() throws {
    let onboarding = try source("Onboarding/OnboardingScreen.swift")
    #expect(onboarding.contains("running: WallMotion.runs(step: shown, reduceMotion: reduceMotion, scenePhase: scenePhase, paused: paused)"))
    #expect(onboarding.contains("dimmed: step != nil && step != .welcome"))
    // Before the first step is known the wall neither drifts nor dims (P3.13).
    #expect(onboarding.contains("case nil: .none\n        case .welcome?: .welcome"))
    #expect(onboarding.contains("systemReduceMotion || WallFlags.reduceMotion"), "the walk's -MQReduceMotion is not read")
    // The step change honours Reduce Motion.
    #expect(onboarding.contains(".animation(reduceMotion ? nil : Theme.Motion.panel, value: step)"))
    // The start picker after a join and Settings' Sign in hold the wall still.
    let root = try source("App/RootView.swift")
    #expect(root.contains("running: false, dimmed: true)\n                        ProfilePicker(mode: .gate, core: core, joinedLine: line)"))
    let later = try body(of: "struct LaterSignIn: View", in: source("Account/SignInScreen.swift"))
    #expect(later.contains("running: false, dimmed: true)"))
    // Decorative, never hit; posters fade in at once under Reduce Motion; the walk's line.
    let wall = try source("Onboarding/PosterWall.swift")
    #expect(wall.contains(".allowsHitTesting(false)\n        .accessibilityHidden(true)"))
    #expect(wall.contains("withAnimation(reduceMotion ? nil : .easeOut(duration: 0.25))"))
    #expect(wall.contains("coreLog.notice(\"\\(line, privacy: .public)\")") && wall.contains("DebugHooks.note(line)"))
    #expect(onboarding.contains("core.call(\"firstRunWall\", as: [String].self)"))
}

@MainActor @Test func theDriftKeepsItsPhaseWhileTheWallHoldsStill() {
    let clock = WallClock()
    let start = Date(timeIntervalSince1970: 1000)
    clock.set(running: true, at: start)
    clock.set(running: false, at: start.addingTimeInterval(5))
    #expect(clock.elapsed(at: start.addingTimeInterval(60)) == 5, "a sheet over the wall stops the drift")
    clock.set(running: true, at: start.addingTimeInterval(100))
    #expect(clock.elapsed(at: start.addingTimeInterval(101)) == 6, "back on Welcome it goes on from where it stopped")
}

@Test func theWallHasFourColumnsOnThePhoneAndNineWideAndEmptyTilesWithoutPosters() {
    #expect(FirstRunRules.columns(.phone) == 4)
    #expect(FirstRunRules.columns(.wide) == 9)
    #expect(FirstRunRules.columns(.tv) == 9)
    let phone = WallLayout(screen: CGSize(width: 390, height: 844), columns: 4, count: 0)
    #expect(abs(phone.tile.height / phone.tile.width - 1.5) < 0.001, "2:3 posters")
    #expect(phone.poster(0, 0, in: []) == nil, "no posters: the empty tile")
    // The loop covers the wall and the lift, so a column never shows a gap.
    #expect(phone.loop >= phone.wall.height + WallLayout.lift)
    // Odd columns run the other way and start higher.
    #expect(phone.offset(0, at: 0) == 0)
    #expect(phone.offset(1, at: 0) == -phone.loop - WallLayout.lift)
    #expect(phone.offset(0, at: WallLayout.seconds(0) / 2) == -phone.loop / 2)
    // The tiles on screen at 6 s, the walk line's denominator: the turned box shows about two of the phone's four
    // columns and six of the Mac's nine, each about four tiles high; never the whole wall.
    let onPhone = phone.visible(at: 6).count
    #expect((6...16).contains(onPhone), "\(onPhone) tiles on the phone")
    let mac = WallLayout(screen: CGSize(width: 1440, height: 900), columns: 9, count: 30)
    let onMac = mac.visible(at: 6).count
    #expect((10...30).contains(onMac), "\(onMac) tiles on the Mac")
    let posters = (0..<30).map { "https://art.example/\($0).jpg" }
    #expect(Set((0..<9).map { mac.poster($0, 0, in: posters) }).count == 9, "every column starts on another poster")
}

// MARK: sheets

@Test func theSheetsTakeRound2sWidthsAndIPadAtRegularWidthTakesTheCenteredSheet() {
    #expect(FirstRunRules.sheetWidth(.login, form: .wide) == 860)
    #expect(FirstRunRules.sheetWidth(.login, form: .tv) == 980)
    #expect(FirstRunRules.sheetWidth(.setup, form: .wide) == 1140)
    for sheet in [FirstRunRules.Sheet.who, .profile, .importing] {
        #expect(FirstRunRules.sheetWidth(sheet, form: .wide) == 760)
    }
    for sheet in [FirstRunRules.Sheet.login, .who, .profile, .setup, .importing] {
        #expect(FirstRunRules.sheetWidth(sheet, form: .phone) == nil, "the phone's bottom sheet")
    }
    // iPad at regular width is the `regular` metrics (RootView), so the centered sheet; compact is the phone's.
    #expect(FirstRunRules.form(wide: Metrics.regular.wide, tv: false) == .wide)
    #expect(FirstRunRules.form(wide: Metrics.compact.wide, tv: false) == .phone)
    #expect(FirstRunRules.form(wide: true, tv: true) == .tv)
    // The grabber only where a drag does something: the phone's Login, back to Welcome.
    #expect(FirstRunRules.grabber(.login, form: .phone))
    for sheet in [FirstRunRules.Sheet.who, .profile, .setup, .importing] {
        #expect(!FirstRunRules.grabber(sheet, form: .phone), "\(sheet) has a grabber that does nothing")
    }
    #expect(!FirstRunRules.grabber(.login, form: .wide))
}

@Test func everyFirstRunControlKeepsTheFortyFourPointFloor() throws {
    #expect(FirstRunRules.Size.all.allSatisfy { $0 >= Theme.minTarget })
    let onboarding = try source("Onboarding/OnboardingScreen.swift")
    for size in ["Size.primary", "Size.toggle", "Size.segment", "Size.link"] {
        #expect(onboarding.contains("FirstRunRules.\(size)"), "\(size) is not used by first run")
    }
    let frame = try source("Onboarding/FirstRunFrame.swift")
    for size in ["Size.option", "Size.chip", "Size.grabber", "Size.link", "Size.swatch"] {
        #expect(frame.contains("FirstRunRules.\(size)"), "\(size) is not used by the sheets")
    }
    #expect(try source("Account/SignInScreen.swift").contains("FirstRunRules.Size.provider"))
}

// MARK: contrast (WCAG 2.2, 4.5:1 for text, 3:1 for large text)

private func lin(_ v: Double) -> Double { v <= 0.04045 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4) }
private func ratio(_ a: Double, _ b: Double) -> Double { (max(a, b) + 0.05) / (min(a, b) + 0.05) }

/// A grey level (0 black, 1 white) after `alpha` of `top` over `under`.
private func over(_ top: Double, _ alpha: Double, _ under: Double) -> Double { top * alpha + under * (1 - alpha) }

/// The sheet's fill (28, 28, 30 at 97 percent) over a white wall under the 62 percent dim, as a grey.
private let sheet = over(29.0 / 255, 0.97, over(0, 0.62, 1))

@Test func sheetTextKeeps4_5To1OverAWhiteWall() {
    #expect(abs(Theme.Wall.blur - 8) < 0.001)
    // White words at 92 (headings), 70 (lines), 64 (second lines), each over the sheet.
    for alpha in [0.92, 0.7, 0.64] {
        let words = over(1, alpha, sheet)
        #expect(ratio(lin(words), lin(sheet)) >= 4.5, "white at \(alpha): \(ratio(lin(words), lin(sheet)))")
    }
    // The key-blue links (#0A84FF).
    let key = 0.2126 * lin(10.0 / 255) + 0.7152 * lin(132.0 / 255) + 0.0722 * lin(1)
    #expect(ratio(key, lin(sheet)) >= 4.5, "links: \(ratio(key, lin(sheet)))")
}

@Test func welcomeWordsKeepTheirContrastOverAWhiteWall() {
    // The block from the foot (40 on the phone, 84 wide): lockup 52, 22, the title, 12, the lead, 26, the buttons.
    // Title over the ramp at the block's top (large text, 3:1); the lead (white 70) from where it starts (4.5:1).
    let screens: [(height: Double, stops: [(at: Double, alpha: Double)], foot: Double, title: Double, lead: Double, acts: Double)] = [
        (844, WallLayout.welcomePhone, 40, 3 * 39, 3 * 24, 120), (740, WallLayout.welcomePhone, 40, 3 * 39, 3 * 24, 120),
        (900, WallLayout.welcomeWide, 84, 2 * 66, 2 * 29, 54), (640, WallLayout.welcomeWide, 84, 2 * 66, 2 * 29, 54),
    ]
    for s in screens {
        let top = s.height - s.foot - (52 + 22 + s.title + 12 + s.lead + 26 + s.acts)
        let leadTop = top + 52 + 22 + s.title + 12
        let titleBack = over(0, WallLayout.shade(s.stops, at: (top + 74) / s.height), 1)
        #expect(ratio(lin(1), lin(titleBack)) >= 3, "\(s.height): title \(ratio(lin(1), lin(titleBack)))")
        let leadBack = over(0, WallLayout.shade(s.stops, at: leadTop / s.height), 1)
        #expect(ratio(lin(over(1, 0.7, leadBack)), lin(leadBack)) >= 4.5, "\(s.height): lead \(ratio(lin(over(1, 0.7, leadBack)), lin(leadBack)))")
    }
}

@Test func focusedTVControlsKeepTheirWordsDarkOnWhite() throws {
    // Under TV focus a control turns white and its words take onPrimary (black 88), its second line black 60.
    for alpha in [0.88, 0.6] {
        let words = over(0, alpha, 1)
        #expect(ratio(lin(1), lin(words)) >= 4.5, "black at \(alpha) on white: \(ratio(lin(1), lin(words)))")
    }
    let frame = try source("Onboarding/FirstRunFrame.swift")
    #expect(frame.contains("lit ? Theme.Palette.onPrimary : Theme.Palette.text"))
    #expect(frame.contains("lit ? Color.black.opacity(0.6) : Color.white.opacity(0.64)"))
}

// MARK: Welcome's toggle

@Test func thePauseToggleIsNamedByWhatItDoesAndKeptOnThisDevice() throws {
    #expect(WallMotion.toggleLabel(paused: false) == FirstRunWords.shared.pauseMotion)
    #expect(WallMotion.toggleLabel(paused: true) == FirstRunWords.shared.playMotion)
    let onboarding = try source("Onboarding/OnboardingScreen.swift")
    let toggle = try body(of: "private var pauseToggle: some View", in: onboarding)
    #expect(toggle.contains("UserDefaults.standard.set(paused, forKey: WallMotion.pausedKey)"))
    #expect(toggle.contains(".accessibilityLabel(WallMotion.toggleLabel(paused: paused))"))
    // Named by what pressing it does, so no toggle trait that VoiceOver would read against the flipped label.
    #expect(!toggle.contains(".isToggle"))
    #expect(onboarding.contains("@State private var paused = UserDefaults.standard.bool(forKey: WallMotion.pausedKey)"))
}

// MARK: Make your first profile

@Test func anUntouchedContinueSendsAnEmptyNameAndAvatar() {
    // The owner's stored avatar is "" (the letter on ember) or one an app wrote: the swatch it opens on is never sent.
    for stored in ["", "letter:ember", "popcorn:sky"] {
        #expect(FirstRunRules.profilePayload(typed: "", pressed: nil, stored: stored) == ["name": "", "avatar": ""], "\(stored)")
    }
    // Only what was typed or pressed: the name trimmed, a pressed colour on the stored picture.
    #expect(FirstRunRules.profilePayload(typed: "  Deniz ", pressed: nil, stored: "") == ["name": "Deniz", "avatar": ""])
    #expect(FirstRunRules.profilePayload(typed: "", pressed: "teal", stored: "") == ["name": "", "avatar": "letter:teal"])
    #expect(FirstRunRules.profilePayload(typed: "", pressed: "teal", stored: "popcorn:sky") == ["name": "", "avatar": "popcorn:teal"])
    #expect(FirstRunRules.profilePayload(typed: String(repeating: "a", count: 40), pressed: nil, stored: "")["name"]?.count == 30)
}

@Test func theProfileFormSendsWhatTheRuleBuildsAndNeverFocusesTheNameField() throws {
    let onboarding = try source("Onboarding/OnboardingScreen.swift")
    #expect(onboarding.contains("core.perform(\"finishProfileStep\", FirstRunRules.profilePayload(typed: typed, pressed: pressed, stored: owner?.avatar ?? \"\"))"))
    #expect(!onboarding.contains("createProfile"), "first run makes a profile")
    // TV starts on Continue; the phone, iPad and Mac on nothing, so no keyboard covers Continue.
    #expect(FirstRunRules.profileFocus(tv: true) == .continueButton)
    #expect(FirstRunRules.profileFocus(tv: false) == .none)
    #expect(onboarding.contains(".defaultFocus($onContinue, FirstRunRules.profileFocus(tv: tvForm) == .continueButton)"))
    let fields = try body(of: "struct ProfileFields: View", in: source("Onboarding/FirstRunFrame.swift"))
    #expect(!fields.contains(".focused("), "the Name field takes focus on open")
    #expect(fields.contains("TextField(words.nameLabel, text: $typed, prompt: Text(placeholder))"))
    #expect(onboarding.contains("ProfileFields(typed: $typed, placeholder: words.namePlaceholder,"), "Main is the placeholder, never a prefill")
    #expect(FirstRunRules.swatches.count == 5 && FirstRunRules.swatches.allSatisfy(ProfileRules.colors.contains))
    // The line under the heading: an account made here, or Not now and no server.
    #expect(FirstRunRules.profileLine(made: true) == FirstRunWords.shared.profileLineAccount)
    #expect(FirstRunRules.profileLine(made: false) == FirstRunWords.shared.profileLineLocal)
}

@Test func whosWatchingSaysHowThisDeviceSignedIn() {
    let words = FirstRunWords.shared
    #expect(FirstRunRules.whoLine(provider: "google", device: nil) == "Signed in with Google. Pick your profile.")
    #expect(FirstRunRules.whoLine(provider: nil, device: "Robin's iPhone") == AccountRules.fill(words.whoDevice, ["device": "Robin's iPhone"]))
    #expect(FirstRunRules.whoLine(provider: nil, device: nil) == words.whoSignedIn)
}

@Test func setupAndImportFollowThePlan() throws {
    let onboarding = try source("Onboarding/OnboardingScreen.swift")
    // The chips come from the rule, More opens today's picker; Start watching writes once and keeps what failed.
    #expect(onboarding.contains("FirstRunRules.chips(device: deviceLanguage"))
    #expect(onboarding.contains("LanguagePicker(title: words.subtitlesTitle"))
    let start = try body(of: "private func startWatching() async", in: onboarding)
    #expect(start.contains("guard !busy else { return }"), "a second press writes twice")
    #expect(start.contains("if !prefsSaved {"), "Try again writes the prefs again")
    // Skip writes no prefs; both install every absent recommended source.
    let skip = try body(of: "private func skipSetup() async", in: onboarding)
    #expect(!skip.contains("setPrefs") && skip.contains("installSources()"))
    #expect(onboarding.contains("DefaultsStep(core: core, scope: .missing).install()"))
    // Import in first run: Skip import, Done to Home after the sources.
    #expect(onboarding.contains("ImportScreen(core: core, finished: { Task { await finishImport() } }, skip: { Task { await finishImport() } })"))
    let importer = try source("Settings/ImportScreen.swift")
    #expect(importer.components(separatedBy: "skipButton").count - 1 >= 4, "Skip import is missing from waiting, failed or expired")
    // No default addon's name in any first-run step's words (D12: Import's own list and Review only).
    let words = try String(contentsOf: shared.appending(path: "first-run-words.json"), encoding: .utf8)
    for name in ["Cinemeta", "Torrentio", "OpenSubtitles"] {
        #expect(!words.contains(name) && !onboarding.contains("\"\(name)"), "\(name) on a first-run step")
    }
}

@Test func theMacFirstRunFillsTheWindow() throws {
    // The sheet takes the window's size (the wall runs edge to edge), never under its floor.
    #expect(FilmRules.macSheet(window: CGSize(width: 1440, height: 900)) == CGSize(width: 1440, height: 900))
    #expect(FilmRules.macSheet(window: CGSize(width: 720, height: 500)) == FilmRules.sheetFloor)
    // Both places that open first run ask for it, through the unchanged OnboardingScreen(core:) { }.
    for page in ["Home/HomeScreen.swift", "Discover/DiscoverScreen.swift"] {
        let text = try source(page)
        #expect(text.contains("FullScreen(isPresented: $onboarding, fillsWindow: true)"), "\(page) opens first run in a small sheet")
        #expect(text.contains("OnboardingScreen(core: core) { onboarding = false }"))
    }
}

// MARK: combo A's details

@MainActor @Test func setupsWideColumnsKeepRound2s115To1To1() throws {
    // The 1140 sheet less its 40 sides: three columns 36 apart, laid out for real.
    @MainActor final class Frames { var all: [Int: CGRect] = [:] }
    let frames = Frames()
    let columns = ShareColumns.setup {
        ForEach(0..<3, id: \.self) { i in
            Color.gray.frame(height: CGFloat(100 + i * 40))
                .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { frames.all[i] = $0 }
        }
    }
    let host = NSHostingView(rootView: columns.frame(width: 1060).frame(maxHeight: .infinity, alignment: .top))
    host.frame = CGRect(x: 0, y: 0, width: 1060, height: 600)
    host.layoutSubtreeIfNeeded()
    RunLoop.main.run(until: Date().addingTimeInterval(0.3))
    let (a, b, c) = (try #require(frames.all[0]), try #require(frames.all[1]), try #require(frames.all[2]))
    #expect(abs(a.width / b.width - 1.15) < 0.01, "the first column is \(a.width), the second \(b.width)")
    #expect(abs(b.width - c.width) < 0.5)
    #expect(abs(b.minX - a.maxX - 36) < 0.5 && abs(c.minX - b.maxX - 36) < 0.5, "the gaps are not 36")
    #expect(abs(a.minY - c.minY) < 0.5, "the columns are not aligned at the top")
    #expect(try source("Onboarding/OnboardingScreen.swift").contains("let columns = wide ? AnyLayout(ShareColumns.setup)"))
}

@Test func signInPagesPushedFromLoginAreSheetsOverTheStillWall() throws {
    // Email (both stages), Other ways, the pairing code and a recovery kit: the same sheet over the still wall, Back to Login.
    let frame = try source("Onboarding/FirstRunFrame.swift")
    let page = try body(of: "struct SignInPage<Content: View>: View", in: frame)
    #expect(page.contains("running: false, dimmed: true)\n            WallSheet(sheet: .login) {"))
    #expect(page.contains("if backShown { WallBack { dismiss() } }"))
    #expect(page.contains(".onExitCommand(perform: backShown ? { dismiss() } : nil)"), "Escape does not go back to Login")
    let screen = try source("Account/SignInScreen.swift")
    for push in ["otherWays.environment(\\.onWall, true)", "lede: words.phoneScanHow, doneOnJoin: true)\n                .environment(\\.onWall, true)",
                 "SyncJoinScreen(kit: true, server: flow.server, joined: { if flow.linkAfterJoin == nil { joined(nil) } })\n                .environment(\\.onWall, true)",
                 "RecoveryKitScreen().environment(\\.onWall, true)",
                 "EmailSignIn(flow: flow) { otherAfterPop = true; flow.back() }.environment(\\.onWall, true)",
                 "EmailCode(flow: flow, otherWays: otherWays).environment(\\.onWall, onWall)"] {
        #expect(screen.contains(push), "\(push) is not on the wall")
    }
    #expect(screen.contains("SignInPage(title: words.otherWays) {"))
    #expect(try body(of: "private struct EmailPage<Content: View>: View", in: screen).contains("SignInPage(title: title) { content }"))
    #expect(try source("Settings/PairingScreen.swift").contains("SignInPage(title: heading ?? \"Pair this device\") {"))
    let flows = try source("Sync/SyncFlows.swift")
    #expect(flows.contains("SignInPage(title: kit ? W.recoverTitle : W.startTitle) {"))
    // While a kit shows, I saved it is the way out: the sheet's Back hides as the system's does.
    #expect(flows.contains("SignInPage(title: W.kitTitle, backShown: shown == nil) {"))
}

@Test func whosWatchingWideSecondLineSaysWhatThePickerKnows() throws {
    func profile(owner: Bool = false, pin: Bool = false, kids: Bool = false) -> ProfileInfo {
        ProfileInfo(id: "p", name: "P", avatar: "", position: 0, hasPin: pin, isOwner: owner, syncing: false, kids: kids)
    }
    // No last title in the snapshot: kids, the main profile and a PIN are what the picker knows.
    #expect(FirstRunRules.whoDetail(profile(kids: true)) == FirstRunWords.shared.whoKids)
    #expect(FirstRunRules.whoDetail(profile(owner: true)) == "Main profile")
    #expect(FirstRunRules.whoDetail(profile(pin: true)) == "PIN set")
    #expect(FirstRunRules.whoDetail(profile()) == nil, "a plain profile gets no filler line")
    #expect(try source("Profiles/ProfilePicker.swift").contains("joinedLine != nil, metrics.wide, let detail = FirstRunRules.whoDetail(profile)"))
}

@Test func addProfileOnFirstRunsPickerCreatesOneNamedProfileWithTheSwatchShown() throws {
    // An empty or blank name is refused: nothing is created.
    #expect(FirstRunRules.newProfile(typed: "", swatch: "sky") == nil)
    #expect(FirstRunRules.newProfile(typed: "   ", swatch: "sky") == nil)
    #expect(FirstRunRules.newProfile(typed: "  Ada ", swatch: "sky") == ["name": "Ada", "avatar": "letter:sky"])
    #expect(FirstRunRules.newProfile(typed: String(repeating: "a", count: 40), swatch: "teal")?["name"]?.count == 30)
    // The swatches stay on the grey avatar tokens (the lead's call for every app).
    let tokens: [String: Color] = ["ember": Theme.Palette.avatarEmber, "sky": Theme.Palette.avatarSky, "teal": Theme.Palette.avatarTeal,
                                   "moss": Theme.Palette.avatarMoss, "plum": Theme.Palette.avatarPlum]
    for swatch in FirstRunRules.swatches {
        let token = try #require(tokens[swatch], "\(swatch) has no token")
        let rgb = try #require(NSColor(token).usingColorSpace(.sRGB))
        #expect(abs(rgb.redComponent - rgb.greenComponent) < 0.02 && abs(rgb.greenComponent - rgb.blueComponent) < 0.03, "\(swatch) is not grey")
    }
    // In the sheet: the tile opens the form in place, Back returns to the tiles, one press creates one profile and opens it.
    let picker = try source("Profiles/ProfilePicker.swift")
    #expect(picker.contains("Button { if joinedLine != nil { adding = true } else { showAdd = true } } label: {"))
    #expect(picker.contains("AddProfileForm(core: core) { adding = false }"))
    let form = try body(of: "private struct AddProfileForm: View", in: picker)
    #expect(form.contains("WallBack(back: back)"))
    #expect(form.contains(".disabled(busy || FirstRunRules.newProfile(typed: typed, swatch: swatch) == nil)"))
    #expect(form.contains("guard !busy, let fields = FirstRunRules.newProfile(typed: typed, swatch: swatch) else { return }"))
    #expect(form.components(separatedBy: "\"createProfile\"").count - 1 == 1, "more than one create")
    #expect(form.contains("await profiles.switchTo(made.id, pin: nil)"))
}

// MARK: review 2026-11-10-first-apple-code.md

@Test func escapeOnLoginGoesBackToWelcome() throws {
    // P1.1: SignInScreen's own Escape wins over first run's, so first run hands it the Back to Welcome.
    let onboarding = try source("Onboarding/OnboardingScreen.swift")
    #expect(onboarding.contains("back: { go(.welcome) }, notNow:"))
    #expect(FirstRunRules.back(from: .login, server: true, made: false) == .welcome)
}

/// P1.2 on a real core: the picker after a join never reopens first run on a pick.
@Suite(.serialized) @MainActor struct JoinedPickerTests {
    @Test func askingAfterAJoinEndsFirstRunAndAPickOpensTheProfile() async throws {
        let core = RealCore()
        #expect(await core.ready())
        let sync = SyncModel(core: core.client, records: FakeKeychain().record,
                             defaults: try #require(UserDefaults(suiteName: "mq-test-\(UUID().uuidString)")))
        let session = ProfileSession(core: core.client, sync: sync)
        #expect(await eventually { session.open })
        // Home's cover was open when the join came.
        session.resumeOnboarding = true
        session.askAfterJoin(line: "Signed in.")
        #expect(!session.resumeOnboarding, "a pick would open first run again")
        #expect(session.joinedWho == "Signed in.")
        #expect(!session.open, "the picker does not show")
        session.markChosen()
        #expect(session.open && session.joinedWho == nil)
        withExtendedLifetime(core) {}
    }

    @Test func firstRunClosesBeforeThePickerAndReadsFreshFromTheAddons() throws {
        let onboarding = try source("Onboarding/OnboardingScreen.swift")
        let join = try body(of: "private func afterJoin(provider: String?) async", in: onboarding)
        let closes = try #require(join.range(of: "close?()")).lowerBound
        let asks = try #require(join.range(of: "profiles.askAfterJoin(")).lowerBound
        #expect(closes < asks, "the picker is asked before first run closes")
        // A reopened cover on a profile that is no longer fresh shows nothing.
        #expect(onboarding.contains("fresh: addons.snapshot?.fresh ?? true"))
        #expect(onboarding.contains(".onChange(of: addons.snapshot?.fresh, initial: true)"))
        // P2.7: only a new problem ends the wait, and the pulled roster gets its moment; P3.9: one wait.
        #expect(join.contains("guard !pulling else { return }"))
        #expect(join.contains("let problem = sync.problem") && join.contains("profiles.snapshot == roster"))
    }
}

@Test func theReviewsSmallerFixesStayFixed() throws {
    let onboarding = try source("Onboarding/OnboardingScreen.swift")
    // P2.3: the phone's Use another device moves on after the join with no Done (checked with the pushes above).
    // P2.4: a changed answer after Import's Back is written again.
    #expect(onboarding.contains(".onChange(of: \"\\(selectedChip)/\\(best)\") { prefsSaved = false }"))
    // P2.5: Return presses Get started, Continue and Start watching; Return in the Name field is Continue.
    #expect(onboarding.components(separatedBy: ".returnKey()").count - 1 == 3)
    #expect(try body(of: "struct ProfileFields: View", in: source("Onboarding/FirstRunFrame.swift")).contains(".onSubmit(submit)"))
    #expect(onboarding.contains("submit: { Task { await finishProfile() } }"))
    // P2.6: Welcome scrolls when it is taller than the screen.
    let welcome = try body(of: "private var welcome: some View", in: onboarding)
    #expect(welcome.contains("ScrollView(.vertical)") && welcome.contains("minHeight: page.size.height, alignment: .bottom"))
    // P2.8: Add profile only for the unlocked owner.
    #expect(try source("Profiles/ProfilePicker.swift").contains("joinedLine != nil && !snapshot.locked && snapshot.current?.isOwner == true"))
    // P3.10: Import's Continue and Skip import go through one guarded finish.
    #expect(onboarding.contains("ImportScreen(core: core, finished: { Task { await finishImport() } }, skip: { Task { await finishImport() } })"))
    #expect(try body(of: "private func finishImport() async", in: onboarding).contains("guard !busy else { return }"))
    // P3.12: Get started forgets "I already use Momotus".
    #expect(onboarding.contains("returning = false\n                go(FirstRunRules.next(entry(.getStarted)))"))
    // P3.14: "Getting your profiles" holds still under Reduce Motion.
    #expect(try body(of: "private var gettingProfiles: some View", in: onboarding).contains("if reduceMotion { Glyph(\"clock\""))
    // P3.17: back on Welcome a screen reader lands on its heading.
    #expect(onboarding.contains(".accessibilityFocused($welcomeHeard)"))
}

@MainActor @Test func aResizeInTheFirstSixSecondsStillLogsTheWallLine() {
    // P3.15: a second start (a rotation's new size) is let through until the line is logged.
    let log = WallLog()
    let layout = WallLayout(screen: CGSize(width: 390, height: 844), columns: 4, count: 0)
    #expect(log.start())
    #expect(log.start(), "a size change stopped the bench line")
    let shown = layout.visible(at: 6).count
    #expect(log.settled(layout, at: 6) == "first-wall poster none tiles6s 0/\(shown)")
    #expect(!log.start(), "logged twice")
    #expect(log.settled(layout, at: 6) == nil)
}

@MainActor @Test func theWallLineCountsThePostersThatCameAfterTheFirstFrame() {
    // Mac walk 2026-11-09 L3: the line read tiles6s 0/13 on a full wall. The wall's first frame has WallOnce's empty
    // list; the count at 6 s must read the posters and art as they are then.
    let log = WallLog()
    let layout = WallLayout(screen: CGSize(width: 1440, height: 900), columns: 9, count: 0)
    #expect(log.start())
    log.show([])
    let posters = (0..<30).map { "https://art.example/\($0).jpg" }
    log.show(posters)
    for url in posters { log.drew(url) }
    let shown = layout.visible(at: 6).count
    #expect(shown > 0)
    #expect(log.settled(layout, at: 6)?.hasSuffix("tiles6s \(shown)/\(shown)") == true)
}
