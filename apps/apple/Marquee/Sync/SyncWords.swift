import Foundation

/// Devices and sync copy, as the web has it (apps/web/src/lib/words.ts,
/// `syncWords`). No protocol words: a device list, not a roster; "the 12
/// characters to compare", not a fingerprint.
enum SyncWords {
    static let title = "Devices and sync"
    static let lede = "Sync keeps your library, progress, addons and settings the same on all your devices. The sync server stores them sealed and cannot read them."
    static let pairTitle = "Pair this device"
    static let pairText = "Show a code here and approve it on a device that already syncs."
    static let startTitle = "Start a sync account"
    static let startText = "For your first device. Other devices join it with a code, so nothing is typed on them."
    static let recoverTitle = "Use a recovery kit"
    static let recoverText = "Join your account again when you have none of your devices."
    static let recoverLede = "Joins your account as a new device. The kit is the 32 letters and digits you saved when you made it."
    static let server = "Sync server"
    static let serverHint = "https://sync.example.org"
    static let device = "This device"
    static let deviceHelp = "Your other devices list it by this name."
    static let invite = "Invite code"
    static let inviteHint = "Only if your sync server asks for one"
    static let kit = "Recovery kit"
    static let kitHint = "XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX"
    static let badAddress = "Enter the address without a user name or password."
    static let noKitEntered = "Enter the recovery kit."
    static let startButton = "Start the account"
    static let starting = "Starting the account"
    static let recoverButton = "Join with the kit"
    static let recovering = "Joining"
    static let wrongKit = "The sync server does not know this kit. Check the letters and try again."
    static let replaceAsk = "This device already syncs with an account"
    static let replaceText = "Joining another account replaces it here. Your library stays. Your other account still lists this device until you remove it there."
    static let replaceButton = "Join the other account"
    static let syncNow = "Sync now"
    static let syncing = "Syncing"
    static let notSynced = "Not synced yet"
    static let compareLabel = "Compare on each device"
    static let compareHelp = "Once they have synced, all your devices show these same 12 characters. If one shows different ones, the sync server may be hiding a device change."
    static let devices = "Devices"
    static let loadingDevices = "Asking the sync server for your devices"
    static let thisDevice = "This device"
    static let unnamed = "Unnamed device"
    static let unknown = "Unknown device"
    static let unknownDetail = "Not in this device's list yet. It goes with the others."
    static let kitName = "Recovery kit"
    static let remove = "Remove"
    static let reportLost = "Report lost"
    static let cancel = "Cancel"
    static let addTitle = "Add a device"
    static let addText = "Approve a new device with the code it shows."
    static let kitTitle = "Make a recovery kit"
    static let kitText = "Get your account back if you lose all your devices."
    static let noKit = "This account has no recovery kit. If you lose all your devices, your synced data cannot be read again. Make one and keep it somewhere private."
    static let afterRecover = "You joined with a recovery kit. Make a new one now, and remove the device you lost."
    static let tvOnly = "On your phone or computer"
    static let tvAdd = "Add devices from your phone or computer: a TV has no camera to scan the code."
    static let tvKit = "Make recovery kits on your phone or computer: on a TV, anyone in the room can read one."
    static let tvAfterRecover = "You joined with a recovery kit. Make a new one on your phone or computer, and remove the device you lost."
    static let leaveTitle = "Stop syncing on this device"
    static let leaveText = "Your library stays here. Edits not sent yet go out first."
    static let leaveButton = "Stop syncing"
    static let leaveAnyway = "Stop anyway"
    static let syncAgain = "Sync again"
    static let useServer = "Use the server's device list"
    static let forgetHere = "Forget it on this device"
    static let forgetHereText = "Forgetting needs no sync server and keeps your library. Your other devices still list this one until you remove it there."
    // Add a device
    static let addLede = "On the new device, open Settings, then Devices and sync, then Pair this device. It shows a code."
    static let addWarning = "Approve only a code on a screen in front of you. Whoever controls that screen gets your account."
    static let scan = "Scan the code"
    static let scanning = "Point the camera at the code on the other device."
    static let noCamera = "The camera is not available. Paste the code instead."
    static let pasteLabel = "Or paste the code"
    static let pasteOnly = "Paste the code"
    static let pasteHint = "marquee:pair?..."
    static let notACode = "That is not a pairing code. It starts with marquee:pair."
    static let approve = "Approve"
    static let approving = "Approving"
    static let approvedStep = "The new device now shows six digits. They must be these:"
    static let approvedHelp = "If they match, confirm on the new device. If they differ, say so there, and nothing is added."
    static let done = "Done"
    // The joining device's side
    static let copyCode = "Copy the code"
    static let copied = "Copied"
    static let copyHelp = "No camera? Copy the code, then paste it on the other device under Add a device."
    // Recovery kit
    static let kitLede = "A recovery kit gets your account back when you have none of your devices. Anyone who has it can join your account, so keep it somewhere private, like a password manager."
    static let kitOnce = "Marquee shows the kit once and keeps no copy."
    static let makeKit = "Make the kit"
    static let makingKit = "Making the kit"
    static let kitShown = "Your recovery kit"
    static let kitWarning = "Save it now. It is not shown again, and anyone who has it can join your account."
    static let copyKit = "Copy"
    static let copyKitHelp = "Copying puts it on this device's clipboard for 2 minutes, never on your other devices. Paste it into your password manager right away."
    static let kitCopied = "Copied for 2 minutes. Paste it into your password manager now."
    static let savedKit = "I saved it"
    // Remove and report lost
    static let removeKitAsk = "Remove this recovery kit? It stops working."
    static let lostLede = "{name} is removed from your account and cannot read anything you change from now on."
    static let lostAdded = "Devices added through {name}"
    static let lostAddedHelp = "Whoever has {name} may have these too, so they are removed as well. Untick the ones you still have."
    static let lostChecking = "Checking what {name} added"
    static let removed = "Removed"
    static let retry = "Try again"
    // Why this device asks who is there (LocalAuthentication)
    static let authRemove = "remove a device from your sync account"
    static let authLost = "report a device lost"
    static let authKit = "make a recovery kit"

