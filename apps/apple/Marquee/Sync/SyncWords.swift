import Foundation

/// Account (the sync page) copy, as the web has it (apps/web/src/lib/words.ts,
/// `syncWords`). No protocol words: a device list, not a roster; "the 12
/// characters to compare", not a fingerprint.
enum SyncWords {
    static let title = "Account"
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
    static let showKit = "Show"
    static let hideKit = "Hide"
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
    static let noLockAsk = "This device has no passcode, so nothing can check that it is you. Stop syncing on it anyway? Your library stays here."
    static let tvSure = "Nobody can confirm it is you on this TV. Are you sure?"
    static let syncAgain = "Sync again"
    // A different device list: the compare page
    static let compare = "Compare device lists"
    static let forkTitle = "Compare device lists"
    static let forkLede = "The sync server lists different devices than this device knows. That happens when the server was restored from a backup, or when someone changed it. Check its list, tick anything you do not recognise, then keep syncing with it."
    static let forkLoading = "Asking the sync server for its list"
    static let forkServer = "On the sync server's list"
    static let forkServerHelp = "Ticked devices are removed when you continue. Those marked as going are removed anyway: this device had removed them, or they are recovery kits only the server knows."
    static let forkGoes = "Removed anyway"
    static let goesWithTicked = "Goes with {name}"
    static let onlyOnServer = "Only on the server's list"
    static let forkMine = "Only this device knew these"
    static let forkMineHelp = "They are not on the server's list, so they are gone once you continue. A device among them pairs again to come back; make a new recovery kit if one of them was yours."
    static let forkSame = "Both lists name the same devices."
    static let forkGo = "Continue with the server's list"
    static let forkGoing = "Continuing"
    static let forkOffList = "The sync server's list no longer includes this device: it was removed there. Pair this device again from one that still syncs."
    static let forkChanged = "The sync server's list changed while you looked. Here it is again; check it before you continue."
    static let forkTokenLost = "This device cannot read the sync server's list: the server no longer knows it. Sync from another of your devices, or pair this one again."
    static let forkNone = "This device and the sync server list the same devices. There is nothing to compare."
    static let forkDone = "This device now syncs with the server's list."
    // Leaving here only
    static let left = "This device no longer syncs. Your library stays here."
    static let leaveHere = "Stop syncing here only"
    static let leaveHereAsk = "Stop syncing here only?"
    static let leaveHereText = "This device forgets the account without telling the sync server, which lists it until another device removes it. Your library stays here, and you can pair this device again."
    static let leftHere = "This device no longer syncs. The sync server still lists it until another device removes it; you can pair it again."
    static let wrongRecord = "This device does not hold this sync account any more, so there is nothing to stop syncing here. Forget it on this device to clear it."
    // Delete the account
    static let deleteTitle = "Delete account"
    static let deleteText = "Deletes your account from the sync service for all your devices. Each device keeps its own copy of your library."
    static let deleteButton = "Delete account"
    static let deleteAsk = "Your profiles, library, progress and settings are deleted from the sync service. This device keeps its own copy."
    static let deleting = "Deleting"
    static let accountGone = "Your Marquee account was deleted. Your library stays on this device."
    static let ok = "OK"
    static let forgetHere = "Forget it on this device"
    static let forgetHereText = "This device deletes the account's key from its Keychain. Your library stays here."
    static let keyUnreadable = "This device cannot read its sync key from the Keychain right now, so it cannot tell whether it syncs. Try again in a moment; if it keeps failing, restart the device."
    static let keyLeftover = "A sync key from an earlier install of Marquee is still in this device's Keychain: Marquee could not delete it, so it does not use it. Try again."
    static let keyLeft = "This device stopped syncing, but its sync key is still in the Keychain: Marquee could not delete it. Try again."
    static let listStale = "This list and the 12 characters may be out of date: this device could not read the list again."
    static let removeStale = "The list could not be read again, so the recovery kits named here may be out of date. Try again before you remove."
    static let dismiss = "Dismiss"
    static let offLine = "Keep your library, progress and addons the same on every device. Pair, start an account or use a recovery kit."
    static let onLine = "Devices, recovery kit, stopping."
    // Add a device
    static let addLede = "On the new device, open Settings, then Account, then Pair this device. It shows a code."
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
    static let reviewAsk = "Approve a new device?"
    static let reviewText = "Approve it only if the new device is in front of you and shows the code you scanned. Whoever confirms the six digits next joins your account."
    static let differ = "They differ"
    static let differEnded = "The approval ended and your sync key changed: whoever answered cannot join with it, and a device that already did is removed, with any device it added."
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
    static let copyKitHelp = "Copying puts it on this device's clipboard for 1 minute, never on your other devices. Paste it into your password manager right away."
    static let kitCopied = "Copied for 1 minute. Paste it into your password manager now."
    static let savedKit = "I saved it"
    static let kitWaiting = "Your new recovery kit is not saved yet. Show it again and save it."
    static let showKitAgain = "Show the kit"
    static let kitAskShow = "The kit is shown again only after your passcode, Face ID or Touch ID."
    static let kitUnseen = "A recovery kit was made here and not said saved before Marquee let go of it (when the app closed, or five minutes after you left its page), so it cannot be shown again. Make a new one: Marquee removes the unseen one."
    static let kitExpired = "A recovery kit was made here and left unsaved for five minutes after you left its page, so Marquee let go of it and cannot show it again. Make a new one: Marquee removes the unseen one."
    static let makeNewKit = "Make a new kit"
    static let kitIdUnknown = "Marquee could not tell which recovery kit in the device list is this one. If you do not save it, check the recovery kits in the list."
    static let kitUnseenUnknown = "A recovery kit was made here and never said saved, and Marquee cannot tell which one it is. Check the recovery kits in the list and remove those you do not hold."
    static let checked = "I checked"
    static let macCapture = "Screen recording and screen sharing can capture what this window shows. So on a Mac the kit shows for 20 seconds after you confirm it is you, and Hold to show works for a minute after that."
    static let kitHidden = "Hidden. Press and hold Hold to show, or choose Show for 20 seconds."
    static let kitHiddenLocked = "Hidden. Show for 20 seconds asks for your password or Touch ID first."
    static let holdToShow = "Hold to show"
    static let showForAWhile = "Show for 20 seconds"

