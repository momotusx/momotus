import Foundation

/// The profile's Trakt status (`trakt` view, plans/m7-trakt.md section 3).
struct TraktStatus: Decodable, Hashable, Sendable {
    let state: TraktState
    let user: String?
    let lastOkMs: Int64?
    /// A status and Trakt's `error` field, never a body.
    let lastError: String?
    let pendingWrites: Int
    /// The newest items Trakt could not take or give, newest first.
    let unmapped: [TraktUnmapped]
    let importing: TraktImporting?
    /// A record the app has not stored: store it, then `setTrakt(record)`.
    let record: JSONValue?
    /// The connection is the Momotus account's: every device of it uses it.
    let shared: Bool
    /// The record this device stored for the profile is no longer its own (disconnected for the account, or
    /// the account was left): delete it.
    let dropStored: Bool

    private enum Key: String, CodingKey {
        case state, sinceMs, reason, user, lastOkMs, lastError, pendingWrites, unmapped, importing, record, shared, dropStored
    }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        state = switch try c.decode(String.self, forKey: .state) {
        case "connected": .connected
        case "failing": .failing(sinceMs: try c.decode(Int64.self, forKey: .sinceMs), reason: try c.decode(String.self, forKey: .reason))
        case "reconnect": .reconnect(reason: try c.decode(String.self, forKey: .reason))
        case "thisDeviceOff": .thisDeviceOff
        case "linking": .linking
        case "offEverywhere": .offEverywhere
        case "notConfigured": .notConfigured
        default: .off
        }
        // An older core sends neither.
        shared = try c.decodeIfPresent(Bool.self, forKey: .shared) ?? false
        dropStored = try c.decodeIfPresent(Bool.self, forKey: .dropStored) ?? false
        user = try c.decodeIfPresent(String.self, forKey: .user)
        lastOkMs = try c.decodeIfPresent(Int64.self, forKey: .lastOkMs)
        lastError = try c.decodeIfPresent(String.self, forKey: .lastError)
        pendingWrites = try c.decodeIfPresent(Int.self, forKey: .pendingWrites) ?? 0
        unmapped = try c.decodeIfPresent([TraktUnmapped].self, forKey: .unmapped) ?? []
        importing = try c.decodeIfPresent(TraktImporting.self, forKey: .importing)
        let record = try c.decodeIfPresent(JSONValue.self, forKey: .record)
        self.record = record == .null ? nil : record
    }

    /// The same status without its record: what the screens read.
    func withoutRecord() -> TraktStatus {
        TraktStatus(state: state, user: user, lastOkMs: lastOkMs, lastError: lastError, pendingWrites: pendingWrites,
                    unmapped: unmapped, importing: importing, record: nil, shared: shared, dropStored: dropStored)
    }

    init(state: TraktState, user: String?, lastOkMs: Int64?, lastError: String?, pendingWrites: Int,
         unmapped: [TraktUnmapped], importing: TraktImporting?, record: JSONValue?, shared: Bool = false,
         dropStored: Bool = false) {
        (self.state, self.user, self.lastOkMs, self.lastError, self.pendingWrites) = (state, user, lastOkMs, lastError, pendingWrites)
        (self.unmapped, self.importing, self.record) = (unmapped, importing, record)
        (self.shared, self.dropStored) = (shared, dropStored)
    }
}

enum TraktState: Hashable, Sendable {
    case connected
    case failing(sinceMs: Int64, reason: String)
    case reconnect(reason: String)
    case off
    case thisDeviceOff
    /// In a Momotus account: the account's sign-in has not reached this device yet.
    case linking
    /// In a Momotus account: Trakt was disconnected on every device of it.
    case offEverywhere
    case notConfigured
}

struct TraktUnmapped: Decodable, Hashable, Sendable {
    let itemId: String
    let name: String
    let reason: String
}

struct TraktImporting: Decodable, Hashable, Sendable {
    let done: Int
    let total: Int
}

/// The `traktLogin` view: a sign-in with a code approved on trakt.tv.
enum TraktLoginSnapshot: Decodable, Hashable, Sendable {
    case starting
    case code(userCode: String, url: String, qr: String, expiresMs: Int64)
    case done(user: String)
    case expired
    case denied
    case notConfigured
    case failed(reason: String)

    private enum Key: String, CodingKey { case phase, userCode, url, qr, expiresMs, user, reason }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        self = switch try c.decode(String.self, forKey: .phase) {
        case "code": .code(userCode: try c.decode(String.self, forKey: .userCode), url: try c.decode(String.self, forKey: .url),
                           qr: try c.decode(String.self, forKey: .qr), expiresMs: try c.decode(Int64.self, forKey: .expiresMs))
        case "done": .done(user: try c.decode(String.self, forKey: .user))
        case "expired": .expired
        case "denied": .denied
        case "notConfigured": .notConfigured
        case "failed": .failed(reason: try c.decode(String.self, forKey: .reason))
        default: .starting
        }
    }
}

/// What one round did (`traktSync`).
struct TraktReport: Decodable, Hashable, Sendable {
    let pulled: Int
    let applied: Int
    let pushed: Int
    let pending: Int
    let unmapped: Int
}

/// Trakt's side of a playback (`PlaybackSnapshot.trakt`).
enum TraktPlayback: String, Decodable, Hashable, Sendable {
    case on, unmapped, off, thisDeviceOff, reconnect, linking
}
