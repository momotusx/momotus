import Foundation

// Debrid account DTOs, mirroring apps/web/src/core/generated/DebridRecord.ts,
// DebridCredential.ts, DebridTombstone.ts and RestoreAnswer.ts. A credential
// and a record never print a key, token, client id or secret: description,
// debugDescription and the reflection dump show only what identifies them.

/// One account connection, as the app's keystore stores it.
struct DebridRecord: Codable, Hashable, Sendable {
    /// "realdebrid", "alldebrid", "torbox" or "premiumize".
    let service: String
    /// 32 hex characters, new for every connection.
    let generation: String
    /// 1 for a new connection, one more for each token rotation.
    let revision: UInt64
    let credential: DebridCredential
}

/// An API key, or an OAuth sign-in with its client and tokens. Tagged by
/// "kind" ("key" or "oauth") in JSON.
enum DebridCredential: Hashable, Sendable {
    case key(String)
    /// `issuedAtMs` is the wall clock when the access token was issued, unix milliseconds.
    case oauth(clientId: String, clientSecret: String, refreshToken: String, accessToken: String, issuedAtMs: UInt64, expiresInMs: UInt64)
}

/// Written by the app before it asks to disconnect: a record with this
/// generation is never restored.
struct DebridTombstone: Codable, Hashable, Sendable {
    let service: String
    let generation: String
}

/// The core's answer to `restoreDebrid`.
struct RestoreAnswer: Codable, Hashable, Sendable {
    /// Generations not restored because they have a tombstone.
    let refused: [String]
    /// Tombstones whose record is gone from the keystore: delete them.
    let tombstones: [DebridTombstone]
}

extension DebridCredential: Codable {
    private enum Key: String, CodingKey {
        case kind, key, clientId, clientSecret, refreshToken, accessToken, issuedAtMs, expiresInMs
    }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        switch try c.decode(String.self, forKey: .kind) {
        case "key":
            self = .key(try c.decode(String.self, forKey: .key))
        case "oauth":
            self = .oauth(
                clientId: try c.decode(String.self, forKey: .clientId),
                clientSecret: try c.decode(String.self, forKey: .clientSecret),
                refreshToken: try c.decode(String.self, forKey: .refreshToken),
                accessToken: try c.decode(String.self, forKey: .accessToken),
                issuedAtMs: try c.decode(UInt64.self, forKey: .issuedAtMs),
                expiresInMs: try c.decode(UInt64.self, forKey: .expiresInMs))
        case let kind:
            throw DecodingError.dataCorruptedError(forKey: .kind, in: c, debugDescription: "unknown credential kind \(kind)")
        }
    }

    func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: Key.self)
        switch self {
        case .key(let key):
            try c.encode("key", forKey: .kind)
            try c.encode(key, forKey: .key)
        case let .oauth(clientId, clientSecret, refreshToken, accessToken, issuedAtMs, expiresInMs):
            try c.encode("oauth", forKey: .kind)
            try c.encode(clientId, forKey: .clientId)
            try c.encode(clientSecret, forKey: .clientSecret)
            try c.encode(refreshToken, forKey: .refreshToken)
            try c.encode(accessToken, forKey: .accessToken)
            try c.encode(issuedAtMs, forKey: .issuedAtMs)
            try c.encode(expiresInMs, forKey: .expiresInMs)
        }
    }
}

extension DebridCredential: CustomStringConvertible, CustomDebugStringConvertible, CustomReflectable {
    /// "Key(<redacted>)" or "Oauth(issuedAtMs: ..., expiresInMs: ..., ..)", as the core's Debug.
    var description: String {
        switch self {
        case .key: "Key(<redacted>)"
        case let .oauth(_, _, _, _, issuedAtMs, expiresInMs): "Oauth(issuedAtMs: \(issuedAtMs), expiresInMs: \(expiresInMs), ..)"
        }
    }

    var debugDescription: String { description }

    var customMirror: Mirror { Mirror(self, children: ["redacted": description]) }
}

extension DebridRecord: CustomStringConvertible, CustomDebugStringConvertible, CustomReflectable {
    var description: String {
        "DebridRecord(service: \(service), generation: \(generation), revision: \(revision), credential: \(credential))"
    }

    var debugDescription: String { description }

    var customMirror: Mirror {
        Mirror(self, children: ["service": service, "generation": generation, "revision": revision, "credential": credential])
    }
}

/// The `debrid` view (apps/web/src/core/generated/DebridSnapshot.ts).
struct DebridSnapshot: Decodable, Hashable, Sendable {
    /// Connected services, the default store first.
    let accounts: [DebridAccount]
    /// Per service, the newest record the app has not acknowledged: store it, then `ackDebrid`.
    let pending: [DebridRecord]
}

struct DebridAccount: Decodable, Hashable, Sendable {
    let service: String
    /// What `disconnectDebrid` names.
    let generation: String
    /// "ok", "failing" or "reconnect".
    let state: String
}