    /// The seconds left before a Mac hides the kit again.
    static func hiddenIn(_ seconds: Int) -> String {
        "Hides in \(count(seconds, "second", "seconds"))"
    }

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
    static let authShowKit = "show your recovery kit again"
    static let authApprove = "approve a new device"
    static let authDiffer = "end the approval of a device"
    static let authServerList = "keep syncing with the sync server's device list"
    static let authLeave = "stop syncing on this device"
    static let authDelete = "delete your sync account"

    /// Fills `{name}` in a sync line.
    static func named(_ line: String, _ name: String) -> String {
        line.replacingOccurrences(of: "{name}", with: name)
    }

    /// "1 device" or "3 devices".
    static func count(_ n: Int, _ one: String, _ many: String) -> String {
        "\(n) \(n == 1 ? one : many)"
    }

    /// The confirm line for removing another device, with the recovery kits made on it, which go with it.
    static func removeDeviceAsk(_ name: String, kits: Int = 0) -> String {
        "Remove \(name)? It stops syncing and cannot read anything you change from now on. Devices it added stay."
            + (kits == 0 ? "" : kits == 1 ? " The recovery kit made on it stops working too." : " The \(kits) recovery kits made on it stop working too.")
    }

    /// Failures since this device joined, kept until the user dismisses them.
    static func failedSince(_ failed: SyncFailed) -> String? {
        let parts = [
            failed.refused > 0 ? "\(count(failed.refused, "edit", "edits")) from this device could not be sent and \(failed.refused == 1 ? "stays" : "stay") here only" : nil,
            failed.unreadable > 0 ? "\(count(failed.unreadable, "change", "changes")) from other devices could not be read" : nil,
            failed.newer > 0 ? "\(count(failed.newer, "change", "changes")) from a newer version of Marquee \(failed.newer == 1 ? "waits" : "wait") for an update here" : nil,
        ].compactMap { $0 }
        return parts.isEmpty ? nil : "Since this device joined, \(parts.joined(separator: ", and "))."
    }

    /// The Settings row under Account: what is wrong or waits
    /// first, then the time; never "Synced" over a warning.
    static func summary(joined: Bool, problem: String?, failed: String?, notes: [String], at: Date?) -> String {
        if let problem { return problem }
        guard joined else { return offLine }
        if let warning = failed ?? notes.first { return warning }
        guard let at else { return onLine }
        return "Synced \(at.formatted(date: .omitted, time: .shortened)). \(onLine)"
    }

    /// The button that continues with the server's list, naming what goes.
    static func forkButton(devices: Int, kits: Int) -> String {
        let parts = [devices > 0 ? count(devices, "device", "devices") : nil, kits > 0 ? count(kits, "recovery kit", "recovery kits") : nil].compactMap { $0 }
        return parts.isEmpty ? forkGo : "Continue and remove \(parts.joined(separator: " and "))"
    }

