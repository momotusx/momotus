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
    /// library does not. Deleted before the core opens, on the first launch
    /// of an install whose library is empty, so the app never joins silently
    /// with it. True when one was deleted.
    @discardableResult
    func forgetLeftover(libraryExists: Bool, defaults: UserDefaults = .standard) -> Bool {
        let marked = defaults.bool(forKey: Self.installedKey)
        defaults.set(true, forKey: Self.installedKey)
        guard !marked, !libraryExists, (try? load()) != nil else { return false }
        guard delete() else {
            coreLog.error("sync: a key left by a deleted install could not be deleted")
            return false
        }
        return true
    }

    /// Set on the first launch of an install; deleting the app deletes it.
    static let installedKey = "installed"
}
