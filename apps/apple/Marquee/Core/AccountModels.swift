import Foundation

// Import and pairing DTOs from the core, mirroring
// apps/web/src/core/generated/{ImportSnapshot,ImportSummary,PairingSnapshot}.ts.

enum ImportSnapshot: Decodable, Hashable, Sendable {
    case starting
    case waitingForLink(code: String, linkUrl: String, qrUrl: String?)
    case importing(step: String)
    case done(ImportSummary)
    case expired
    case failed(reason: String)

    private enum Key: String, CodingKey { case phase, code, linkUrl, qrUrl, step, summary, reason }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        switch try c.decode(String.self, forKey: .phase) {
        case "starting": self = .starting
        case "waitingForLink":
            self = .waitingForLink(
                code: try c.decode(String.self, forKey: .code), linkUrl: try c.decode(String.self, forKey: .linkUrl),
                qrUrl: try c.decodeIfPresent(String.self, forKey: .qrUrl))
        case "importing": self = .importing(step: try c.decode(String.self, forKey: .step))
        case "done": self = .done(try c.decode(ImportSummary.self, forKey: .summary))
        case "expired": self = .expired
        case "failed": self = .failed(reason: try c.decode(String.self, forKey: .reason))
        case let phase:
            throw DecodingError.dataCorruptedError(forKey: .phase, in: c, debugDescription: "unknown import phase \(phase)")
        }
    }
}

struct ImportSummary: Decodable, Hashable, Sendable {
    let addonsImported: Int
    /// "Name: reason" for each addon that could not be imported.
    let addonsSkipped: [String]
    let titles: Int
    let inProgress: Int
    let watched: Int
    /// Series whose watched marks could not be matched to episodes.
    let seriesUnmatched: Int
}

enum PairingSnapshot: Decodable, Hashable, Sendable {
    case starting
    /// `qr` is the text to show as a QR code.
    case waiting(qr: String, expiresMs: Int64)
    /// Both screens show this code; the user confirms they match.
    case confirm(code: String)
    /// The keystore record to store; the app never looks inside it.
    case joined(record: JSONValue)
    case expired
    case failed(reason: String)

    private enum Key: String, CodingKey { case phase, qr, expiresMs, code, record, reason }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        switch try c.decode(String.self, forKey: .phase) {
        case "starting": self = .starting
        case "waiting": self = .waiting(qr: try c.decode(String.self, forKey: .qr), expiresMs: try c.decode(Int64.self, forKey: .expiresMs))
        case "confirm": self = .confirm(code: try c.decode(String.self, forKey: .code))
        case "joined": self = .joined(record: try c.decode(JSONValue.self, forKey: .record))
        case "expired": self = .expired
        case "failed": self = .failed(reason: try c.decode(String.self, forKey: .reason))
        case let phase:
            throw DecodingError.dataCorruptedError(forKey: .phase, in: c, debugDescription: "unknown pairing phase \(phase)")
        }
    }
}

/// What one `syncNow` round did (apps/web/src/core/generated/SyncReport.ts).
struct SyncReport: Decodable, Hashable, Sendable {
    let pushed: Int
    let stale: Int
    let refused: Int
    /// Edits the relay takes only once its clock catches up; still queued.
    let waiting: Int
    let pulled: Int
    let applied: Int
    let unreadable: Int
    let newer: Int
    /// Records waiting for a key this device does not hold yet.
    let parked: Int
    /// Above 0: the relay may be hiding roster changes.
    let oldEpochWrites: Int
    /// This device's clock minus the relay's.
    let clockGapMs: Int64
    let rotationWaitMs: Int64
    /// "rotationPending", "notActive" or "relayDisagrees".
    let blocked: String?
    let fingerprint: String
    /// A changed keystore record: store it, then sync again at once.
    let record: JSONValue?
    let again: Bool
}

/// The account's members (Devices.ts).
struct SyncDevices: Decodable, Hashable, Sendable {
    let members: [MemberInfo]
    let fingerprint: String
    let epoch: Int64
    let rotationPending: Bool
    let record: JSONValue?
}

struct MemberInfo: Decodable, Hashable, Sendable, Identifiable {
    let id: String
    /// A recovery kit rather than a device.
    let kit: Bool
    let name: String?
    /// Who vouched for it.
    let sponsor: String?
    let addedMs: Int64
    let thisDevice: Bool
}

/// Any JSON, kept as is: for records the app stores and hands back but never reads.
enum JSONValue: Codable, Hashable, Sendable {
    case null
    case bool(Bool)
    /// Decimal keeps large integers (64-bit ids) exact, which Double would round.
    case number(Decimal)
    case string(String)
    case array([JSONValue])
    case object([String: JSONValue])

    init(from decoder: any Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let v = try? c.decode(Bool.self) { self = .bool(v) }
        else if let v = try? c.decode(Decimal.self) { self = .number(v) }
        else if let v = try? c.decode(String.self) { self = .string(v) }
        else if let v = try? c.decode([JSONValue].self) { self = .array(v) }
        else { self = .object(try c.decode([String: JSONValue].self)) }
    }

    /// The value as Foundation objects for JSONSerialization; numbers stay
    /// exact as NSDecimalNumber.
    var foundation: Any {
        switch self {
        case .null: NSNull()
        case .bool(let v): v
        case .number(let v): NSDecimalNumber(decimal: v)
        case .string(let v): v
        case .array(let v): v.map(\.foundation)
        case .object(let v): v.mapValues(\.foundation)
        }
    }

    func encode(to encoder: any Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .null: try c.encodeNil()
        case .bool(let v): try c.encode(v)
        case .number(let v): try c.encode(v)
        case .string(let v): try c.encode(v)
        case .array(let v): try c.encode(v)
        case .object(let v): try c.encode(v)
        }
    }
}

/// What reporting a device lost drops by default (LostPreview.ts): the devices
/// it vouched for, ticked for the user to untick, and the kits that always go.
struct LostPreview: Decodable, Hashable, Sendable {
    let devices: [String]
    let kits: [String]
}
