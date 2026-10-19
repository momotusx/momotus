import Foundation
#if os(iOS)
import UIKit
import UniformTypeIdentifiers
#elseif os(macOS)
import AppKit
#endif

/// The clipboard for a secret (a recovery kit) or a pairing code: this device
/// only, and gone after a while.
enum Clipboard {
    /// How long a copied kit stays.
    static let secretLifetime: TimeInterval = 60

    /// Copies a kit for a minute, this device only (no Universal
    /// Clipboard). True when it was copied.
    @MainActor @discardableResult
    static func copySecret(_ text: String) -> Bool {
        copy(text, lifetime: secretLifetime, localOnly: true)
    }

    /// A pairing code may cross to the user's other device (Universal
    /// Clipboard): it holds public keys only and expires with the code.
    @MainActor @discardableResult
    static func copy(_ text: String, lifetime: TimeInterval, localOnly: Bool) -> Bool {
        #if os(iOS)
        // The system drops it at the expiry, whether or not the app still runs.
        UIPasteboard.general.setItems([[UTType.plainText.identifier: text]], options: [
            .localOnly: localOnly, .expirationDate: Date().addingTimeInterval(lifetime),
        ])
        return true
        #elseif os(macOS)
        let board = NSPasteboard.general
        let change = write(text, to: board)
        // The timer dies with the app: a secret is also cleared on quit, and at the next launch.
        if localOnly { pending(change) }
        Task { @MainActor in
            try? await Task.sleep(for: .seconds(lifetime))
            clear(board, ifStill: change)
        }
        return true
        #else
        return false
        #endif
    }

    #if os(macOS)
    /// The pasteboard change count of a secret the app wrote and has not cleared yet.
    static let pendingKey = "secretPasteboardChange"

    /// Writes `text` marked transient and concealed (clipboard managers that
    /// honour the marks skip it); returns the board's change count.
    @discardableResult
    static func write(_ text: String, to board: NSPasteboard) -> Int {
        board.clearContents()
        board.declareTypes([.string, NSPasteboard.PasteboardType("org.nspasteboard.TransientType"), NSPasteboard.PasteboardType("org.nspasteboard.ConcealedType")], owner: nil)
        board.setString(text, forType: .string)
        return board.changeCount
    }

    /// Empties `board` if it still holds what the app wrote (`change`): a copy
    /// made since, here or in another app, stays. True when it was emptied.
    @discardableResult
    static func clear(_ board: NSPasteboard, ifStill change: Int, defaults: UserDefaults = .standard) -> Bool {
        if defaults.object(forKey: pendingKey) as? Int == change { defaults.removeObject(forKey: pendingKey) }
        guard board.changeCount == change else { return false }
        board.clearContents()
        return true
    }

    /// A secret the app wrote and could not clear before it stopped (quit,
    /// crash, a kill): emptied at the next launch if nothing was copied since.
    /// ponytail: the change count restarts with the system, so after a restart
    /// an unrelated copy with the same count is emptied once.
    @MainActor static func clearLeftover(_ board: NSPasteboard = .general, defaults: UserDefaults = .standard) {
        guard let change = defaults.object(forKey: pendingKey) as? Int else { return }
        clear(board, ifStill: change, defaults: defaults)
        defaults.removeObject(forKey: pendingKey)
    }

    @MainActor private static var watchingQuit = false

    /// Remembers the secret's change count, and empties the clipboard when the app quits.
    @MainActor static func pending(_ change: Int, defaults: UserDefaults = .standard) {
        defaults.set(change, forKey: pendingKey)
        guard !watchingQuit else { return }
        watchingQuit = true
        NotificationCenter.default.addObserver(forName: NSApplication.willTerminateNotification, object: nil, queue: .main) { _ in
            MainActor.assumeIsolated { clearLeftover() }
        }
    }
    #endif
}