/// A sign-in by code (`debridLogin`), tagged by "phase".
enum DebridLoginSnapshot: Decodable, Hashable, Sendable {
    case starting
    /// Show `userCode` and `url`, and `qr` as a QR code, until `expiresMs` (wall clock).
    case code(userCode: String, url: String, qr: String, expiresMs: Int64)
    case done, expired, denied, used
    case failed(reason: String)

    private enum Key: String, CodingKey { case phase, userCode, url, qr, expiresMs, reason }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        switch try c.decode(String.self, forKey: .phase) {
        case "starting": self = .starting
        case "code":
            self = .code(userCode: try c.decode(String.self, forKey: .userCode), url: try c.decode(String.self, forKey: .url),
                         qr: try c.decode(String.self, forKey: .qr), expiresMs: try c.decode(Int64.self, forKey: .expiresMs))
        case "done": self = .done
        case "expired": self = .expired
        case "denied": self = .denied
        case "used": self = .used
        case "failed": self = .failed(reason: try c.decode(String.self, forKey: .reason))
        case let phase: throw DecodingError.dataCorruptedError(forKey: .phase, in: c, debugDescription: "unknown sign-in phase \(phase)")
        }
    }
}

/// The debrid services and the words around them (Android's `DebridCopy`).
enum DebridWords {
    struct Service: Identifiable, Sendable {
        let id: String
        let name: String
        let keyPage: String
    }

    /// In this order: `restoreDebrid` makes the first stored one the default.
    static let services = [
        Service(id: "realdebrid", name: "Real-Debrid", keyPage: "https://real-debrid.com/apitoken"),
        Service(id: "torbox", name: "TorBox", keyPage: "https://torbox.app/settings"),
        Service(id: "alldebrid", name: "AllDebrid", keyPage: "https://alldebrid.com/apikeys"),
        Service(id: "premiumize", name: "Premiumize", keyPage: "https://www.premiumize.me/account"),
    ]

    /// Services that sign in with a code; Premiumize takes a key only.
    static let codeServices: Set<String> = ["realdebrid", "alldebrid", "torbox"]

    /// A card offers its sign-in by code, its one primary, while the service has
    /// none or asked for a new one; a connected card has no primary (Android's DebridScreen).
    static func offersSignIn(_ service: String, _ account: DebridAccount?) -> Bool {
        codeServices.contains(service) && (account == nil || account?.state == "reconnect")
    }

    static func inUse(_ count: Int) -> String {
        switch count {
        case 0: "No debrid service in use."
        case 1: "1 debrid service in use."
        default: "\(count) debrid services in use."
        }
    }

    /// A card's state in words, nil when the service is not connected.
    /// "Connected" only once its record is stored and nothing is pending.
    static func state(_ account: DebridAccount?, pending: Bool, stored: Bool = true) -> String? {
        guard let account else { return nil }
        // The core holds it, but the Keychain did not take it: gone after a restart.
        if pending && !stored { return "Kept for this session" }
        switch account.state {
        case "reconnect": return "Sign in again: the service asked for it"
        case "failing": return "Not answering, trying again"
        default: return pending ? "Saving" : "Connected"
        }
    }

    /// What a sign-in by code says, for every phase but the code itself.
    static func login(_ phase: DebridLoginSnapshot) -> String {
        switch phase {
        case .starting: "Getting a code"
        case .code: "Enter the code"
        case .done: "Connected"
        case .expired: "The code ran out before it was entered."
        case .denied: "The sign-in was turned down on the service's page."
        case .used: "Another sign-in or key took its place."
        case .failed(let reason): reason
        }
    }

    /// "Good for 4:59": the time a code has left, rounded up.
    static func goodFor(_ ms: Int64) -> String {
        let total = max(0, (ms + 999) / 1000)
        return "Good for \(total / 60):\(String(format: "%02d", total % 60))"
    }

    /// "real-debrid.com/device": the sign-in page as a person types it.
    static func address(_ url: String) -> String {
        var text = url
        for prefix in ["https://", "http://", "www."] where text.hasPrefix(prefix) { text.removeFirst(prefix.count) }
        while text.hasSuffix("/") { text.removeLast() }
        return text
    }

    /// A sign-in by code is finished once its record is the account's: the
    /// generation moved on from the one before it started, and nothing is
    /// pending for the service.
    static func signedIn(done: Bool, before: String?, now: String?, pending: Bool) -> Bool {
        done && now != nil && now != before && !pending
    }

    /// What a sign-in that went through says until its record is stored.
    static func doneWords(stored: Bool) -> String {
        stored ? "Saving" : "Kept for this session"
    }

    /// A disconnect whose tombstone could not be saved: nothing changed.
    static let notSaved = "This device could not save that, so it stays connected."
}
