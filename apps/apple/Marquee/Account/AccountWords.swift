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
    let removePasskeyAsk: String
    let announceOk: String
    let providerAccount: [String: String]
    let providerAccountOther: String
    let providerName: [String: String]
    let signInTitle: String
    let continueWith: String
    let `continue`: String
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
    let switchProfile: String
    let signInAnother: String
    let approveWarning: String
    let scanAgainHere: String
    let codeFound: String
    let usePhoneToSignIn: String
    let tvScanHow: String
    let tvQrLabel: String
    let browseFirst: String
    let tvSignedInFrom: String
    let approveAsk: String
    let approveAskTv: String
    let approveAskOther: String
    let approveUnnamed: String
    let approveButton: String
    let approveNear: String
    let approveScanHere: String
    let approveSigningIn: String
    let approveDone: String
    let approveNotYou: String
    let approveSlow: String
    let approveAnswered: String
    let approveExpired: String
    let approveKids: String
    let phoneScanHow: String
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
    let signupLimitNetwork: String
    let linkedElsewhere: String
    let linkedElsewhereActions: [String]
    let busy: String
    let otherRelay: String
    let otherRelayCode: String
    let otherRelayCodeActions: [String]
    let signInRefused: String
    let signInWindow: String
    let popupBlocked: String
    let returnClose: String
    let ownServerLabel: String
    let ownServerUse: String
    let foundWeb: String
    let startOver: String
    let signedInWith: String
    let linkAnother: String
    let connectProvider: String
    let connectProviderText: String
    let linkedOn: String
    let unlink: String
    let savePasskeyHere: String
    let unlinkLast: String
    let appleRevokeFailed: String
    let wiping: String
    let wipeFailed: String
    let secondWayInText: String
    let notOwner: String
    let continueWithEmail: String
    let emailTitle: String
    let emailLabel: String
    let emailSend: String
    let badAddress: String
    let codeTitle: String
    let codeSentTo: String
    let codeLabel: String
    let codeResent: String
    let codeMissing: String
    let codeResend: String
    let codeResendIn: String
    let codeOtherAddress: String
    let wrongCode: String
    let codeExpired: String
    let codeExpiredActions: [String]
    let emailWait: String
    let emailLimit: String
    let emailLimitActions: [String]
    let mailUnavailable: String
    let mailUnavailableActions: [String]
    let deletionScheduledByEmail: String
    let deletionCancelUnlinkEmail: String

    /// Every key the file holds but its "about".
    static let keys: Set<String> = [
        "announceDevice", "announceDeviceUnnamed", "announcePasskey", "announceKit", "announceRemoved", "announceRemovedUnnamed", "announceRemovedPasskey", "announceRemovedKit", "announceLeft", "announceLeftUnnamed", "announceDropped", "announceUnnamedDevice", "announceGoneDevice", "announceNotYou", "removePasskeyAsk", "announceOk", "providerAccount", "providerAccountOther", "providerName", "signInTitle", "continueWith", "continue", "otherWays", "usePasskey", "scanFromDevice", "useRecoveryKit", "useOwnServer", "notNow", "passkeyHere", "createAccount", "alreadyUse", "libraryAdded", "cantUseAny", "tvSignIn", "tvSignInHow", "tvWithoutAccount", "switchProfile", "signInAnother", "approveWarning", "scanAgainHere", "codeFound", "usePhoneToSignIn", "tvScanHow", "tvQrLabel", "browseFirst", "tvSignedInFrom", "approveAsk", "approveAskTv", "approveAskOther", "approveUnnamed", "approveButton", "approveNear", "approveScanHere", "approveSigningIn", "approveDone", "approveNotYou", "approveSlow", "approveAnswered", "approveExpired", "approveKids", "phoneScanHow", "passkeyLabel", "recoveryKitName", "joinedWithPasskey", "lastUsed", "givingAway", "kitHere", "kitDropped", "tooManyKits", "kitTaken", "plainLink", "debridConnected", "traktConnected", "unlinkAnyway", "deleteAnyway", "accountGone", "accountGoneActions", "deletionScheduled", "deletionScheduledBy", "deletionCancel", "deletionCancelUnlink", "secondWayIn", "noSecondWayIn", "noSecondWayInActions", "removeLibraryAsk", "removeLibraryDebrid", "reportLostRemove", "unreachable", "unreachableActions", "noRedirect", "noRedirectActions", "passkeyCancelled", "passkeyCancelledActions", "kitUnknownPasskey", "kitUnknownPasskeyActions", "startOverWarning", "ticketExpired", "ticketExpiredActions", "signupLimit", "signupLimitActions", "signupLimitNetwork", "linkedElsewhere", "linkedElsewhereActions", "busy", "otherRelay", "otherRelayCode", "otherRelayCodeActions", "signInRefused", "signInWindow", "popupBlocked", "returnClose", "ownServerLabel", "ownServerUse", "foundWeb", "startOver", "signedInWith", "linkAnother", "connectProvider", "connectProviderText", "linkedOn", "unlink", "savePasskeyHere", "unlinkLast", "appleRevokeFailed", "wiping", "wipeFailed", "secondWayInText", "notOwner",
        "continueWithEmail", "emailTitle", "emailLabel", "emailSend", "badAddress", "codeTitle", "codeSentTo", "codeLabel", "codeResent", "codeMissing", "codeResend", "codeResendIn", "codeOtherAddress", "wrongCode", "codeExpired", "codeExpiredActions", "emailWait", "emailLimit", "emailLimitActions", "mailUnavailable", "mailUnavailableActions", "deletionScheduledByEmail", "deletionCancelUnlinkEmail",
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
