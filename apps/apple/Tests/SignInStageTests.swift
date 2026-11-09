import AppKit
import Foundation
import SwiftUI
import Testing

// The Login sheet's two columns on wide forms (plans/first-screens.md 3.3, D10, round2.html combo A), laid out for
// real: the ways on the left, the phone's live QR on the right at 260, under the ways in a narrow sheet. The screen's
// wiring (the code asked as the step opens, the providers' own buttons on every form, Escape) is read from its source,
// which needs the app's models to run.

@MainActor private final class Frames {
    var block = CGRect.zero
    var corner = CGRect.zero
}

/// The columns in a sheet whose content is `width` wide, with stand-ins of the ways' and the QR's sizes.
@MainActor private func staged(width: CGFloat) -> Frames {
    let frames = Frames()
    let stage = SignInStage(placed: { frames.block = $0; frames.corner = $1 }) {
        Color.gray.frame(height: 320)
    } corner: {
        Color.white.frame(height: 300)
    }
    let host = NSHostingView(rootView: stage.frame(width: width).frame(maxHeight: .infinity, alignment: .top))
    host.frame = CGRect(x: 0, y: 0, width: width, height: 900)
    host.layoutSubtreeIfNeeded()
    RunLoop.main.run(until: Date().addingTimeInterval(0.3))
    return frames
}

@MainActor @Test func theWaysSitLeftAndTheCodeRightInTheWideSheet() {
    // The 860 sheet less its 40 sides.
    let f = staged(width: 780)
    #expect(f.block.height == 320, "the ways were not laid out: \(f.block)")
    #expect(abs(f.block.minX) < 1, "the ways start at \(f.block.minX)")
    #expect(abs(f.corner.width - 260) < 1, "the code column is \(f.corner.width) wide")
    #expect(abs(f.corner.maxX - 780) < 1, "the code ends at \(f.corner.maxX)")
    #expect(abs(f.corner.minY - f.block.minY) < 1, "the code is not level with the ways")
    #expect(f.corner.minX - f.block.maxX >= 80, "the ways \(f.block) run into the code \(f.corner)")
}

@MainActor @Test func aNarrowSheetPutsTheCodeUnderTheWays() {
    let f = staged(width: SignInStage<EmptyView, EmptyView>.besideFrom - 1)
    #expect(abs(f.block.minX) < 1)
    #expect(abs(f.corner.minX) < 1, "the code is at \(f.corner.minX)")
    #expect(f.corner.minY >= f.block.maxY, "the code \(f.corner) is not under the ways \(f.block)")
}

@Test func aLineSeparatesTheColumnsOnlyWhenTheyStandSideBySide() throws {
    typealias Stage = SignInStage<EmptyView, EmptyView>
    #expect(Stage.beside(780))
    #expect(!Stage.beside(Stage.besideFrom - 1))
    // The line sits in the middle of the gap, as tall as the code's column.
    let stage = try source("Account/SignInStage.swift")
    #expect(stage.contains(".overlay(alignment: .leading) { Self.divider(beside: Self.beside(width)) }"))
    #expect(stage.contains("Rectangle().fill(Theme.Palette.line).frame(width: 1).offset(x: -(gap + 1))"))
}

@Test func providersDrawTheirOwnButtonsOnlyForGoogleAndFacebook() {
    #expect(ProviderButtonStyle.draws("google"))
    #expect(ProviderButtonStyle.draws("facebook"))
    #expect(!ProviderButtonStyle.draws("apple"))
    #expect(!ProviderButtonStyle.draws("mock"))
}

private func source(_ path: String) throws -> String {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    return try String(contentsOf: app.appending(path: path), encoding: .utf8)
}

@Test func theLoginSheetHoldsTheCodeOnWideFormsAndAsksForItAsItOpens() throws {
    let screen = try source("Account/SignInScreen.swift")
    // Wide forms (the Mac, iPad at regular width) draw the columns, the code beside the steps that offer a join.
    #expect(screen.contains("if metrics.wide {\n                    SignInStage { stage } corner: {\n                        if cornerShows { PhoneCorner(server: flow.server) }"))
    #expect(screen.contains("case .choose, .new, .cannotOpen: return !flow.onEmail"))
    // Not now is the sheet's link at the top right, on the first step.
    #expect(screen.contains("link: flow.stage == .choose ? words.notNow : nil, onLink: notNow"))
    // The code asks as it shows and ends when the step goes, unless it joined; its words are first run's.
    let corner = try body(of: "private struct PhoneCorner: View", in: screen)
    #expect(corner.contains(".task { start() }"))
    #expect(corner.contains(".onDisappear { if !joined { sync.endPairing() } }"))
    #expect(corner.contains("Text(words.qrTitle)") && corner.contains("Text(words.qrWaiting)"))
    #expect(corner.contains(".frame(width: 200, height: 200)"))
    // Google and Facebook in their own buttons on every form (no Mac-only switch).
    #expect(screen.contains(".buttonStyle(ProviderButtonStyle(provider: id))"))
    #expect(!screen.contains("stageWays"))
    // The phone's Use another device, first when the sheet came from "I already use Momotus".
    #expect(screen.contains("if returning, offersDevice { anotherDevice }"))
    #expect(screen.contains("if !returning, offersDevice { anotherDevice }"))
    // Escape is Not now on the first step.
    // Escape on the first step: first run's Back to Welcome, Not now only where no Back is given (Settings).
    #expect(screen.contains(".onExitCommand {\n            switch flow.stage {\n            case .choose: (toWelcome ?? notNow)()"))
    // The code's join takes a held sign-in along, as the pushed pairing page's does.
    #expect(screen.contains("await flow.joinedByCode()"))
    // Settings' Sign in is the same sheet over the wall, asking for the wall as it opens (D10).
    let later = try body(of: "struct LaterSignIn: View", in: screen)
    #expect(later.contains("WallSheet(sheet: .login)") && later.contains("PosterWall("))
    #expect(later.contains("core.call(\"firstRunWall\""))
    #expect(!screen.contains("firstRunStill"), "the sign-in still asks for the old still")
}
