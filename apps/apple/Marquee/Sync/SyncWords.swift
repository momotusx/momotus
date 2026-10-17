import Foundation

/// A sync failure in plain words; `code` names the ones with a fix.
struct SyncProblem: Equatable {
    let code: String?
    let message: String

    var words: String {
        switch code {
        case "dropped": "This device was removed from your sync account. Your library stays here as local data."
        case "fork": "The sync server lists different devices than this device knows. Use the server's list to keep syncing."
        case "wrongRelay": "This is not the sync server this device joined. Check the server address."
        case "relayBehind": "The sync server lost device entries it cannot get back. Pair this device again from one that still syncs."
        case "notJoined": "This device has not joined a sync account yet."
        case "unsent": "Some changes have not been sent yet. Sync again first, or leave anyway and they stay only here."
        case "insecure": "The sync server address must start with https."
        default: message
        }
    }
}

extension SyncReport {
    /// Things worth telling the user after a round, in words.
    var notes: [String] {
        var notes: [String] = []
        if abs(clockGapMs) > 600_000 {
            notes.append("The sync server's clock and this device's disagree by \(abs(clockGapMs) / 60_000) minutes, so edits sync in the order they are sent.")
        }
        if waiting > 0 { notes.append("\(waiting) edits wait for the sync server's clock to catch up.") }
        if parked > 0 { notes.append("\(parked) records wait for a key this device does not have yet.") }
        if oldEpochWrites > 0 { notes.append("The sync server may be hiding device changes. Compare the fingerprint on each device.") }
        if rotationWaitMs > 0 { notes.append("Changes wait \(max(1, rotationWaitMs / 60_000)) minutes after a sync server restore.") }
        switch blocked {
        case "rotationPending": notes.append("Edits wait until a key change finishes.")
        case "notActive": notes.append("This device is not active in the account, so its edits were not sent.")
        case "relayDisagrees": notes.append("The sync server disagrees about the device list, so edits were not sent.")
        default: break
        }
        return notes
    }
}
