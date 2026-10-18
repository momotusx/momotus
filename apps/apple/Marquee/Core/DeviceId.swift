import Foundation
import Security

/// Small values in the Keychain, this-device-only, so a backup restored on
/// another device does not carry them.
enum Keychain {
    private static func query(_ account: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: "app.marquee",
            kSecAttrAccount as String: account,
            kSecUseDataProtectionKeychain as String: true,
        ]
    }

    static func read(_ account: String) -> Data? {
        var query = query(account)
        query[kSecReturnData as String] = true
        var result: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess else { return nil }
        return result as? Data
    }

    /// Replaces the value in place, durable or nothing: a refused write
    /// leaves the old value as it was. False when the Keychain is not
    /// available (unsigned local builds).
    @discardableResult
    static func write(_ account: String, _ data: Data) -> Bool {
        let changes: [String: Any] = [
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        let updated = SecItemUpdate(query(account) as CFDictionary, changes as CFDictionary)
        if updated == errSecSuccess { return true }
        guard updated == errSecItemNotFound else { return false }
        var query = query(account)
        query.merge(changes) { $1 }
        return SecItemAdd(query as CFDictionary, nil) == errSecSuccess
    }

    /// Removes the value; false when the Keychain refused (an absent value is removed already).
    @discardableResult
    static func delete(_ account: String) -> Bool {
        let status = SecItemDelete(query(account) as CFDictionary)
        return status == errSecSuccess || status == errSecItemNotFound
    }

    /// The names of the app's items that start with `prefix`.
    static func accounts(prefix: String) -> [String] {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: "app.marquee",
            kSecUseDataProtectionKeychain as String: true,
            kSecMatchLimit as String: kSecMatchLimitAll,
            kSecReturnAttributes as String: true,
        ]
        var result: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess,
              let items = result as? [[String: Any]] else { return [] }
        return items.compactMap { $0[kSecAttrAccount as String] as? String }.filter { $0.hasPrefix(prefix) }.sorted()
    }
}

/// The debrid keystore's items in the Keychain.
struct KeychainItems: SecretItems {
    func read(_ account: String) -> Data? { Keychain.read(account) }
    func write(_ account: String, _ data: Data) -> Bool { Keychain.write(account, data) }
    func delete(_ account: String) -> Bool { Keychain.delete(account) }
    func accounts(prefix: String) -> [String] { Keychain.accounts(prefix: prefix) }
}

/// This install's sync device id, kept in the Keychain.
enum DeviceId {
    static func current(fallbackDirectory: URL) -> UInt64 {
        let file = fallbackDirectory.appending(path: "device-id")
        let stored = Keychain.read("deviceId") ?? (try? Data(contentsOf: file))
        if let stored, let id = UInt64(String(decoding: stored, as: UTF8.self)) {
            return id
        }
        let id = UInt64.random(in: 1...UInt64.max)
        let data = Data(String(id).utf8)
        if !Keychain.write("deviceId", data) {
            // Unsigned local builds have no keychain entitlement: keep the id in
            // a file that backups leave out.
            try? FileManager.default.createDirectory(at: fallbackDirectory, withIntermediateDirectories: true)
            try? data.write(to: file, options: .atomic)
            var values = URLResourceValues()
            values.isExcludedFromBackup = true
            var url = file
            try? url.setResourceValues(values)
        }
        return id
    }
}
