import Foundation
import SwiftUI
import Testing

// The iPhone bar drops its words below 360 points or past a 1.3 font scale, as Android does.

@Test func theBarKeepsItsWordsAtTheDefaultSizeOnAPhone() {
    #expect(!tabBarIconsOnly(width: 402, type: .large))
    #expect(!tabBarIconsOnly(width: 360, type: .large))
}

@Test func theBarShowsIconsOnlyBelow360Points() {
    #expect(tabBarIconsOnly(width: 359, type: .large))
    #expect(tabBarIconsOnly(width: 320, type: .xSmall))
}

@Test func theBarShowsIconsOnlyPastAFontScaleOf1Point3() {
    // xxLarge is 21 over 17 (1.24), xxxLarge is 23 over 17 (1.35).
    #expect(!tabBarIconsOnly(width: 402, type: .xxLarge))
    #expect(tabBarIconsOnly(width: 402, type: .xxxLarge))
    #expect(tabBarIconsOnly(width: 402, type: .accessibility1))
    #expect(tabBarIconsOnly(width: 1024, type: .accessibility5))
}

@Test func onlyThePushedPageInFrontOfTheShownPlaceGivesGoBack() {
    #expect(canGoBack(shown: true, inFront: true, presented: true))
    // A place kept out of sight, a page with another pushed over it, and a place's root give none.
    #expect(!canGoBack(shown: false, inFront: true, presented: true))
    #expect(!canGoBack(shown: true, inFront: false, presented: true))
    #expect(!canGoBack(shown: true, inFront: true, presented: false))
    // The profile picker sheet over the places: the pages under it give none, so the sheet's own does.
    #expect(!canGoBack(shown: true, inFront: true, presented: true, sheetOver: true))
}

@Test func theSystemBackIsHiddenOutOfSightAndWhileThePageHoldsEdits() {
    #expect(!backButtonHidden(shown: true, holdsEdits: false))
    #expect(backButtonHidden(shown: false, holdsEdits: false))
    // The page's own Back asks first: no system back beside it that leaves without asking.
    #expect(backButtonHidden(shown: true, holdsEdits: true))
}

@Test func backRouteFollowsWhatTheSheetFlowSays() {
    var closed = false
    let close = SheetBack.close { closed = true }
    // Not in a sheet flow: a pushed page leaves, a root gives none.
    #expect(!backRoute(presented: true, sheet: .none).isNone && !backRoute(presented: true, sheet: .none).closes)
    #expect(backRoute(presented: false, sheet: .none).isNone)
    // A stack's root inside the profile sheet may not read as presented: Back closes the sheet, not nothing.
    let root = backRoute(presented: false, sheet: close)
    #expect(root.closes)
    if case .close(let action) = root { action() }
    #expect(closed)
    #expect(!backRoute(presented: true, sheet: close).closes && !backRoute(presented: true, sheet: close).isNone)
    // Onboarding keeps Back to itself: none, presented or not.
    #expect(backRoute(presented: true, sheet: .off).isNone)
    #expect(backRoute(presented: false, sheet: .off).isNone)
}

@Test func theSheetHelperTellsThePagesUnderItAndGivesItsFlowACloseAction() throws {
    // Read from the sources: the pages and the sheet are SwiftUI views in a window.
    let file = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/App/RootView.swift")
    let root = try String(contentsOf: file, encoding: .utf8)
    let start = try #require(root.range(of: "struct FullScreen"))
    let helper = try body(of: "func body(content: Content) -> some View", in: String(root[start.lowerBound...]))
    // Written on the content, before the sheet: the sheet's flow never sees it. The flow gets the close action.
    let over = try #require(helper.range(of: ".environment(\\.sheetOver, sheetCovers(outer: outerSheetOver, own: presented.wrappedValue))"), "the pages under a sheet are not told")
    let sheet = try #require(helper.range(of: ".sheet(isPresented: presented)"))
    let close = helper.range(of: ".environment(\\.sheetBack, sheetBack)")
    #expect(String(root[start.lowerBound...]).contains("closesOnBack ? .close({ isPresented = false }) : .off"), "the close action no longer closes the sheet")
    #expect(close != nil, "the sheet's flow cannot close it")
    #expect(over.lowerBound < sheet.lowerBound && sheet.lowerBound < (close?.lowerBound ?? helper.startIndex))
    #expect(root.contains("presented: !route.isNone, sheetOver: sheetOver"), "a page under the sheet gives Back again")
    #expect(root.contains("backRoute(presented: presented, sheet: sheetBack)"))
    #expect(root.contains("} else if case .close(let close) = route {\n                close()"), "Back no longer runs the sheet's close action")
}

