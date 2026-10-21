import Foundation
import Observation

/// Sign out with "Remove your profiles and library from this device" (D13),
/// in the three steps the core holds the app to, repeated after a restart
/// until the wipe answers: (1) leave the account, (2) sign every profile out
/// of Trakt and delete its record, and disconnect each debrid account when
/// asked, (3) the wipe. The `accountGone` card's Remove from this device is
/// steps 2 and 3. The owner profile's only (`notOwner` otherwise).
@MainActor @Observable
final class AccountWipe {
    /// A wipe runs now.
    private(set) var running = false
    /// What stopped the last run, in words; the pending flag stays for the next.
    private(set) var problem: String?

    @ObservationIgnored private let core: CoreClient
    @ObservationIgnored private let sync: SyncModel
    /// A profile's Trakt record goes (TraktModel.profileGone).
    @ObservationIgnored private let traktGone: @MainActor (String) -> Void
    /// The debrid accounts now, and their disconnect (tombstone first: DebridSession).
    @ObservationIgnored private let debridAccounts: @MainActor () -> [DebridAccount]
    @ObservationIgnored private let disconnect: @MainActor (DebridAccount) async throws -> Void
    @ObservationIgnored private let defaults: UserDefaults

    /// The flag kept from the first step to the wipe, and whether debrid goes too.
    static let pendingKey = "accountWipePending"
    static let debridKey = "accountWipeDebrid"

    init(core: CoreClient, sync: SyncModel, traktGone: @escaping @MainActor (String) -> Void,
         debridAccounts: @escaping @MainActor () -> [DebridAccount], disconnect: @escaping @MainActor (DebridAccount) async throws -> Void,
         defaults: UserDefaults = .standard) {
        self.core = core
        self.sync = sync
        self.traktGone = traktGone
        self.debridAccounts = debridAccounts
        self.disconnect = disconnect
        self.defaults = defaults
    }

    /// A wipe was started and has not answered yet.
    var pending: Bool { defaults.bool(forKey: Self.pendingKey) }

    /// Sign out, then remove: step 1 behind the OS prompt, then 2 and 3.
    func signOutAndRemove(profiles: [String], disconnectDebrid: Bool, force: Bool, askedAgain: Bool = false) async throws {
        try await sync.signOut(force: force, askedAgain: askedAgain, quiet: true)
        mark(disconnectDebrid: disconnectDebrid)
        try await finish(profiles: profiles)
    }

    /// The account is gone already (another device deleted it): steps 2 and 3.
    func removeHere(profiles: [String], disconnectDebrid: Bool) async throws {
        mark(disconnectDebrid: disconnectDebrid)
        try await finish(profiles: profiles)
    }

    /// After a restart on the owner profile: the steps left, when one was pending.
    func resume(profiles: [String]) async {
        guard pending, !sync.joined, !running else { return }
        try? await finish(profiles: profiles)
    }

    private func mark(disconnectDebrid: Bool) {
        defaults.set(true, forKey: Self.pendingKey)
        defaults.set(disconnectDebrid, forKey: Self.debridKey)
    }

    /// Steps 2 and 3. Each profile's Trakt sign-out comes before its record goes;
    /// each debrid account goes tombstone first; the wipe only confirms them.
    private func finish(profiles: [String]) async throws {
        guard !running else { return }
        running = true
        problem = nil
        defer { running = false }
        let disconnectDebrid = defaults.bool(forKey: Self.debridKey)
        do {
            for profile in profiles {
                try await core.perform("traktSignOut", ["profile": profile])
                traktGone(profile)
            }
            if disconnectDebrid {
                for account in debridAccounts() { try await disconnect(account) }
            }
            try await core.perform("signOut", ["wipe": true, "disconnectDebrid": disconnectDebrid])
            defaults.removeObject(forKey: Self.pendingKey)
            defaults.removeObject(forKey: Self.debridKey)
        } catch {
            let code = (error as? CoreError)?.code ?? ""
            problem = AccountRules.trouble(code)?.text ?? error.localizedDescription
            throw error
        }
    }
}
