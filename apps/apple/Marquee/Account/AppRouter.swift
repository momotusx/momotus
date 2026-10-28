import Observation
import SwiftUI
#if os(iOS)
import UIKit
#endif

/// Where Sign in another device opens from outside a page (plans/accounts.md
/// 4.2, plans/tv-qr-login.md 5.6): the home-screen quick action and the top
/// bar's profile menu (the scanner), and a pairing code by link, the `/pair`
/// universal link or `momotus://pair` from the link host's page (the approve
/// sheet with that code). A phone not signed in yet goes to the Account page first.
@MainActor @Observable
final class AppRouter {
    static let shared = AppRouter()

    /// Bumped by each request; the shell presents the sheet when it changes.
    private(set) var scanRequests = 0
    /// The code a link brought, for the sheet to take once; none opens the scanner. Memory only.
    var pendingCode: String?
    /// A request the shell has not presented yet: one made at a cold start, before the shell exists.
    var unanswered = false

    func requestScan(code: String? = nil) {
        pendingCode = code
        unanswered = true
        scanRequests += 1
    }



    #if os(iOS)
    /// The quick action's type.
    static let signInAnotherType = "app.momotus.signInAnother"

    /// The home-screen quick action: only a phone or tablet in an account approves.
    static func updateQuickAction(joined: Bool) {
        UIApplication.shared.shortcutItems = joined
            ? [UIApplicationShortcutItem(type: signInAnotherType, localizedTitle: AccountWords.shared.signInAnother)]
            : []
    }
    #endif
}

#if os(iOS)
/// Hands the quick action to the router, at launch or while running.
final class SceneDelegate: NSObject, UIWindowSceneDelegate {
    func scene(_ scene: UIScene, willConnectTo session: UISceneSession, options: UIScene.ConnectionOptions) {
        // A brightness the player changed and a crash left behind goes back (plans/player-gestures.md 3.5).
        MainActor.assumeIsolated { BrightnessKeeper.repair((scene as? UIWindowScene)?.screen) }
        if options.shortcutItem?.type == AppRouter.signInAnotherType {
            MainActor.assumeIsolated { AppRouter.shared.requestScan() }
        }
        // A cold start by a pairing code's link (plans/tv-qr-login.md 5.6).
        open(options.urlContexts)
    }

    /// `momotus://pair#...` from the link host's page while the app runs. This
    /// delegate replaces SwiftUI's own, so URLs arrive here, not in `onOpenURL`.
    func scene(_ scene: UIScene, openURLContexts contexts: Set<UIOpenURLContext>) {
        open(contexts)
    }

    private func open(_ contexts: Set<UIOpenURLContext>) {
        guard let url = contexts.first?.url else { return }
        MainActor.assumeIsolated {
            guard let code = AccountRules.pairingCode(url, linkHost: PasskeyContract.relyingParty) else { return }
            AppRouter.shared.requestScan(code: code)
        }
    }

    /// The launch's first activation repairs once more: at connect the scene is not active yet, so a put there may not
    /// take and its record waits. Only the first: no player is up yet, while a later activation (after Control Center)
    /// would undo a brightness the viewer just swiped.
    func sceneDidBecomeActive(_ scene: UIScene) {
        MainActor.assumeIsolated {
            guard !BrightnessKeeper.repairedOnActive else { return }
            BrightnessKeeper.repairedOnActive = true
            BrightnessKeeper.repair((scene as? UIWindowScene)?.screen)
        }
    }

    func windowScene(_ windowScene: UIWindowScene, performActionFor shortcutItem: UIApplicationShortcutItem, completionHandler: @escaping (Bool) -> Void) {
        let ours = shortcutItem.type == AppRouter.signInAnotherType
        if ours { MainActor.assumeIsolated { AppRouter.shared.requestScan() } }
        completionHandler(ours)
    }
}
#endif