@Test func aSheetOverTheShellStaysOnWhenAnInnerWriterSetsItsOwn() {
    // The profiles sheet over a title whose synopsis is closed: the title gives no Back.
    #expect(sheetCovers(outer: true, own: false))
    #expect(!canGoBack(shown: true, inFront: true, presented: true, sheetOver: sheetCovers(outer: true, own: false)))
    #expect(sheetCovers(outer: false, own: true))
    #expect(!sheetCovers(outer: false, own: false))
}

@Test func theTitleClosesItsSynopsisSheetOnBackAndNeverWritesTheShellsSheetFlag() throws {
    let file = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Title/TitleScreen.swift")
    let title = try String(contentsOf: file, encoding: .utf8)
    #expect(title.contains(".environment(\\.leaveAsk, synopsisClose)"), "Cmd-[ pops the title under its own sheet")
    #expect(title.contains("return { reading = false }"))
    // A writer of its own would override the shell's value for the title.
    #expect(!title.contains(".environment(\\.sheetOver"), "the title overrides the sheet flag the shell wrote")
}
@Test func onlyTheProfileSheetClosesOnBack() throws {
    // Read from the sources: the presenters are SwiftUI views.
    let dir = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    let root = try String(contentsOf: dir.appending(path: "App/RootView.swift"), encoding: .utf8)
    let start = try #require(root.range(of: "struct FullScreen"))
    let helper = String(root[start.lowerBound...])
    #expect(helper.contains("var closesOnBack = false"), "a sheet closes on Back unless asked")
    #expect(helper.contains("closesOnBack ? .close({ isPresented = false }) : .off"), "a flow that does not close on Back is not told to keep it")
    #expect(root.contains("canGoBack(shown: shown, inFront: inFront, presented: !route.isNone, sheetOver: sheetOver)"))
    #expect(root.contains("FullScreen(isPresented: $showProfiles, closesOnBack: true)"), "the profile sheet lost its close on Back")
    for file in ["Home/HomeScreen.swift", "Discover/DiscoverScreen.swift"] {
        let source = try String(contentsOf: dir.appending(path: file), encoding: .utf8)
        #expect(source.contains("FullScreen(isPresented: $onboarding, fillsWindow: true)"), "\(file) presents onboarding another way")
        #expect(!source.contains("closesOnBack"), "onboarding in \(file) closes on Back")
    }
}

@Test func onlyAPageInsideASheetsFlowIsInASheet() {
    #expect(!SheetBack.none.isInSheet)
    #expect(SheetBack.close({}).isInSheet)
    #expect(SheetBack.off.isInSheet)
}

// Settings has no bar item on any platform: the profile's menu at the bar's far right opens it (plans/nav-profile.md).
@Test func noBarHoldsSettings() {
    #expect(!AppTab.bar.contains(.settings))
    #expect(AppTab.bar.filter(\.isTool) == [.search])
    // The Go menu keeps every place, Settings with its key.
    #expect(AppTab.allCases.contains(.settings))
}

@Test func tvFocusEntersTheBarOnTheProfileForSettings() {
    #expect(AppTab.settings.barFocus == nil)
    for tab in AppTab.bar { #expect(tab.barFocus == tab) }
}
