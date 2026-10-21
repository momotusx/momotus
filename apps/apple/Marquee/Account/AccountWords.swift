import Foundation

/// The account words (apps/shared/account-words.json), bundled as they are
/// and shared with the web and Android: every key decodes, and a key the
/// file adds or drops fails the reader test (AccountWordsTests).
struct AccountWords: Decodable, Sendable {
    let announceDevice: String
    let announceDeviceUnnamed: String
    let announcePasskey: String
    let announceKit: String
    let announceRemoved: String
    let announceRemovedUnnamed: String
    let announceRemovedPasskey: String
    let announceRemovedKit: String
    let announceLeft: String
    let announceLeftUnnamed: String
    let announceDropped: String
    let announceUnnamedDevice: String
    let announceGoneDevice: String
    let announceNotYou: String
    let announceOk: String
    let providerAccount: [String: String]
    let providerAccountOther: String
    let signInTitle: String
    let continueWith: String
    let otherWays: String
    let usePasskey: String
    let scanFromDevice: String
    let useRecoveryKit: String
    let useOwnServer: String
    let notNow: String
    let passkeyHere: String
    let createAccount: String
    let alreadyUse: String
    let libraryAdded: String
    let cantUseAny: String
    let tvSignIn: String
    let tvSignInHow: String
    let tvWithoutAccount: String
    let signInAnother: String
    let approveWarning: String
    let scanAgainHere: String
    let codeFound: String
    let usePhoneToSignIn: String
    let passkeyLabel: String
    let recoveryKitName: String
    let joinedWithPasskey: String
    let lastUsed: String
    let givingAway: String
    let kitHere: String
    let kitDropped: String
    let tooManyKits: String
    let kitTaken: String
    let plainLink: String
    let debridConnected: String
    let traktConnected: String
    let unlinkAnyway: String
    let deleteAnyway: String
    let accountGone: String
    let accountGoneActions: [String]
    let deletionScheduled: String
    let deletionScheduledBy: String
    let deletionCancel: String
    let deletionCancelUnlink: String
    let secondWayIn: String
    let noSecondWayIn: String
    let noSecondWayInActions: [String]
    let removeLibraryAsk: String
    let removeLibraryDebrid: String
    let reportLostRemove: [String]
    let unreachable: String
    let unreachableActions: [String]
    let noRedirect: String
    let noRedirectActions: [String]
    let passkeyCancelled: String
    let passkeyCancelledActions: [String]
    let kitUnknownPasskey: String
    let kitUnknownPasskeyActions: [String]
    let startOverWarning: String
    let ticketExpired: String
    let ticketExpiredActions: [String]
    let signupLimit: String
    let signupLimitActions: [String]
    let linkedElsewhere: String
    let linkedElsewhereActions: [String]
    let busy: String
    let otherRelay: String
    let otherRelayCode: String
    let otherRelayCodeActions: [String]
    let notOwner: String

    /// Every key the file holds but its "about".
    static let keys: Set<String> = [
        "announceDevice", "announceDeviceUnnamed", "announcePasskey", "announceKit", "announceRemoved", "announceRemovedUnnamed", "announceRemovedPasskey", "announceRemovedKit", "announceLeft", "announceLeftUnnamed", "announceDropped", "announceUnnamedDevice", "announceGoneDevice", "announceNotYou", "announceOk", "providerAccount", "providerAccountOther", "signInTitle", "continueWith", "otherWays", "usePasskey", "scanFromDevice", "useRecoveryKit", "useOwnServer", "notNow", "passkeyHere", "createAccount", "alreadyUse", "libraryAdded", "cantUseAny", "tvSignIn", "tvSignInHow", "tvWithoutAccount", "signInAnother", "approveWarning", "scanAgainHere", "codeFound", "usePhoneToSignIn", "passkeyLabel", "recoveryKitName", "joinedWithPasskey", "lastUsed", "givingAway", "kitHere", "kitDropped", "tooManyKits", "kitTaken", "plainLink", "debridConnected", "traktConnected", "unlinkAnyway", "deleteAnyway", "accountGone", "accountGoneActions", "deletionScheduled", "deletionScheduledBy", "deletionCancel", "deletionCancelUnlink", "secondWayIn", "noSecondWayIn", "noSecondWayInActions", "removeLibraryAsk", "removeLibraryDebrid", "reportLostRemove", "unreachable", "unreachableActions", "noRedirect", "noRedirectActions", "passkeyCancelled", "passkeyCancelledActions", "kitUnknownPasskey", "kitUnknownPasskeyActions", "startOverWarning", "ticketExpired", "ticketExpiredActions", "signupLimit", "signupLimitActions", "linkedElsewhere", "linkedElsewhereActions", "busy", "otherRelay", "otherRelayCode", "otherRelayCodeActions", "notOwner",
    ]

    /// The bundled file: the app's own bundle, or the test bundle that compiles this file.
    static let shared: AccountWords = {
        guard let url = Bundle(for: AccountWordsToken.self).url(forResource: "account-words", withExtension: "json"),
              let data = try? Data(contentsOf: url),
              let words = try? JSONDecoder().decode(AccountWords.self, from: data)
        else { fatalError("account-words.json is bundled with the app (project.yml)") }
        return words
    }()
}

private final class AccountWordsToken {}
