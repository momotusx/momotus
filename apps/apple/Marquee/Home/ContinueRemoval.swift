import Foundation
import Observation

/// "Remove from Continue Watching" with five seconds to take it back (Android's `PendingRemoval`). Home leaves the
/// title out meanwhile; when the time passes it goes to the core (a synced register, so a slip would hide the title
/// on every device), and at once when another title is removed. A failure is said with Try again, and the title is
/// back. Held by Home's model, so a profile switch drops a removal still waiting instead of sending it.
/// A removal still waiting when the app is quit is dropped, so the title stays: the safe side of a synced register.
@MainActor @Observable
final class ContinueRemoval {
    static let window: Duration = .seconds(5)

    /// The title waiting for its Undo.
    private(set) var removing: Tile?
    /// The title on its way to the core: out of the row, past its Undo.
    private(set) var sending: Tile?
    /// The last removal the core refused: the title and why.
    private(set) var failed: (tile: Tile, words: String)?
    @ObservationIgnored private let wait: Duration
    @ObservationIgnored private let send: (Tile) async throws -> Void
    @ObservationIgnored private var timer: Task<Void, Never>?

    init(wait: Duration = window, send: @escaping (Tile) async throws -> Void) {
        self.wait = wait
        self.send = send
    }

    /// The removal that last took a title, for `played`: set by `remove`, not `init`, since a view's init can make
    /// models it throws away.
    private static weak var current: ContinueRemoval?

    /// A title started playing (`PlayerModel`, every way a play starts): playing it brings it back to Continue
    /// Watching, so a removal of it still waiting for its Undo is dropped, never sent when the time ends.
    static func played(itemId: String) {
        if let current, current.removing?.id == itemId { current.undo() }
    }

    /// Whether the row leaves `tile` out: waiting for its Undo, or on its way.
    func hides(_ tile: Tile) -> Bool { tile.id == removing?.id || tile.id == sending?.id }

    func remove(_ tile: Tile) {
        timer?.cancel()
        Self.current = self
        // The one waiting goes now, without its Undo.
        if let earlier = removing { Task { await deliver(earlier) } }
        failed = nil
        removing = tile
        timer = Task { [weak self, wait] in
            try? await Task.sleep(for: wait)
            guard !Task.isCancelled, let self, self.removing == tile else { return }
            await self.deliver(tile)
        }
    }

    /// Nothing goes to the core, and the title is back.
    func undo() {
        timer?.cancel()
        timer = nil
        removing = nil
    }

    private func deliver(_ tile: Tile) async {
        if removing == tile { removing = nil }
        sending = tile
        do {
            try await send(tile)
        } catch {
            coreLog.error("remove from continue watching: \(error.localizedDescription, privacy: .public)")
            failed = (tile, removeFailedWords(name: tile.name, reason: error.localizedDescription))
        }
        // Gone from the row once the core's own list leaves it out; back when it failed.
        if sending == tile { sending = nil }
    }
}

/// The line under Continue Watching while a removal can be taken back.
func removedWords(name: String) -> String { "\(name) was removed from Continue Watching." }