    /// Fills `{name}` in a sync line.
    static func named(_ line: String, _ name: String) -> String {
        line.replacingOccurrences(of: "{name}", with: name)
    }

    /// "1 device" or "3 devices".
    static func count(_ n: Int, _ one: String, _ many: String) -> String {
        "\(n) \(n == 1 ? one : many)"
    }

    /// The confirm line for removing another device.
    static func removeDeviceAsk(_ name: String) -> String {
        "Remove \(name)? It stops syncing and cannot read anything you change from now on. Devices it added stay."
    }

    /// The step before approving a code: which sync server it joins through.
    static func approveAsk(host: String) -> String {
        "Approve a device from \(host)?"
    }

    /// The button that confirms a lost report: it names how many devices go.
    static func lostButton(_ devices: Int) -> String {
        "Remove \(count(devices, "device", "devices"))"
    }

    /// The kits a lost report removes, always.
    static func lostKits(_ kits: Int) -> String {
        (kits == 1 ? "1 recovery kit made on these devices stops" : "\(kits) recovery kits made on these devices stop")
            + " working. Make a new one afterwards."
    }

    /// A member's second line: when it was added, and through which device.
    static func addedLine(kit: Bool, addedMs: Int64, by: String?) -> String {
        let day = Date(timeIntervalSince1970: TimeInterval(addedMs) / 1000)
            .formatted(.dateTime.month(.abbreviated).day().year().locale(Locale(identifier: "en_US")))
        if kit { return by.map { "Made \(day) on \($0)" } ?? "Made \(day)" }
        return by.map { "Added \(day) through \($0)" } ?? "Added \(day), the first device"
    }
}

/// Where a sync failure happened: some answers mean something only there (a
/// 403 when starting is an invite, a 404 on a kit an unknown kit, a 404 on a
/// code a used one).
enum SyncAction: Equatable, Sendable {
    case round, start, recover, approve, devices, kit, remove, lost, leave
}

/// A sync failure in plain words; `code` names the ones with a fix.
struct SyncProblem: Equatable {
    let code: String?
    let message: String
    var action: SyncAction = .round

    init(code: String?, message: String, action: SyncAction = .round) {
        self.code = code
        self.message = message
        self.action = action
    }

    /// From a thrown error: the core's code and words.
    init(_ error: any Error, action: SyncAction) {
        let core = error as? CoreError
        self.init(code: core?.code, message: core?.message ?? error.localizedDescription, action: action)
    }

