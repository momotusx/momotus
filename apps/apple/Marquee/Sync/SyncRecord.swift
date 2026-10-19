import Foundation

/// The sync account's keystore record. The core never stores it: the app
/// keeps it in the Keychain and hands it back per call.
enum SyncRecord {
    @discardableResult
    static func store(_ record: JSONValue) -> Bool {
        guard let data = try? JSONEncoder().encode(record) else { return false }
        return Keychain.write("syncRecord", data)
    }

    static func load() -> JSONValue? {
        (try? Keychain.read("syncRecord")).flatMap { try? JSONDecoder().decode(JSONValue.self, from: $0) }
    }

    static func delete() {
        Keychain.delete("syncRecord")
    }
}
