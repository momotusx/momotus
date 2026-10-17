import Foundation
import Security

/// This install's sync device id. It lives in the Keychain with this-device-only
/// access, so a backup restored on another device gets a new id.
enum DeviceId {
    static func current(fallbackDirectory: URL) -> UInt64 {
        let file = fallbackDirectory.appending(path: "device-id")
        if let id = readKeychain() ?? (try? String(contentsOf: file, encoding: .utf8)).flatMap(UInt64.init) {
            return id
        }
        let id = UInt64.random(in: 1...UInt64.max)
        if !writeKeychain(id) {
            // Unsigned local builds have no keychain entitlement: keep the id in
            // a file that backups leave out.
            try? FileManager.default.createDirectory(at: fallbackDirectory, withIntermediateDirectories: true)
            try? Data(String(id).utf8).write(to: file, options: .atomic)
            var values = URLResourceValues()
            values.isExcludedFromBackup = true
            var url = file
            try? url.setResourceValues(values)
        }
        return id
    }

    private static func query() -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: "app.marquee.device",
            kSecAttrAccount as String: "deviceId",
            kSecUseDataProtectionKeychain as String: true,
        ]
    }

    private static func readKeychain() -> UInt64? {
        var query = query()
        query[kSecReturnData as String] = true
        var result: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess, let data = result as? Data else {
            return nil
        }
        return UInt64(String(decoding: data, as: UTF8.self))
    }

    private static func writeKeychain(_ id: UInt64) -> Bool {
        var query = query()
        query[kSecValueData as String] = Data(String(id).utf8)
        query[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        return SecItemAdd(query as CFDictionary, nil) == errSecSuccess
    }
}