    /// On a ticked row of the server's list: the kits that go with it.
    static func goesWith(_ kits: Int) -> String {
        kits == 1 ? "Its recovery kit goes with it." : "Its \(kits) recovery kits go with it."
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
    case round, start, recover, approve, devices, kit, remove, lost, leave, delete, differ, compare, serverList
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
        case "accountGone": return SyncWords.accountGone
        case "fork": return "The sync server lists different devices than this device knows. Compare the lists to keep syncing."
        case "rosterChanged": return SyncWords.forkChanged
        case "wrongRecord": return SyncWords.wrongRecord
        case "wrongRelay": return "This is not the sync server this device joined. Try again, or forget the account here and pair again."
        case "relayBehind": return "The sync server lost device changes it cannot get back. Pair this device again from one that still syncs."
        case "notJoined": return "This device has not joined a sync account yet."
        case "unsent": return "Some changes have not been sent yet. Sync again first, or stop anyway and they stay only here."
        case "insecure": return "The sync server address must start with https."
        case "kitUnknown": return SyncWords.wrongKit
        // The app's own: the Keychain kept a new record from it, so nothing is sent until it is stored.
        case "storage": return "This device could not store the sync account's key, so sync is paused. Free some storage, then sync again."
        case "unconfirmed":
            return action == .differ ? "The approval is still open: this device did not confirm it is you." : "Nothing changed: this device did not confirm it is you."
        case "noPasscode": return "Set a device passcode first: Marquee asks for it before changing your devices or making a kit."
        case "keyUnreadable": return SyncWords.keyUnreadable
        // A kit, a passkey or a wipe is the main profile's (D13, D17).
        case "notOwner": return "Only the main profile can do that."
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
        if action == .compare, message == "http status 401" { return SyncWords.forkTokenLost }
        if action == .round { return "The sync server did not answer. Your changes wait here and go out on the next sync." }
        if action == .devices { return "The sync server did not answer, so this list may be out of date. Try again in a moment." }
        let notDone = switch action {
        case .round, .devices, .serverList: "Nothing changed."
        case .approve: "Nothing was approved."
        case .kit: "No kit was made."
        case .remove, .lost: "Nothing was removed."
        case .leave: "This device still syncs."
        // The relay may have deleted it before its answer was lost: claim nothing.
        case .delete: "The deletion was not confirmed."
        case .differ: "The approval is still open."
        case .compare: "The lists could not be compared."
        case .start, .recover: "This device did not join."
        }
        return "\(notDone) The sync server did not answer. Try again in a moment."
    }
}

/// Changes that failed since this device joined: a clean round does not
/// clear them, only the user (Dismiss), a join or a leave.
struct SyncFailed: Equatable {
    var refused = 0
    var unreadable = 0
    var newer = 0

