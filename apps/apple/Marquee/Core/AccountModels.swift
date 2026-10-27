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
    /// A phone approved: this device is joining.
    case joining
    /// The keystore record to store (the app never looks inside it), and the
    /// approving device's name as the account knows it.
    case joined(record: JSONValue, by: String?)

    private enum Key: String, CodingKey { case phase, qr, expiresMs, record, by }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        switch try c.decode(String.self, forKey: .phase) {
        case "starting": self = .starting
        case "waiting": self = .waiting(qr: try c.decode(String.self, forKey: .qr), expiresMs: try c.decode(Int64.self, forKey: .expiresMs))
        case "joining": self = .joining
        case "joined": self = .joined(record: try c.decode(JSONValue.self, forKey: .record), by: try c.decodeIfPresent(String.self, forKey: .by))
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
    /// A deletion of the account that waits (D14), from the round's last relay answer.
    var deletion: ScheduledDeletion? = nil
    /// Roster changes another member made, until `ackAnnouncements` (plans/accounts.md 3.2).
    var announcements: [Announcement] = []
}

extension SyncReport {
    private enum Key: String, CodingKey {
        case pushed, stale, refused, waiting, pulled, applied, unreadable, newer, parked, oldEpochWrites,
             clockGapMs, rotationWaitMs, blocked, fingerprint, record, again, deletion, announcements
    }

    /// An older core sends neither `deletion` nor `announcements`: none.
    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        self.init(
            pushed: try c.decode(Int.self, forKey: .pushed), stale: try c.decode(Int.self, forKey: .stale),
            refused: try c.decode(Int.self, forKey: .refused), waiting: try c.decode(Int.self, forKey: .waiting),
            pulled: try c.decode(Int.self, forKey: .pulled), applied: try c.decode(Int.self, forKey: .applied),
            unreadable: try c.decode(Int.self, forKey: .unreadable), newer: try c.decode(Int.self, forKey: .newer),
            parked: try c.decode(Int.self, forKey: .parked), oldEpochWrites: try c.decode(Int.self, forKey: .oldEpochWrites),
            clockGapMs: try c.decode(Int64.self, forKey: .clockGapMs), rotationWaitMs: try c.decode(Int64.self, forKey: .rotationWaitMs),
            blocked: try c.decodeIfPresent(String.self, forKey: .blocked), fingerprint: try c.decode(String.self, forKey: .fingerprint),
            record: try c.decodeIfPresent(JSONValue.self, forKey: .record), again: try c.decode(Bool.self, forKey: .again),
            deletion: try c.decodeIfPresent(ScheduledDeletion.self, forKey: .deletion),
            announcements: try c.decodeIfPresent([Announcement].self, forKey: .announcements) ?? [])
    }
}

/// A deletion of the account that waits (ScheduledDeletion.ts): every device
/// shows it with Cancel; `provider` names the identity that asked from the web page.
struct ScheduledDeletion: Decodable, Hashable, Sendable {
    let dueMs: Int64
    let provider: String?
}

/// A roster change another member made (Announcement.ts): a device joined, a
/// passkey or recovery kit added, a member removed, or older ones not kept.
struct Announcement: Decodable, Hashable, Sendable, Identifiable {
    /// What `ackAnnouncements` takes.
    let id: String
    /// `device`, `passkey`, `kit`, `removed` or `dropped`.
    let kind: String
    let member: String?
    let kit: Bool
    let passkey: Bool
    /// The member that vouched for it or removed it (itself when a device left).
    let by: String?
    let name: String?
    /// For `dropped`: how many older announcements were not kept.
    let count: Int?
}

/// The account's members (Devices.ts).
struct SyncDevices: Decodable, Hashable, Sendable {
    let members: [MemberInfo]
    let fingerprint: String
    let epoch: Int64
    let rotationPending: Bool
    let record: JSONValue?
    /// The account id as base64url without padding: a new passkey's `user.id` (plans/accounts.md 3.1).
    var account: String? = nil
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
    /// A kit made from a passkey, else a recovery kit.
    var passkey: Bool = false
    /// `phone`, `tablet`, `tv`, `computer` or `web`, when the device said:
    /// only phones and tablets approve, and count as a second way in.
    var form: String? = nil
    /// A device that joined through a passkey ("Joined with a passkey").
    var viaPasskey: Bool = false
    /// A kit's newest device and when it joined: "Last used".
    var lastUsedBy: String? = nil
    var lastUsedMs: Int64? = nil
}

extension MemberInfo {
    private enum Key: String, CodingKey {
        case id, kit, name, sponsor, addedMs, thisDevice, passkey, form, viaPasskey, lastUsedBy, lastUsedMs
    }

