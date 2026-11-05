import AppKit
import Foundation
import SwiftUI
import Testing

// The Mac's sign-in stage (variant 4, the owner's pick of 2026-11-04, design/exploration/desktop-signin), laid out
// for real: the step at the lower left on the first run's inset, the phone's QR at the lower right, under the step
// in a narrow window. The screen's wiring (the corner's code asked as the step opens, the providers' own buttons,
// Escape) is read from its source, which needs the app's models to run.

@MainActor private final class Frames {
    var block = CGRect.zero
    var corner = CGRect.zero
}

/// The stage in a `width` by 900 window, with stand-ins of the step's and the corner's sizes.
@MainActor private func staged(width: CGFloat) -> Frames {
    let frames = Frames()
    let stage = SignInStage(placed: { frames.block = $0; frames.corner = $1 }) {
        Color.gray.frame(height: 320)
    } corner: {
        Color.white.frame(width: SignInStage<EmptyView, EmptyView>.cornerWidth, height: 220)
    }
    let host = NSHostingView(rootView: stage.frame(width: width, height: 900))
    host.frame = CGRect(x: 0, y: 0, width: width, height: 900)
    host.layoutSubtreeIfNeeded()
    RunLoop.main.run(until: Date().addingTimeInterval(0.3))
    return frames
}

@MainActor @Test func theStepSitsAtTheLowerLeftAndTheCodeAtTheLowerRight() {
    let f = staged(width: 1440)
    #expect(f.block.height == 320, "the step was not laid out: \(f.block)")
    #expect(abs(f.block.minX - 80) < 1, "the step starts at \(f.block.minX)")
    #expect(abs(f.block.maxY - (900 - 84)) < 1, "the step ends at \(f.block.maxY)")
    #expect(abs(f.corner.maxX - (1440 - 56)) < 1, "the code ends at \(f.corner.maxX)")
    #expect(abs(f.corner.maxY - (900 - 84)) < 1, "the code's foot is at \(f.corner.maxY)")
    #expect(f.block.maxX < f.corner.minX, "the step \(f.block) runs into the code \(f.corner)")
}

@MainActor @Test func aNarrowWindowPutsTheCodeUnderTheStep() {
    let f = staged(width: BackdropStops.macMinWidth)
    #expect(abs(f.block.minX - 80) < 1)
    #expect(abs(f.corner.minX - 80) < 1, "the code is at \(f.corner.minX)")
    #expect(f.corner.minY >= f.block.maxY, "the code \(f.corner) is not under the step \(f.block)")
    #expect(f.corner.maxY <= 900 - 84 + 1)
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

@Test func theMacsSignInIsTheStageWithTheCodeAskedAsItOpens() throws {
    let screen = try source("Account/SignInScreen.swift")
    // Every step is drawn in the stage, the code in its corner on the steps that offer a join.
    #expect(screen.contains("    private var layout: some View {\n        SignInStage {"))
    #expect(screen.contains("if cornerShows { PhoneCorner(server: flow.server) }"))
    #expect(screen.contains("case .choose, .new, .cannotOpen: return !flow.onEmail"))
    // The corner asks for its code as it shows and ends it when the step goes, unless it joined.
    let corner = try body(of: "private struct PhoneCorner: View", in: screen)
    #expect(corner.contains(".task { start() }"))
    #expect(corner.contains(".onDisappear { if !joined { sync.endPairing() } }"))
    #expect(corner.contains("Text(words.tvSignIn)"))
    // Google and Facebook in their own buttons; "I already use Momotus on another device" on every first step.
    #expect(screen.contains(".buttonStyle(ProviderButtonStyle(provider: id))"))
    #expect(screen.contains("if flow.providers.isEmpty || Self.stageWays, !flow.unreachable {"))
    // Escape is Not now on the first step.
    #expect(screen.contains(".onExitCommand {\n            switch flow.stage {\n            case .choose: notNow()"))
    // The corner's join takes a held sign-in along, as the pushed pairing page's does.
    #expect(screen.contains("await flow.joinedByCode()"))
}