    func adding(_ report: SyncReport) -> SyncFailed {
        SyncFailed(refused: refused + report.refused, unreadable: unreadable + report.unreadable, newer: newer + report.newer)
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
    /// A pairing code as the new device shows it: the TV's link
    /// `https://<link host>/pair#v=3&...`, or `marquee:pair?v=2&...` from an
    /// older build. Any other QR in view is ignored.
    static func isPairingCode(_ text: String) -> Bool {
        let code = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if code.hasPrefix("marquee:pair?") { return true }
        guard let link = URL(string: code), link.scheme == "https", link.path() == "/pair",
              let fields = link.fragment() else { return false }
        return fields.split(separator: "&").contains("v=3")
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

    /// What asking who is there comes to (Android's `guarded`).
    enum OwnerStep: Equatable {
        /// The OS prompt (passcode, Face ID or Touch ID).
        case prompt
        /// Asked twice already: it runs.
        case run
        /// No lock to ask with: the page asks a second time in its own words.
        case askAgain
        /// No lock, and the op is more than ending access: nothing can confirm it.
        case refuse
    }

    /// Delete account shows on the owner profile only: the account holds
    /// every profile (D15).
    static func mayDeleteAccount(_ snapshot: ProfilesSnapshot?) -> Bool {
        snapshot?.current?.isOwner == true
    }

    /// Make a recovery kit shows on the owner profile only: a kit is a copy
    /// of the whole account key (D17).
    static func mayMakeKit(_ snapshot: ProfilesSnapshot?) -> Bool {
        mayDeleteAccount(snapshot)
    }

    /// The codes after which this device forgets the account (its library
    /// stays): it was removed, or the account was deleted.
    static func forgets(_ code: String?) -> Bool {
        code == "dropped" || code == "accountGone"
    }

    /// With a lock the OS prompt; without one, stopping syncing (it only ends
    /// this device's access) asks a second time and runs, anything else is refused.
    static func ownerStep(hasLock: Bool, endsAccess: Bool, askedAgain: Bool) -> OwnerStep {
        if hasLock { return .prompt }
        guard endsAccess else { return .refuse }
        return askedAgain ? .run : .askAgain
    }

    /// The account has no kit the user holds: none in the list but those that went unseen here.
    static func noKit(_ members: [MemberInfo], unseen: [String]) -> Bool {
        !members.contains { $0.kit && !unseen.contains($0.id) }
    }

    /// What a lost report drops: the lost device and what it added that the user did not untick.
    static func lostDrop(lost: String, added: [String], keep: Set<String>) -> [String] {
        [lost] + added.filter { !keep.contains($0) }
    }

    /// The kits that go with a lost report, from the core's answer: every kit
    /// of the preview less those of the devices the user unticked (they keep theirs).
    static func lostKitsGoing(_ preview: LostPreview, keep: Set<String>) -> [String] {
        let kept = Set(preview.deviceKits.filter { keep.contains($0.device) }.flatMap(\.kits))
        return preview.kits.filter { !kept.contains($0) }
    }

    /// The recovery kits made on `device`: removing it stops them too.
    static func kitsMade(by device: String, in members: [MemberInfo]) -> Int {
        members.filter { $0.kit && $0.sponsor == device }.count
    }

    /// A row VoiceOver reads: its name and its line, so two "Recovery kit" rows differ.
    static func spokenRow(_ name: String, _ detail: String) -> String {
        "\(name), \(detail)"
    }

    /// What the hub warns about, in order: the round's problem, failures
    /// since joining, the round's notes, and a list that may be out of date.
    static func warnings(problem: String?, failed: String?, notes: [String], listStale: Bool) -> [String] {
        [problem, failed].compactMap { $0 } + notes + (listStale ? [SyncWords.listStale] : [])
    }

    /// What VoiceOver hears when the warnings change: only those that are
    /// new, once; a round that repeats them says nothing.
    static func announcement(old: [String], new: [String]) -> String? {
        let fresh = new.filter { !old.contains($0) }
        return fresh.isEmpty ? nil : fresh.joined(separator: " ")
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

    /// The devices only one side of a different device list has, by id.
    static func forkSides(mine: [MemberInfo], server: [MemberInfo]) -> (serverOnly: [MemberInfo], mineOnly: [MemberInfo]) {
        let here = Set(mine.map(\.id)), there = Set(server.map(\.id))
        return (server.filter { !here.contains($0.id) }, mine.filter { !there.contains($0.id) })
    }

    /// What continuing with the server's list removes besides its own drops:
    /// the user's ticks among its members, never this device.
    static func forkDrop(server: [MemberInfo], drops: [String], ticked: Set<String>) -> [String] {
        server.filter { !$0.thisDevice && !drops.contains($0.id) && ticked.contains($0.id) }.map(\.id)
    }

    /// The kits a device of the server's list made that go with it when ticked
    /// (not those that go anyway).
    static func forkKits(of device: String, in preview: ForkPreview) -> [String] {
        (preview.kits.first { $0.device == device }?.kits ?? []).filter { !preview.drops.contains($0) }
    }

    /// How a row of the server's list reads.
    enum ForkRow: Equatable {
        case thisDevice
        /// Removed whatever the user ticks.
        case goes
        /// A kit whose maker is ticked: it goes with it.
        case withTicked(String)
        /// The user decides.
        case tickable
    }

    static func forkRow(_ member: MemberInfo, in preview: ForkPreview, ticked: Set<String>) -> ForkRow {
        if member.thisDevice { return .thisDevice }
        if preview.drops.contains(member.id) { return .goes }
        if member.kit, let maker = member.sponsor, ticked.contains(maker) {
            return .withTicked((preview.server + preview.mine).first { $0.id == maker }.map(memberName) ?? "a removed device")
        }
        return .tickable
    }

    /// The button's counts: ticked devices, and every kit that goes with the ticks.
    static func forkCounts(_ preview: ForkPreview, ticked: Set<String>) -> (devices: Int, kits: Int) {
        let drop = forkDrop(server: preview.server, drops: preview.drops, ticked: ticked)
        let isKit = { (id: String) in preview.server.contains { $0.id == id && $0.kit } }
        let kits = Set(drop.filter(isKit) + drop.flatMap { forkKits(of: $0, in: preview) })
        return (drop.filter { !isKit($0) }.count, kits.count)
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
