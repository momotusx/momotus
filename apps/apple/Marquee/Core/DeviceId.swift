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

    /// Replaces the value; false when the Keychain is not available (unsigned local builds).
    @discardableResult
    static func write(_ account: String, _ data: Data) -> Bool {
        SecItemDelete(query(account) as CFDictionary)
        var query = query(account)
        query[kSecValueData as String] = data
        query[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        return SecItemAdd(query as CFDictionary, nil) == errSecSuccess
    }

    static func delete(_ account: String) {
        SecItemDelete(query(account) as CFDictionary)
    }
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
