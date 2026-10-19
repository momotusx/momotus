import Foundation

/// The sync account's keystore record. The core never stores it: the app
/// keeps it in the Keychain and hands it back per call. Tests pass their own.
struct SyncRecord: Sendable {
    /// The record, or nil when there is none. Throws when the Keychain did
    /// not answer or the record does not read, which is not the same as none.
    var load: @Sendable () throws -> JSONValue?
    /// False when the Keychain refused it.
    var store: @Sendable (JSONValue) -> Bool
    /// False when the Keychain refused (an absent record is deleted already).
    var delete: @Sendable () -> Bool

    static let keychain = SyncRecord(
        load: { try Keychain.read("syncRecord").map { try JSONDecoder().decode(JSONValue.self, from: $0) } },
        store: { record in (try? JSONEncoder().encode(record)).map { Keychain.write("syncRecord", $0) } ?? false },
        delete: { Keychain.delete("syncRecord") }
    )

    /// A record left by a deleted install: the Keychain outlives the app, its
    /// library does not. Run before the core opens. The first launch of an
    /// install whose library is empty marks the cleanup pending; it is done
    /// only once the Keychain says there is no record or deleted it, and is
    /// tried again at every launch until then (the library exists by then).
    /// True while it is pending: the app then never loads that key.
    func forgetLeftover(libraryExists: Bool, defaults: UserDefaults = .standard) -> Bool {
        guard !defaults.bool(forKey: Self.installedKey) else { return false }
        // An update, not a new install: nothing was left behind. A missing library alone proves
        // nothing (tvOS keeps it in Caches, which the system purges); a join's server address does.
        guard defaults.bool(forKey: Self.pendingKey) || !(libraryExists || defaults.string(forKey: Self.serverKey) != nil) else {
            defaults.set(true, forKey: Self.installedKey)
            return false
        }
        defaults.set(true, forKey: Self.pendingKey)
        let record: JSONValue?
        do {
            record = try load()
        } catch {
            coreLog.error("sync: the Keychain did not answer; a key left by a deleted install is removed at the next launch")
            return true
        }
        if record != nil {
            guard delete() else {
                coreLog.error("sync: a key left by a deleted install could not be deleted")
                return true
            }
            coreLog.notice("sync: deleted a key left by a deleted install")
        }
        Self.cleaned(defaults)
        return false
    }

    /// The leftover is gone: the install is marked, and nothing is pending.
    static func cleaned(_ defaults: UserDefaults) {
        defaults.set(true, forKey: installedKey)
        defaults.removeObject(forKey: pendingKey)
    }

    /// Set once an install's first launch found nothing left behind, or removed it; deleting the app deletes it.
    static let installedKey = "installed"
    /// The sync server's address, stored by every join: deleting the app deletes it.
    static let serverKey = "syncServer"
    /// A record left by a deleted install is still to be removed.
    static let pendingKey = "syncLeftoverPending"
}
