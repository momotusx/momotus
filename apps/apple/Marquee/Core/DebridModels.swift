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
