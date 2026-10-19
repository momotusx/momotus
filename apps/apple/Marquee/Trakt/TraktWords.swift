/// Trakt copy, the same as apps/shared/trakt-words.json (the web and Android
/// read that file; TraktTests checks this one against it), then the app's own.
enum TraktWords {
    static let title = "Trakt"
    static let eyebrow = "Settings"
    static let lede = "Trakt keeps a record of what you watch. With it connected, what you play here reaches Trakt, and what you watched in other Trakt apps shows up here as watched and as places to resume. Each profile connects its own account."
    static let rowOff = "Keep watched marks and resume points in step with your Trakt account."
    static let connected = "Connected as {user}"
    static let connectedNoUser = "Connected to Trakt"
    static let failing = "Trakt has not answered since {time}: {reason}. Marquee keeps trying, and nothing waiting here is lost."
    static let reconnect = "Trakt needs you to sign in again ({reason}). Until then nothing is sent or brought in."
    static let off = "Not connected."
    static let thisDeviceOff = "This profile uses Trakt on another device, but this device is not signed in, so what you play here does not reach Trakt."
    static let notConfigured = "Trakt is not set up in this build of Marquee, so nothing is sent to Trakt."
    static let connect = "Connect Trakt"
    static let signInAgain = "Sign in again"
    static let signInHere = "Sign in on this device"
    static let starting = "Asking Trakt for a code"
    static let open = "On your phone or computer, open"
    static let scanOrOpen = "Scan the code with your phone, or open"
    static let enterCode = "and enter this code:"
    static let goodFor = "Good for {time}"
    static let openPage = "Open the page"
    static let saving = "Saving the sign-in"
    static let expired = "The code ran out before it was entered. Get a new code to try again."
    static let denied = "The sign-in was turned down on Trakt, so nothing was connected."
    static let failed = "The sign-in did not finish: {reason}."
    static let newCode = "Get a new code"
    static let cancel = "Cancel"
    static let close = "Close"
    static let lastSynced = "Last synced {time}"
    static let neverSynced = "Not synced with Trakt yet"
    static let pendingNone = "Nothing waiting to go to Trakt"
    static let pendingOne = "1 change waiting to go to Trakt"
    static let pendingMany = "{count} changes waiting to go to Trakt"
    static let importing = "Bringing in your Trakt history: {done} of {total} shows"
    static let lastError = "Last error: {error}"
    static let syncNow = "Sync now"
    static let syncing = "Syncing"
    static let synced = "Synced: {pushed} sent, {applied} brought in"
    static let syncFailed = "Sync did not work: {reason}"
    static let signOut = "Sign out"
    static let signOutAsk = "Sign out of Trakt on this device? Watched marks and progress stay here. Changes that have not reached Trakt yet are dropped."
    static let signedOut = "Signed out of Trakt on this device."
    static let signOutFailed = "Not signed out: {reason}"
    static let memoryOnly = "Kept for this session only: this device could not store the sign-in. Sign in again after the app restarts."
    static let unmappedTitle = "Not matched"
    static let unmappedLede = "Trakt and Marquee could not agree on these, so they are not sent or brought in."
    static let playerOn = "Trakt: this play is recorded"
    static let playerUnmapped = "Trakt: this title could not be matched, so this play is not recorded"
    static let playerThisDeviceOff = "Trakt: this device is not signed in, so this play is not recorded"

    /// Every shared word by its key, for the parity test.
    static let table: [String: String] = [
        "title": title,
        "eyebrow": eyebrow,
        "lede": lede,
        "rowOff": rowOff,
        "connected": connected,
        "connectedNoUser": connectedNoUser,
        "failing": failing,
        "reconnect": reconnect,
        "off": off,
        "thisDeviceOff": thisDeviceOff,
        "notConfigured": notConfigured,
        "connect": connect,
        "signInAgain": signInAgain,
        "signInHere": signInHere,
        "starting": starting,
        "open": open,
        "scanOrOpen": scanOrOpen,
        "enterCode": enterCode,
        "goodFor": goodFor,
        "openPage": openPage,
        "saving": saving,
        "expired": expired,
        "denied": denied,
        "failed": failed,
        "newCode": newCode,
        "cancel": cancel,
        "close": close,
        "lastSynced": lastSynced,
        "neverSynced": neverSynced,
        "pendingNone": pendingNone,
        "pendingOne": pendingOne,
        "pendingMany": pendingMany,
        "importing": importing,
        "lastError": lastError,
        "syncNow": syncNow,
        "syncing": syncing,
        "synced": synced,
        "syncFailed": syncFailed,
        "signOut": signOut,
        "signOutAsk": signOutAsk,
        "signedOut": signedOut,
        "signOutFailed": signOutFailed,
        "memoryOnly": memoryOnly,
        "unmappedTitle": unmappedTitle,
        "unmappedLede": unmappedLede,
        "playerOn": playerOn,
        "playerUnmapped": playerUnmapped,
        "playerThisDeviceOff": playerThisDeviceOff,
    ]
}

