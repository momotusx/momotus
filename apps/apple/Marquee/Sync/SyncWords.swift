import Foundation

/// A sync failure in plain words; `code` names the ones with a fix.
struct SyncProblem: Equatable {
    let code: String?
    let message: String

    var words: String {
        switch code {
        case "dropped": "This device was removed from your sync account. Your library stays here as local data."
        case "fork": "The sync server lists different devices than this device knows. Use the server's list to keep syncing."
        case "wrongRelay": "This is not the sync server this device joined. Try again, or forget the account here and pair again."
        case "relayBehind": "The sync server lost device entries it cannot get back. Pair this device again from one that still syncs."
        case "notJoined": "This device has not joined a sync account yet."
        case "unsent": "Some changes have not been sent yet. Sync again first, or leave anyway and they stay only here."
        case "insecure": "The sync server address must start with https."
        // The app's own: the Keychain kept a new record from it, so nothing is sent until it is stored.
        case "storage": "This device could not store the sync account's key, so sync is paused. Free some storage, then sync again."
        // The core's transport errors read "network: ..." with a URL; nobody needs that.
        case nil where message.hasPrefix("network"):
            "The sync server did not answer. Your changes wait here and go out on the next sync."
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
        if waiting > 0 { notes.append(waiting == 1 ? "1 edit waits for the sync server's clock to catch up." : "\(waiting) edits wait for the sync server's clock to catch up.") }
        if parked > 0 { notes.append(parked == 1 ? "1 record waits for a key this device does not have yet." : "\(parked) records wait for a key this device does not have yet.") }
        if oldEpochWrites > 0 { notes.append("The sync server may be hiding device changes. Compare the fingerprint on each device.") }
        if rotationWaitMs > 0 {
            let minutes = max(1, rotationWaitMs / 60_000)
            notes.append("Changes wait \(minutes) \(minutes == 1 ? "minute" : "minutes") after a sync server restore.")
        }
        switch blocked {
        case "rotationPending": notes.append("Edits wait until a key change finishes.")
        case "notActive": notes.append("This device is not active in the account, so its edits were not sent.")
        case "relayDisagrees": notes.append("The sync server disagrees about the device list, so edits were not sent.")
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
