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
