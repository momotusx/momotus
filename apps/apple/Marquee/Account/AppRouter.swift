import Observation
import SwiftUI
#if os(iOS)
import UIKit
#endif

/// Where Sign in another device opens from outside a page (plans/accounts.md
/// 4.2): the home-screen quick action, the `/pair` universal link a phone
/// camera opens, and the top bar's profile menu. The shell shows the scanner
/// on each request; a phone not signed in yet goes to the Account page first.
@MainActor @Observable
final class AppRouter {
    static let shared = AppRouter()

    /// Bumped by each request; the shell presents the scanner when it changes.
    private(set) var scanRequests = 0
    /// The request came from the `/pair` link: the scanner asks to scan once more here.
    private(set) var fromLink = false

    func requestScan(fromLink: Bool = false) {
        self.fromLink = fromLink
        scanRequests += 1
    }

    #if os(iOS)
    /// The quick action's type.
    static let signInAnotherType = "app.marquee.signInAnother"

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
        if options.shortcutItem?.type == AppRouter.signInAnotherType {
            MainActor.assumeIsolated { AppRouter.shared.requestScan() }
        }
    }

    func windowScene(_ windowScene: UIWindowScene, performActionFor shortcutItem: UIApplicationShortcutItem, completionHandler: @escaping (Bool) -> Void) {
        let ours = shortcutItem.type == AppRouter.signInAnotherType
        if ours { MainActor.assumeIsolated { AppRouter.shared.requestScan() } }
        completionHandler(ours)
    }
}
#endif
