import Foundation
import Observation

/// Sign out with "Remove your profiles and library from this device" (D13),
/// in the three steps the core holds the app to: (1) leave the account,
/// (2) sign every profile out of Trakt and delete its record, and disconnect
/// each debrid account when asked, (3) the wipe. The core keeps the removal
/// from before the leave goes out until the wipe answers (`signOut
/// {removeAfter}`, the profiles view's `wipePending`; a failed leave or any
/// join clears it), so a restart repeats steps 2 and 3. The `accountGone`
/// card's Remove from this device records it without a leave, then the same.
/// The owner profile's only (`notOwner` otherwise).
@MainActor @Observable
final class AccountWipe {
    /// A wipe runs now.
    private(set) var running = false
    /// What stopped the last run, in words; the core keeps the removal for the next.
    private(set) var problem: String?

    @ObservationIgnored private let core: CoreClient
    @ObservationIgnored private let sync: SyncModel
    /// The removal the core holds (the profiles view's `wipePending`).
    @ObservationIgnored private let wipePending: @MainActor () -> WipePending?
    /// A profile's Trakt record goes (TraktModel.profileGone).
    @ObservationIgnored private let traktGone: @MainActor (String) -> Void
    /// The debrid accounts now, and their disconnect (tombstone first: DebridSession).
    @ObservationIgnored private let debridAccounts: @MainActor () -> [DebridAccount]
    @ObservationIgnored private let disconnect: @MainActor (DebridAccount) async throws -> Void

    init(core: CoreClient, sync: SyncModel, wipePending: @escaping @MainActor () -> WipePending?,
         traktGone: @escaping @MainActor (String) -> Void,
         debridAccounts: @escaping @MainActor () -> [DebridAccount], disconnect: @escaping @MainActor (DebridAccount) async throws -> Void) {
        self.core = core
        self.sync = sync
        self.wipePending = wipePending
        self.traktGone = traktGone
        self.debridAccounts = debridAccounts
        self.disconnect = disconnect
    }

    /// A removal was started and its wipe has not answered yet.
    var pending: Bool { wipePending() != nil }

    /// Sign out, then remove: step 1 behind the OS prompt (the core records the removal first), then 2 and 3.
    func signOutAndRemove(profiles: [String], disconnectDebrid: Bool, force: Bool, askedAgain: Bool = false) async throws {
        try await sync.signOut(force: force, askedAgain: askedAgain, removeAfter: disconnectDebrid)
        try await finish(profiles: profiles, disconnectDebrid: disconnectDebrid)
    }

    /// The account is gone already (another device deleted it): behind the same
    /// OS prompt as a sign-out, the removal recorded with no leave, then steps 2 and 3.
    func removeHere(profiles: [String], disconnectDebrid: Bool, askedAgain: Bool = false) async throws {
        try await sync.confirmRemoval(askedAgain: askedAgain)
        try await core.perform("signOut", ["removeAfter": ["debrid": disconnectDebrid]])
        try await finish(profiles: profiles, disconnectDebrid: disconnectDebrid)
    }

    /// At start, on the owner profile with no account here: the steps the core says are left.
    func resume(profiles: [String]) async {
        guard let pending = wipePending(), !sync.joined, !running else { return }
        try? await finish(profiles: profiles, disconnectDebrid: pending.debrid)
    }

    /// Steps 2 and 3. Each profile's Trakt sign-out comes before its record goes;
    /// each debrid account goes tombstone first; the wipe only confirms them.
    private func finish(profiles: [String], disconnectDebrid: Bool) async throws {
        guard !running else { return }
        running = true
        problem = nil
        defer { running = false }
        do {
            for profile in profiles {
                try await core.perform("traktSignOut", ["profile": profile])
                traktGone(profile)
            }
            if disconnectDebrid {
                for account in debridAccounts() { try await disconnect(account) }
            }
            try await core.perform("signOut", ["wipe": true, "disconnectDebrid": disconnectDebrid])
        } catch {
            let code = (error as? CoreError)?.code ?? ""
            problem = AccountRules.trouble(code)?.text ?? AccountWords.shared.wipeFailed
            throw error
        }
    }
}
