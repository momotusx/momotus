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

    static func enter() {
        guard UIDevice.current.userInterfaceIdiom == .phone, let scene else { return }
        // A second appear (back from Versions over the player) keeps the pages' first one.
        before = before ?? scene.effectiveGeometry.interfaceOrientation
        turn(scene, allowing: .allButUpsideDown, to: .landscape)
    }

    static func leave() {
        guard UIDevice.current.userInterfaceIdiom == .phone, let scene, let before else { return }
        self.before = nil
        turn(scene, allowing: .all, to: before.isLandscape ? .landscape : .portrait)
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
}
#endif