    var words: String {
        switch code {
        case "dropped": return "This device was removed from your sync account. Your library stays here as local data."
        case "fork": return "The sync server lists different devices than this device knows. Use the server's list to keep syncing."
        case "wrongRelay": return "This is not the sync server this device joined. Try again, or forget the account here and pair again."
        case "relayBehind": return "The sync server lost device changes it cannot get back. Pair this device again from one that still syncs."
        case "notJoined": return "This device has not joined a sync account yet."
        case "unsent": return "Some changes have not been sent yet. Sync again first, or stop anyway and they stay only here."
        case "insecure": return "The sync server address must start with https."
        // The app's own: the Keychain kept a new record from it, so nothing is sent until it is stored.
        case "storage": return "This device could not store the sync account's key, so sync is paused. Free some storage, then sync again."
        case "unconfirmed": return "Nothing changed: this device did not confirm it is you."
        case "noPasscode": return "Set a device passcode first: Marquee asks for it before changing your devices or making a kit."
        default: break
        }
        if action == .start, message == "http status 403" { return "This sync server needs an invite code, or the one entered is wrong." }
        if action == .recover, message == "http status 404" { return SyncWords.wrongKit }
        if action == .approve, message == "http status 404" || message == "http status 410" {
            return "This code expired or was used already. Ask the new device for a new one."
        }
        // The core's own sentences are kept; anything else may name the server's address.
        if message.hasPrefix("roster: ") { return PairingWords.problem("pairing: " + message.dropFirst("roster: ".count)) }
        for kept in ["pairing: ", "invalid url", "blocked private address", "the relay must use https"] where message.hasPrefix(kept) {
            return PairingWords.problem(message)
        }
        switch action {
        case .round: return "The sync server did not answer. Your changes wait here and go out on the next sync."
        case .devices: return "The sync server did not answer, so this list may be out of date. Try again in a moment."
        default: return "The sync server did not answer. Try again in a moment."
        }
    }
}

extension SyncReport {
    /// What a round's facts mean for the user (plans/core-protocol.md, `syncNow`).
    var notes: [String] {
        var notes: [String] = []
        let gap = abs(clockGapMs)
        if gap > 600_000 {
            let minutes = Int((Double(gap) / 60_000).rounded())
            notes.append("The sync server's clock and this device's disagree by \(SyncWords.count(minutes, "minute", "minutes")), so edits sync in the order they are sent.")
        }
        if waiting > 0 { notes.append("\(waiting == 1 ? "1 edit waits" : "\(waiting) edits wait") for the sync server's clock to catch up. Nothing is lost.") }
        if parked > 0 {
            notes.append("\(parked == 1 ? "1 change from another device waits" : "\(parked) changes from other devices wait") until this device catches up with a device change.")
        }
        if oldEpochWrites > 0 { notes.append("The sync server may be hiding a device change. Compare the 12 characters below on each device.") }
        if rotationWaitMs > 0 {
            let minutes = max(1, Int((Double(rotationWaitMs) / 60_000).rounded(.up)))
            notes.append("Edits wait \(SyncWords.count(minutes, "minute", "minutes")) more: the sync server was restored, and other devices get a chance to catch up first.")
        }
        if refused > 0 {
            notes.append("\(refused == 1 ? "1 edit was" : "\(refused) edits were") too large or malformed for the sync server, so \(refused == 1 ? "it stays" : "they stay") only on this device.")
        }
        if unreadable > 0 {
            notes.append("\(unreadable == 1 ? "1 change" : "\(unreadable) changes") from another device did not open with this account's key and were skipped.")
        }
        if newer > 0 {
            notes.append("\(newer == 1 ? "1 change comes" : "\(newer) changes come") from a newer version of Marquee. Update the app to see \(newer == 1 ? "it" : "them").")
        }
        switch blocked {
        case "rotationPending": notes.append("Edits wait until the account finishes a device change.")
        case "notActive": notes.append("The sync server does not take edits from this device yet, so they were not sent.")
        case "relayDisagrees": notes.append("The sync server disagrees about your device list, so edits were not sent. Compare the 12 characters below on each device.")
        default: break
        }
        return notes
    }
}

/// Pairing in words, as Android's `SyncCopy` has it.
enum PairingWords {
    /// A join the Keychain would not keep: it holds until the app closes.
    static let sessionOnly = "This device could not store the sync account's key: the join lasts until the app closes, and sync is paused."

    /// Why pairing stopped. The core's own pairing reasons are fixed sentences
    /// and are kept; a connection or address error is replaced, since its text
    /// can name the server's address.
    static func problem(_ reason: String) -> String {
        if reason.hasPrefix("pairing: ") {
            let why = reason.dropFirst("pairing: ".count)
            return why.prefix(1).uppercased() + why.dropFirst() + "."
        }
        if reason.hasPrefix("the relay must use https") { return SyncProblem(code: "insecure", message: reason).words }
        if reason.hasPrefix("invalid url") { return "That is not a sync server address. It looks like https://sync.example.org." }
        if reason.hasPrefix("blocked private address") { return "This device does not connect to that address." }
        return "The sync server did not answer. Check the address and try again."
    }

