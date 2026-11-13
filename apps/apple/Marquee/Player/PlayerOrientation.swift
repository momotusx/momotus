#if os(iOS)
import UIKit

/// The phone player opens across, either side by the sensor, and turns
/// upright when the phone does, as the system player does; leaving puts back
/// the orientation the pages had. iPad windows keep theirs.
@MainActor
enum PlayerOrientation {
    /// Answered by the app delegate for every window.
    private(set) static var allowed: UIInterfaceOrientationMask = .all
    private static var before: UIInterfaceOrientation?
    /// Players on screen: the last to leave hands the turn back, never one leaving under another (QA r5, R5-29).
    private static var players = 0

    /// `across`: turn across (a play's first appear), else upright.
    static func enter(across: Bool) {
        players += 1
        guard UIDevice.current.userInterfaceIdiom == .phone, let scene else { return }
        // A second appear (back from Versions over the player) keeps the pages' first one.
        before = before ?? scene.effectiveGeometry.interfaceOrientation
        turn(scene, allowing: .allButUpsideDown, to: across ? .landscape : .portrait)
    }

    /// Whether the player was across as it left, for its next appear; nil off the phone.
    @discardableResult static func leave() -> Bool? {
        players = max(0, players - 1)
        // Another player is up (the film brought back from Picture in Picture before this one's page went): it keeps
        // its turn. Handed back here, the phone turned upright under it.
        guard players == 0, UIDevice.current.userInterfaceIdiom == .phone, let scene, let before else { return nil }
        let across = scene.effectiveGeometry.interfaceOrientation.isLandscape
        self.before = nil
        turn(scene, allowing: .all, to: before.isLandscape ? .landscape : .portrait)
        return across
    }

    private static func turn(_ scene: UIWindowScene, allowing mask: UIInterfaceOrientationMask, to target: UIInterfaceOrientationMask) {
        allowed = mask
        scene.keyWindow?.rootViewController?.setNeedsUpdateOfSupportedInterfaceOrientations()
        scene.requestGeometryUpdate(.iOS(interfaceOrientations: target))
    }

    private static var scene: UIWindowScene? {
        UIApplication.shared.connectedScenes.lazy.compactMap { $0 as? UIWindowScene }.first { $0.activationState == .foregroundActive }
    }
}

/// Hands the player's orientation to UIKit.
final class AppDelegate: NSObject, UIApplicationDelegate {
    func application(_ application: UIApplication, supportedInterfaceOrientationsFor window: UIWindow?) -> UIInterfaceOrientationMask {
        PlayerOrientation.allowed
    }

    /// The quick action (Sign in another device) reaches the scene's delegate.
    func application(_ application: UIApplication, configurationForConnecting session: UISceneSession, options: UIScene.ConnectionOptions) -> UISceneConfiguration {
        let configuration = UISceneConfiguration(name: nil, sessionRole: session.role)
        configuration.delegateClass = SceneDelegate.self
        return configuration
    }
}
#endif