/// The app's own lines, after the shared ones.
extension TraktWords {
    static let loading = "Reading Trakt's state"
    static let waiting = "Waiting for you to approve Marquee on Trakt."
    static let signingOut = "Signing out"
    static let keptHere = "The sign-in is kept on this device only, in its Keychain."
    static let deleteFailed = "Signed out of Trakt, but this device could not delete the sign-in it keeps. Try again."
    static let tryAgain = "Try again"
    static let importLabel = "Trakt history brought in"
    static let tvOnly = "Connect Trakt here with the code: approve it on your phone or computer."
}

/// What the Trakt card does in each state, kept apart from the screens so it
/// is tested (the web's lib/trakt.ts).
enum TraktRules {
    enum Action: Equatable {
        case connect, signInHere, signInAgain, syncNow, signOut
    }

    /// Fills `{name}` fields.
    static func fill(_ line: String, _ fields: [String: String]) -> String {
        fields.reduce(line) { $0.replacingOccurrences(of: "{\($1.key)}", with: $1.value) }
    }

    /// "4:05" left of the code, never below zero.
    static func timeLeft(expiresMs: Int64, nowMs: Int64) -> String {
        let s = max(0, Int((Double(expiresMs - nowMs) / 1000).rounded(.up)))
        return String(format: "%d:%02d", s / 60, s % 60)
    }

    /// What the sign-in step says, by phase (the code step has its own layout).
    static func loginLine(_ login: TraktLoginSnapshot) -> String {
        switch login {
        case .starting: TraktWords.starting
        case .code: TraktWords.waiting
        case .done(let user): fill(TraktWords.connected, ["user": user])
        case .expired: TraktWords.expired
        case .denied: TraktWords.denied
        case .notConfigured: TraktWords.notConfigured
        case .failed(let reason): fill(TraktWords.failed, ["reason": reason])
        }
    }

    /// A sign-in that ended without connecting offers a new code (never a build without Trakt).
    static func loginRetry(_ login: TraktLoginSnapshot) -> Bool {
        switch login {
        case .expired, .denied, .failed: true
        default: false
        }
    }

    /// The card's line, its detail and its actions for a state. `clock` formats a time of day.
    static func card(_ status: TraktStatus, clock: (Int64) -> String) -> (line: String, detail: String?, actions: [Action]) {
        switch status.state {
        case .connected:
            (status.user.map { fill(TraktWords.connected, ["user": $0]) } ?? TraktWords.connectedNoUser, nil, [.syncNow, .signOut])
        case .failing(let since, let reason):
            (fill(TraktWords.failing, ["time": clock(since), "reason": reason]),
             status.lastError.map { fill(TraktWords.lastError, ["error": $0]) }, [.syncNow, .signOut])
        case .reconnect(let reason):
            (fill(TraktWords.reconnect, ["reason": reason]), nil, [.signInAgain, .signOut])
        case .off: (TraktWords.off, nil, [.connect])
        case .thisDeviceOff: (TraktWords.thisDeviceOff, nil, [.signInHere])
        case .notConfigured: (TraktWords.notConfigured, nil, [])
        }
    }

    /// Settings' line: off (or not read yet) invites, the other states say themselves.
    static func rowLine(_ status: TraktStatus?, clock: (Int64) -> String) -> String {
        guard let status, status.state != .off else { return TraktWords.rowOff }
        return card(status, clock: clock).line
    }

    /// Writes still waiting, in words, or nothing.
    static func pendingLine(_ count: Int) -> String? {
        count <= 0 ? nil : count == 1 ? TraktWords.pendingOne : fill(TraktWords.pendingMany, ["count": "\(count)"])
    }

    static func syncedLine(_ report: TraktReport) -> String {
        fill(TraktWords.synced, ["pushed": "\(report.pushed)", "applied": "\(report.applied)"])
    }

    /// The player's mark: none when the profile does not use Trakt; a warning says itself on screen.
    static func playbackMark(_ trakt: TraktPlayback?) -> (warn: Bool, label: String)? {
        switch trakt {
        case .on: (false, TraktWords.playerOn)
        case .unmapped: (true, TraktWords.playerUnmapped)
        case .thisDeviceOff: (true, TraktWords.playerThisDeviceOff)
        case .off, nil: nil
        }
    }

    /// The code read letter by letter, once, when it shows.
    static func spokenCode(_ code: String) -> String {
        "Code \(code.map(String.init).joined(separator: " "))"
    }
}