    /// An address with a host and no user name or password in it: anything
    /// else is never kept or sent.
    static func plainAddress(_ text: String) -> Bool {
        guard let url = URL(string: text), url.host() != nil else { return false }
        return url.user() == nil && url.password() == nil
    }
}

/// Sync rules the screens share (the web's lib/sync.ts).
enum SyncRules {
    /// A pairing code as the new device shows it; any other QR in view is ignored.
    static func isPairingCode(_ text: String) -> Bool {
        text.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("marquee:pair?")
    }

    /// The sync server a pairing code joins through, for the step before
    /// approving it; nil when the code names none.
    static func pairingHost(_ code: String) -> String? {
        let items = URLComponents(string: code.trimmingCharacters(in: .whitespacesAndNewlines))?.queryItems ?? []
        guard let server = items.first(where: { $0.name == "s" })?.value, let host = URL(string: server)?.host(), !host.isEmpty else { return nil }
        return host
    }

    /// "VW4BV9DJD5EZ" to "VW4B V9DJ D5EZ", for reading aloud.
    static func fingerprintGroups(_ fingerprint: String) -> String {
        stride(from: 0, to: fingerprint.count, by: 4).map { start in
            let begin = fingerprint.index(fingerprint.startIndex, offsetBy: start)
            return String(fingerprint[begin..<(fingerprint.index(begin, offsetBy: 4, limitedBy: fingerprint.endIndex) ?? fingerprint.endIndex)])
        }
        .joined(separator: " ")
    }

    /// This device first, then the other devices and then the kits, oldest first.
    static func sortMembers(_ members: [MemberInfo]) -> [MemberInfo] {
        members.sorted { a, b in
            if a.thisDevice != b.thisDevice { return a.thisDevice }
            if a.kit != b.kit { return !a.kit }
            return a.addedMs < b.addedMs
        }
    }

    static func memberName(_ member: MemberInfo) -> String {
        member.kit ? SyncWords.kitName : (member.name.flatMap { $0.isEmpty ? nil : $0 } ?? SyncWords.unnamed)
    }

    /// The name of the device that added `member`, or nil for the first device.
    static func sponsorName(_ member: MemberInfo, in members: [MemberInfo]) -> String? {
        guard let sponsor = member.sponsor else { return nil }
        return members.first { $0.id == sponsor }.map(memberName) ?? "a removed device"
    }

    /// What a lost report drops: the lost device and what it added that the user did not untick.
    static func lostDrop(lost: String, added: [String], keep: Set<String>) -> [String] {
        [lost] + added.filter { !keep.contains($0) }
    }

    /// The kits that go with a lost report, as the core works them out: every
    /// kit of the preview whose maker does not stay (a device the user unticked
    /// keeps its kits).
    static func kitsGoing(previewKits: [String], members: [MemberInfo], drop: [String]) -> [String] {
        let ids = Set(members.map(\.id))
        return previewKits.filter { kit in
            guard let maker = members.first(where: { $0.id == kit })?.sponsor else { return true }
            return !ids.contains(maker) || drop.contains(maker)
        }
    }

    /// One device a lost report would drop, as the page lists it.
    struct LostRow: Equatable, Identifiable {
        let id: String
        let name: String
        let detail: String
    }

    /// The preview's devices with their names from a fresh device list, oldest
    /// first; an id the list does not have yet is still listed, as an unknown
    /// device, so nothing goes unseen.
    static func lostRows(preview: LostPreview, members: [MemberInfo]) -> [LostRow] {
        let known = sortMembers(members.filter { preview.devices.contains($0.id) }).map {
            LostRow(id: $0.id, name: memberName($0), detail: SyncWords.addedLine(kit: false, addedMs: $0.addedMs, by: sponsorName($0, in: members)))
        }
        let unknown = preview.devices.filter { id in !members.contains { $0.id == id } }.map {
            LostRow(id: $0, name: SyncWords.unknown, detail: SyncWords.unknownDetail)
        }
        return known + unknown
    }

    /// The record's revision (`SyncConfig.revision`); 0 when it has none.
    static func revision(_ record: JSONValue?) -> Decimal {
        if case .object(let fields)? = record, case .number(let n)? = fields["revision"] { return n }
        return 0
    }

    /// Records carry a revision and the app keeps the highest: an answer that
    /// arrives after a newer record was stored never rolls it back.
    static func newer(_ held: JSONValue?, _ next: JSONValue) -> JSONValue {
        guard let held, revision(held) > revision(next) else { return next }
        return held
    }
}