    /// A core before accounts phase 1c sends none of the newer fields: a plain device or recovery kit.
    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        self.init(
            id: try c.decode(String.self, forKey: .id), kit: try c.decode(Bool.self, forKey: .kit),
            name: try c.decodeIfPresent(String.self, forKey: .name), sponsor: try c.decodeIfPresent(String.self, forKey: .sponsor),
            addedMs: try c.decode(Int64.self, forKey: .addedMs), thisDevice: try c.decode(Bool.self, forKey: .thisDevice),
            passkey: try c.decodeIfPresent(Bool.self, forKey: .passkey) ?? false,
            form: try c.decodeIfPresent(String.self, forKey: .form),
            viaPasskey: try c.decodeIfPresent(Bool.self, forKey: .viaPasskey) ?? false,
            lastUsedBy: try c.decodeIfPresent(String.self, forKey: .lastUsedBy),
            lastUsedMs: try c.decodeIfPresent(Int64.self, forKey: .lastUsedMs))
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
    /// Those of `devices` that joined through a passkey, newest first.
    var passkeyJoined: [String] = []
    /// The kits that go with everything ticked.
    let kits: [String]
    /// The kits each of `devices` made: they stay with a device the user unticks.
    let deviceKits: [DeviceKits]
}

/// Both sides of a different device list (ForkPreview.ts), read only: this
/// device's members, the sync server's, what taking the server's drops on its
/// own, the kits each server-side device made, whether this device is on the
/// server's side, and the head the preview showed.
struct ForkPreview: Decodable, Hashable, Sendable {
    let mine: [MemberInfo]
    let server: [MemberInfo]
    /// Members of the server's side that go whatever the user ticks.
    let drops: [String]
    /// The kits each device of the server's side made: they go with it when ticked.
    let kits: [DeviceKits]
    /// This device is on the server's side; when not, it cannot take it.
    let onServer: Bool
    /// The server's last entry as shown; continuing hands it back.
    let head: String
}

/// A device and the recovery kits it made (DeviceKits.ts).
struct DeviceKits: Decodable, Hashable, Sendable {
    let device: String
    let kits: [String]
}

/// `createRecoveryKit`'s answer: the kit (shown once, kept nowhere), its own
/// entry in the device list, and the record to store. A core that answers no
/// id still hands over the kit (its id is then unknown, never a lost kit).
struct MadeKit: Decodable, Sendable {
    let kit: String
    let id: String
    let record: JSONValue

    private enum Key: String, CodingKey { case kit, id, record }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        kit = try c.decode(String.self, forKey: .kit)
        id = try c.decodeIfPresent(String.self, forKey: .id) ?? ""
        record = try c.decode(JSONValue.self, forKey: .record)
    }

    /// The kit to hold until "I saved it", by its id.
    var held: HeldKit { HeldKit(id: id, kit: kit) }
}

extension LostPreview {
    private enum Key: String, CodingKey { case devices, passkeyJoined, kits, deviceKits }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        self.init(
            devices: try c.decode([String].self, forKey: .devices),
            passkeyJoined: try c.decodeIfPresent([String].self, forKey: .passkeyJoined) ?? [],
            kits: try c.decode([String].self, forKey: .kits),
            deviceKits: try c.decode([DeviceKits].self, forKey: .deviceKits))
    }
}

/// The `signIn` view (SignInSnapshot.ts): a relay's brokered sign-in.
enum SignInSnapshot: Decodable, Hashable, Sendable {
    case starting
    /// Open `url` in the auth session; the relay takes the flow until `expiresMs` (its time).
    case browser(url: String, expiresMs: Int64)
    case waiting
    /// The identity signs in to `account`.
    case found(ticket: String, account: String)
    /// No account yet: create one with the ticket, or link it after joining.
    case new(ticket: String)
    /// Apple's revoke flow: the receipt for an unlink or a delete.
    case receipt(receipt: String, ticket: String, account: String?)
    case failed(code: String)

    private enum Key: String, CodingKey { case phase, url, expiresMs, ticket, account, receipt, code }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        switch try c.decode(String.self, forKey: .phase) {
        case "starting": self = .starting
        case "browser": self = .browser(url: try c.decode(String.self, forKey: .url), expiresMs: try c.decode(Int64.self, forKey: .expiresMs))
        case "waiting": self = .waiting
        case "found": self = .found(ticket: try c.decode(String.self, forKey: .ticket), account: try c.decode(String.self, forKey: .account))
        case "new": self = .new(ticket: try c.decode(String.self, forKey: .ticket))
        case "receipt":
            self = .receipt(receipt: try c.decode(String.self, forKey: .receipt), ticket: try c.decode(String.self, forKey: .ticket),
                            account: try c.decodeIfPresent(String.self, forKey: .account))
        case "failed": self = .failed(code: try c.decode(String.self, forKey: .code))
        // A phase a newer core adds: wait, as for `waiting`.
        default: self = .waiting
        }
    }
}

/// A sign-in identity linked to the account (IdentityLink.ts): no subject, email or name.
struct IdentityLink: Decodable, Hashable, Sendable, Identifiable {
    let link: Int64
    let provider: String
    let createdMs: Int64

    var id: Int64 { link }
}

/// `nativeSignInStart`'s answer: the flow, the nonce for the native sheet (already
/// bound to the relay host), and the relay's deadline for it.
struct NativeStart: Decodable, Hashable, Sendable {
    let flow: String
    let nonce: String
    let expiresMs: Int64
}

/// `emailSignInStart`'s answer: the flow the typed code goes to, and the relay's deadline for it.
struct EmailStart: Decodable, Hashable, Sendable {
    let flow: String
    let expiresMs: Int64
}

/// What a pairing code says (`readPairingCode`): its sync server, and the
/// name and form the screen gives itself, cleaned by the core.
struct PairingCode: Decodable, Hashable, Sendable {
    let server: String
    let name: String?
    let form: String?
}
